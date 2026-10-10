//! The outcome of an assertion, with what was measured against what was
//! expected.

use std::fmt;

use kitest_scope::{Expectation, Region};

use super::{Signal, Tolerance};

/// Whether an assertion passed, and a sentence saying why.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    passed: bool,
    message: String,
    regions: Vec<Part>,
}

/// One part of a check, with the region it holds a trace to.
#[derive(Debug, Clone, PartialEq)]
struct Part {
    region: Region,
    passed: bool,
    message: String,
}

impl Check {
    /// A check with a message written by the caller.
    pub fn new(passed: bool, message: String) -> Self {
        Self {
            passed,
            message,
            regions: Vec::new(),
        }
    }

    /// A check that passes when every one of `checks` passes, with their
    /// messages and regions in order.
    pub fn all(checks: impl IntoIterator<Item = Check>) -> Self {
        let mut all = Self::new(true, String::new());
        for check in checks {
            all.passed &= check.passed;
            if !all.message.is_empty() {
                all.message.push_str("; ");
            }
            all.message.push_str(&check.message);
            all.regions.extend(check.regions);
        }
        all
    }

    /// Whether `measured` is within `tolerance` of `expected`, in `unit`.
    pub fn near(
        measured: f64,
        expected: f64,
        tolerance: Tolerance,
        unit: &str,
    ) -> Self {
        let passed = (measured - expected).abs() <= tolerance.band(expected);
        let relation = if passed { "within" } else { "outside" };
        Self::new(
            passed,
            format!(
                "{} is {} from {}, {relation} {}",
                si(measured, unit),
                si((measured - expected).abs(), unit),
                si(expected, unit),
                describe(tolerance, expected, unit),
            ),
        )
    }

    /// Whether `signal`, in volts, stays within `tolerance` of `target`
    /// over the last `window` seconds.
    pub fn settles(
        signal: &Signal,
        target: f64,
        tolerance: Tolerance,
        window: f64,
    ) -> Self {
        let passed = signal.settles_to(target, tolerance, window);
        let message = match signal.worst_deviation(target, window) {
            Some(worst) => format!(
                "over the last {}, the signal strays up to {} from {}, {} {}",
                si(window, "s"),
                si(worst, "V"),
                si(target, "V"),
                if passed { "within" } else { "outside" },
                describe(tolerance, target, "V"),
            ),
            None => {
                let span = match (signal.time().first(), signal.time().last()) {
                    (Some(first), Some(last)) => si(last - first, "s"),
                    _ => si(0.0, "s"),
                };
                format!(
                    "the signal spans {span}, shorter than the {} window",
                    si(window, "s")
                )
            }
        };
        let check = Self::new(passed, message);
        signal
            .time()
            .first()
            .zip(signal.time().last())
            .and_then(|(&first, &last)| {
                (last - first >= window).then(|| {
                    let band = tolerance.band(target);
                    Region::Band {
                        start: last - window,
                        end: last,
                        low: target - band,
                        high: target + band,
                    }
                })
            })
            .map_or(check.clone(), |region| check.with_region(region))
    }

    /// Whether the assertion passed.
    pub fn passed(&self) -> bool {
        self.passed
    }

    /// What was measured against what was expected.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Adds where the check holds the trace.
    pub fn with_region(mut self, region: Region) -> Self {
        self.regions.push(Part {
            region,
            passed: self.passed,
            message: self.message.clone(),
        });
        self
    }

    /// The expectations to draw on `trace`, one per region, each with its own
    /// verdict; empty when the check holds no region.
    pub fn expectations(&self, trace: &str) -> Vec<Expectation> {
        self.regions
            .iter()
            .map(|part| Expectation {
                trace: trace.to_owned(),
                passed: part.passed,
                message: part.message.clone(),
                region: part.region,
            })
            .collect()
    }
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// `tolerance` around `target`, as `±25 mV` or `±1% (25 mV)`.
fn describe(tolerance: Tolerance, target: f64, unit: &str) -> String {
    match tolerance {
        Tolerance::Abs(v) => format!("±{}", si(v, unit)),
        Tolerance::Percent(p) => {
            format!("±{p}% ({})", si(tolerance.band(target), unit))
        }
    }
}

/// `value` with an SI prefix and up to four decimals, such as `10.1496 MHz`.
pub(crate) fn si(value: f64, unit: &str) -> String {
    const PREFIXES: [&str; 9] = ["p", "n", "µ", "m", "", "k", "M", "G", "T"];
    if value == 0.0 || !value.is_finite() {
        return format!("{value} {unit}").trim_end().to_owned();
    }
    let power = (value.abs().log10() / 3.0).floor().clamp(-4.0, 4.0);
    let scaled = value / 1000f64.powf(power);
    let digits = format!("{scaled:.4}");
    let digits = digits.trim_end_matches('0').trim_end_matches('.');
    let prefix = PREFIXES[(power + 4.0) as usize];
    format!("{digits} {prefix}{unit}").trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn si_picks_the_prefix_for_the_magnitude() {
        assert_eq!(si(10_149_552.89, "Hz"), "10.1496 MHz");
        assert_eq!(si(0.025, "V"), "25 mV");
        assert_eq!(si(-2.5, "V"), "-2.5 V");
        assert_eq!(si(0.0, "V"), "0 V");
        assert_eq!(si(999.99999, "V"), "1000 V");
    }

    #[test]
    fn near_passes_on_the_band_edge_and_fails_past_it() {
        assert!(Check::near(2.525, 2.5, Tolerance::percent(1.0), "V").passed());
        assert!(!Check::near(2.53, 2.5, Tolerance::percent(1.0), "V").passed());
    }

    #[test]
    fn settles_carries_its_time_and_voltage_band_into_an_expectation() {
        let time = [0.0, 1.0, 2.0, 3.0];
        let values = [0.0, 0.96, 0.98, 1.0];
        let check = Check::settles(
            &Signal::new(&time, &values),
            1.0,
            Tolerance::abs(0.05),
            2.0,
        );
        let [expectation] = check.expectations("out").try_into().unwrap();
        assert!(expectation.passed);
        assert_eq!(expectation.trace, "out");
        assert_eq!(
            expectation.region,
            Region::Band {
                start: 1.0,
                end: 3.0,
                low: 0.95,
                high: 1.05,
            }
        );
    }

    #[test]
    fn all_keeps_each_parts_own_verdict_and_region() {
        let frequency = Region::Frequency {
            low: 9.0,
            high: 11.0,
        };
        let distortion = Region::Distortion {
            fundamental: 10.0,
            amplitude: 1.0,
            maximum: 0.05,
        };
        let check = Check::all([
            Check::new(true, "in band".into()).with_region(frequency),
            Check::new(false, "too distorted".into()).with_region(distortion),
        ]);
        assert!(!check.passed());
        assert_eq!(check.message(), "in band; too distorted");
        let expectations = check.expectations("out");
        assert_eq!(expectations.len(), 2);
        assert!(expectations[0].passed && !expectations[1].passed);
        assert_eq!(expectations[1].message, "too distorted");
        assert_eq!(expectations[1].region, distortion);
    }
}
