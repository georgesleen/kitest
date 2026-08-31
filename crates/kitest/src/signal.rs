//! Waveform views and assertions over [Results].

use num_complex::Complex64;

/// A time-domain waveform: real values over a time axis.
pub struct Signal<'a> {
    time: &'a [f64],
    values: &'a [f64],
}

impl<'a> Signal<'a> {
    pub fn new(time: &'a [f64], values: &'a [f64]) -> Self {
        Self { time, values }
    }

    /// The time axis, in seconds.
    pub fn time(&self) -> &[f64] {
        self.time
    }

    /// The sampled values.
    pub fn values(&self) -> &[f64] {
        self.values
    }

    /// True if signal stays within `tolerance` of `target` over the last `window` seconds.
    pub fn settles_to(
        &self,
        target: f64,
        tolerance: Tolerance,
        window: f64,
    ) -> bool {
        let band = tolerance.band(target);
        let (Some(&first), Some(&last)) = (self.time.first(), self.time.last())
        else {
            return false;
        };
        if last - first < window {
            return false;
        }
        let start = last - window;
        self.time
            .iter()
            .zip(self.values)
            .filter(|(t, _)| **t >= start)
            .all(|(_, v)| (*v - target).abs() <= band)
    }

    /// Peak value above `target`, as a fraction of `target`.
    pub fn overshoot(&self, target: f64) -> f64 {
        match self.values.iter().copied().reduce(f64::max) {
            Some(peak) => ((peak - target) / target.abs()).max(0.0),
            None => f64::NAN,
        }
    }
}

/// A frequency-domain response: complex values over a (real) frequency axis.
pub struct Response<'a> {
    frequency: &'a [f64],
    values: &'a [Complex64],
}

impl<'a> Response<'a> {
    /// Decibels per decade of amplitude ratio.
    const DB_PER_DECADE: f64 = 20.0;

    pub fn new(frequency: &'a [f64], values: &'a [Complex64]) -> Self {
        Self { frequency, values }
    }

    /// The frequency axis, in Hz.
    pub fn frequency(&self) -> &[f64] {
        self.frequency
    }

    /// The complex response values.
    pub fn values(&self) -> &[Complex64] {
        self.values
    }

    /// Index of the sweep point whose frequency is nearest `frequency` Hz.
    fn nearest(&self, frequency: f64) -> Option<usize> {
        (0..self.frequency.len()).min_by(|&a, &b| {
            (self.frequency[a] - frequency)
                .abs()
                .total_cmp(&(self.frequency[b] - frequency).abs())
        })
    }

    /// Gain in dB at the sweep point nearest `frequency`.
    pub fn gain_db_at(&self, frequency: f64) -> Option<f64> {
        let i = self.nearest(frequency)?;
        Some(Self::DB_PER_DECADE * self.values[i].norm().log10())
    }

    /// Phase in degrees at the sweep point nearest `frequency`.
    pub fn phase_deg_at(&self, frequency: f64) -> Option<f64> {
        let i = self.nearest(frequency)?;
        Some(self.values[i].arg().to_degrees())
    }
}

/// Single value associated with a node at an operating point.
#[derive(Debug, Clone, Copy)]
pub struct Voltage {
    volts: f64,
}

impl Voltage {
    pub(crate) fn new(volts: f64) -> Self {
        Self { volts }
    }

    /// Return the value of the Voltage at a node.
    pub fn volts(&self) -> f64 {
        self.volts
    }

    /// Checks that the voltage at a node is within the specified tolerance
    pub fn near(&self, expected: f64, tolerance: Tolerance) -> bool {
        (self.volts - expected).abs() <= tolerance.band(expected)
    }
}

/// How close a value must be to a target.
#[derive(Debug, Clone, Copy)]
pub enum Tolerance {
    /// Absolute distance from the target.
    Abs(f64),
    /// Percentage of the target.
    Pct(f64),
}

impl Tolerance {
    /// An absolute tolerance of `v`.
    pub fn abs(v: f64) -> Self {
        Self::Abs(v)
    }

    /// A tolerance of `p` percent of the target.
    pub fn pct(p: f64) -> Self {
        Self::Pct(p)
    }

    /// The allowed distance from `target`: absolute as-is, percent of `|target|`.
    fn band(self, target: f64) -> f64 {
        match self {
            Self::Abs(v) => v,
            Self::Pct(p) => p / 100.0 * target.abs(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIME: [f64; 5] = [0.0, 1.0, 2.0, 3.0, 4.0];

    #[test]
    fn settles_when_tail_in_band() {
        let values = [0.0, 0.6, 0.97, 0.99, 1.0];
        let s = Signal::new(&TIME, &values);
        assert!(s.settles_to(1.0, Tolerance::abs(0.05), 2.0));
    }

    #[test]
    fn rejects_ringing_tail() {
        let values = [0.0, 0.6, 1.1, 0.9, 1.1];
        let s = Signal::new(&TIME, &values);
        assert!(!s.settles_to(1.0, Tolerance::abs(0.05), 2.0));
    }

    #[test]
    fn rejects_window_longer_than_data() {
        let values = [0.0, 0.6, 0.97, 0.99, 1.0];
        let s = Signal::new(&TIME, &values);
        assert!(!s.settles_to(1.0, Tolerance::abs(0.05), 10.0));
    }

    #[test]
    fn rejects_empty_signal() {
        let s = Signal::new(&[], &[]);
        assert!(!s.settles_to(1.0, Tolerance::abs(0.05), 1.0));
    }

    #[test]
    fn overshoot_measures_peak_above_target() {
        let values = [0.0, 1.2, 1.0];
        let s = Signal::new(&TIME[..3], &values);
        assert!((s.overshoot(1.0) - 0.2).abs() < 1e-9);
    }

    #[test]
    fn overshoot_zero_when_monotonic() {
        let values = [0.0, 0.5, 1.0];
        let s = Signal::new(&TIME[..3], &values);
        assert_eq!(s.overshoot(1.0), 0.0);
    }

    const FREQ: [f64; 2] = [1.0, 10.0];

    #[test]
    fn gain_db_reads_nearest_point() {
        let values = [Complex64::new(1.0, 0.0), Complex64::new(0.1, 0.0)];
        let r = Response::new(&FREQ, &values);
        assert!((r.gain_db_at(1.0).unwrap()).abs() < 1e-9);
        assert!((r.gain_db_at(9.0).unwrap() + 20.0).abs() < 1e-9);
    }

    #[test]
    fn phase_deg_reads_angle() {
        let values = [Complex64::new(0.0, 1.0), Complex64::new(0.0, -1.0)];
        let r = Response::new(&FREQ, &values);
        assert!((r.phase_deg_at(1.0).unwrap() - 90.0).abs() < 1e-9);
        assert!((r.phase_deg_at(10.0).unwrap() + 90.0).abs() < 1e-9);
    }

    #[test]
    fn response_empty_is_none() {
        let r = Response::new(&[], &[]);
        assert!(r.gain_db_at(1.0).is_none());
    }

    #[test]
    fn near_accepts_value_inside_absolute_band() {
        let v = Voltage::new(2.49);
        assert!(v.near(2.5, Tolerance::abs(0.05)));
    }

    #[test]
    fn near_rejects_value_outside_absolute_band() {
        let v = Voltage::new(2.4);
        assert!(!v.near(2.5, Tolerance::abs(0.05)));
    }

    #[test]
    fn near_uses_percent_of_target() {
        // 1% of 5.0 is a 0.05 band around the target.
        assert!(Voltage::new(4.96).near(5.0, Tolerance::pct(1.0)));
        assert!(!Voltage::new(4.9).near(5.0, Tolerance::pct(1.0)));
    }

    #[test]
    fn volts_returns_raw_value() {
        assert_eq!(Voltage::new(3.3).volts(), 3.3);
    }
}
