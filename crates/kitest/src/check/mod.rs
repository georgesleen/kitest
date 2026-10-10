//! Running every probe's expectation against a design and reporting each.

mod diagnosis;
mod expect;

pub use expect::{ExpectError, Expectation, parse as parse_expectation};

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use kitest_measure::Curve;
use kitest_scope::Region;

use crate::analysis::si;
use crate::kicad::{is_ground, is_spice_ground, node_name};
use crate::{
    Backend, Check, Component, Config, ConfigError, Corner, Design, KicadError,
    ModelError, ModelLibrary, Net, Netlist, NetlistError, Ngspice, Power,
    Probe, ProbeError, Signal, SupplyError, Tolerance, Tran, TranSource,
    Transient, export_design,
};

/// Output samples per expected cycle.
const SAMPLES_PER_CYCLE: f64 = 100.0;
/// Smallest amplitude, zero to peak, counted as oscillating.
const MIN_OSCILLATION_VOLTS: f64 = 1e-3;
/// Run lengths tried in turn, in expected cycles, until the envelope settles.
const RUN_CYCLES: [f64; 3] = [200.0, 800.0, 3200.0];
/// Cycles per envelope window.
const WINDOW_CYCLES: f64 = 25.0;
/// Largest change in amplitude between windows still counted as steady.
const STEADY_CHANGE: f64 = 0.01;
/// Current impulse into the probe's net that starts every oscillation run,
/// before it is sized to the net.
const KICK_AMPS: f64 = 1e-3;
/// Largest voltage step a kick may cause before it is scaled down.
const MAX_KICK_VOLTS: f64 = 0.5;
/// The step a kick is scaled to when it would exceed `MAX_KICK_VOLTS`.
const SIZED_KICK_VOLTS: f64 = 0.2;
/// Cycles after the start in which the kick's step is read.
const KICK_STEP_CYCLES: f64 = 0.1;
/// Prefixes of the names KiCad gives nets no label names.
const AUTO_NET_PREFIXES: [&str; 2] = ["Net-(", "unconnected-("];
/// Harmonics, the fundamental counted as the first, that distortion sums.
const HARMONICS: usize = 10;

/// What an oscillation check asks of a net beyond its frequency.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// The least swing, half the peak-to-peak voltage, in volts.
    min_swing: Option<f64>,
    /// The most total harmonic distortion, as a fraction of the fundamental.
    max_thd: Option<f64>,
}

/// One probe's result at one corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The probe's name.
    pub probe: String,
    /// The probe's reference designator.
    pub reference: String,
    /// The net the probe sits on.
    pub net: String,
    /// The voltage of each rail kitest sourced, by SPICE node.
    pub corner: Vec<(String, f64)>,
    /// The check, or `None` for a probe with no `Expect`.
    pub check: Option<Check>,
    /// For a failed check, what the corner's operating point says about it:
    /// the probe net's bias, each transistor's bias, and undriven supplies.
    pub diagnosis: Vec<String>,
    /// The probe's net over an oscillation check's final run.
    pub waveform: Option<Waveform>,
}

/// One net's voltage over a transient run.
#[derive(Debug, Clone, PartialEq)]
pub struct Waveform {
    pub time: Vec<f64>,
    pub volts: Vec<f64>,
    /// The frequency the check expected, in hertz.
    pub expected_hertz: f64,
}

impl Outcome {
    /// False only for a check that ran and failed.
    pub fn passed(&self) -> bool {
        self.check.as_ref().is_none_or(Check::passed)
    }
}

/// Every probe's outcome at every corner, in probe then corner order.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub outcomes: Vec<Outcome>,
}

impl Report {
    /// Whether no check failed.
    pub fn passed(&self) -> bool {
        self.outcomes.iter().all(Outcome::passed)
    }
}

/// Why probes could not be checked at all.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("{path} is not a KiCad project: {reason}")]
    Project { path: PathBuf, reason: String },

    #[error(transparent)]
    Kicad(#[from] KicadError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    Model(#[from] ModelError),

    #[error(transparent)]
    Netlist(#[from] NetlistError),

    #[error(transparent)]
    Supply(#[from] SupplyError),

    #[error(transparent)]
    Probe(#[from] ProbeError),

    #[error("{}", expect_message(.problems))]
    Expect {
        problems: Vec<(String, ExpectError)>,
    },

    #[error("the simulation failed")]
    Simulation(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Check every probe in the KiCad project at `path`, a project directory or
/// a `.kicad_sch` file, with its `kitest.toml` and ngspice.
pub fn check_project(path: &Path) -> Result<Report, CheckError> {
    let schematic = find_schematic(path)?;
    let directory = schematic.parent().unwrap_or(Path::new("."));
    let config = Config::for_project(directory)?;
    let design = export_design(&schematic)?;
    design.check(&config, &Ngspice::default())
}

impl Design {
    /// Check every probe's expectation at every corner of `config`.
    pub fn check<B>(
        &self,
        config: &Config,
        backend: &B,
    ) -> Result<Report, CheckError>
    where
        B: Backend,
        B::Error: Send + Sync,
    {
        let probes = self.probes()?;
        let expectations = expectations(&probes)?;
        let mut libraries = config
            .model_libraries
            .iter()
            .map(|path| ModelLibrary::load(path))
            .collect::<Result<Vec<_>, _>>()?;
        libraries.push(ModelLibrary::bundled());
        let netlist = self.netlist(&libraries)?;
        let power = self.power(&config.supplies)?;
        let labelled = self.labelled_nets(&power);
        let crystal = self
            .components
            .iter()
            .find(|component| !component.dnp && is_crystal(component))
            .map(|component| component.reference.as_str());

        let mut outcomes = Vec::new();
        for corner in power.corners() {
            let corner_outcomes = check_corner(
                &netlist,
                &corner,
                &probes,
                &expectations,
                &labelled,
                crystal,
                backend,
            )
            .map_err(|error| CheckError::Simulation(Box::new(error)))?;
            outcomes.extend(corner_outcomes);
        }
        outcomes.sort_by_key(|outcome| {
            probes
                .iter()
                .position(|probe| probe.reference() == outcome.reference)
        });
        Ok(Report { outcomes })
    }

    /// Nets named by a label, not by KiCad, that `power` neither sources
    /// nor grounds.
    fn labelled_nets(&self, power: &Power) -> Vec<&str> {
        self.nets
            .iter()
            .map(|net| net.name.as_str())
            .filter(|name| {
                !AUTO_NET_PREFIXES
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
                    && !is_ground(name)
                    && !is_spice_ground(name)
                    && !power.rails().iter().any(|rail| rail.net() == *name)
            })
            .collect()
    }
}

/// Each probe's parsed `Expect`, or every probe whose field is unreadable.
fn expectations(
    probes: &[Probe<'_>],
) -> Result<Vec<Option<Expectation>>, CheckError> {
    let mut parsed = Vec::new();
    let mut problems = Vec::new();
    for probe in probes {
        match probe.expect().map(expect::parse).transpose() {
            Ok(expectation) => parsed.push(expectation),
            Err(error) => problems.push((probe.reference().to_owned(), error)),
        }
    }
    if problems.is_empty() {
        Ok(parsed)
    } else {
        Err(CheckError::Expect { problems })
    }
}

fn check_corner<B: Backend>(
    netlist: &Netlist,
    corner: &Corner<'_>,
    probes: &[Probe<'_>],
    expectations: &[Option<Expectation>],
    labelled: &[&str],
    crystal: Option<&str>,
    backend: &B,
) -> Result<Vec<Outcome>, B::Error> {
    let voltages: Vec<(String, f64)> = corner
        .voltages()
        .map(|(node, volts)| (node.to_owned(), volts))
        .collect();
    let needs_op = expectations
        .iter()
        .any(|expectation| matches!(expectation, Some(Expectation::Dc { .. })));
    let mut op = if needs_op {
        Some(backend.run_op(&netlist.text, &corner.dc_supplies())?)
    } else {
        None
    };

    let mut outcomes = Vec::new();
    for (probe, expectation) in probes.iter().zip(expectations) {
        let node = &probe.net().name;
        let (check, waveform) = match *expectation {
            None => (None, None),
            Some(Expectation::Dc { near, within }) => {
                let op =
                    op.as_ref().expect("an operating point ran for a dc check");
                let check = match op.node(node) {
                    Some(voltage) => {
                        Check::near(voltage.volts(), near, within, "V")
                    }
                    None => missing(probe.net()),
                };
                (Some(check), None)
            }
            Some(Expectation::Oscillates {
                near,
                within,
                min_swing,
                max_thd,
            }) => match crystal {
                Some(crystal) => (Some(crystal_unsupported(crystal)), None),
                None => {
                    let (check, waveform) = oscillation(
                        backend,
                        &netlist.text,
                        corner.tran_sources(),
                        probe.net(),
                        (near, within),
                        Limits { min_swing, max_thd },
                    )?;
                    (Some(check), waveform)
                }
            },
        };
        let failed = check.as_ref().is_some_and(|check| !check.passed());
        let diagnosis = if failed {
            if op.is_none() {
                op =
                    Some(backend.run_op(&netlist.text, &corner.dc_supplies())?);
            }
            let op = op.as_ref().expect("an operating point ran");
            let measured_dc =
                matches!(expectation, Some(Expectation::Dc { .. }));
            let probe_net = (!measured_dc).then_some(node.as_str());
            diagnosis::diagnose(op, probe_net, &netlist.transistors, labelled)
        } else {
            Vec::new()
        };
        outcomes.push(Outcome {
            probe: probe.name().to_owned(),
            reference: probe.reference().to_owned(),
            net: probe.net().name.clone(),
            corner: voltages.clone(),
            check,
            diagnosis,
            waveform,
        });
    }
    Ok(outcomes)
}

/// Whether `net` sustains an oscillation within `within` of `near` hertz,
/// given as `(near, within)`, and meets `limits`.
///
/// The net is kicked at the start, so startup does not hang on numerical
/// noise, and the run lengthens until the amplitude is steady. A steady
/// amplitude is an oscillation; one that decays is a resonance the kick
/// rang. Decay slower than `STEADY_CHANGE` per window, a resonance with a
/// Q above about 8000 such as a crystal, still reads as steady.
fn oscillation<B: Backend>(
    backend: &B,
    netlist: &str,
    sources: Vec<TranSource>,
    net: &Net,
    (near, within): (f64, Tolerance),
    limits: Limits,
) -> Result<(Check, Option<Waveform>), B::Error> {
    let node = node_name(&net.name);
    let mut amps = KICK_AMPS;
    let mut sized = false;
    let mut last = None;
    for cycles in RUN_CYCLES {
        let tran = Tran::new(1.0 / (near * SAMPLES_PER_CYCLE), cycles / near);
        let run = |amps: f64| {
            let mut kicked = sources.clone();
            kicked.push(TranSource::kick(&node, amps));
            backend.run_tran(netlist, &kicked, tran)
        };
        let mut result = run(amps)?;
        if !sized {
            sized = true;
            let step = result
                .node(&net.name)
                .map_or(0.0, |signal| kick_step(&signal, near));
            if step > MAX_KICK_VOLTS {
                amps *= SIZED_KICK_VOLTS / step;
                result = run(amps)?;
            }
        }
        let Some(signal) = result.node(&net.name) else {
            return Ok((missing(net), None));
        };
        let envelope = Envelope::measure(&signal, near);
        if let Some(check) =
            envelope.verdict(cycles, (near, within), limits, &signal)
        {
            return Ok((check, waveform(result, net, near)));
        }
        last = Some((envelope, result));
    }
    let (envelope, result) = last.expect("at least one run");
    let cycles = RUN_CYCLES[RUN_CYCLES.len() - 1];
    let amplitude = si(envelope.amplitude(), "V");
    let change = envelope.change() * 100.0;
    let check = Check::new(
        false,
        if envelope.change() > 0.0 {
            format!(
                "still starting after {cycles} cycles: {amplitude} amplitude, \
                 growing {change:.2}% every {WINDOW_CYCLES} cycles"
            )
        } else {
            format!(
                "rings but does not sustain: {amplitude} amplitude after \
                 {cycles} cycles, falling {:.2}% every {WINDOW_CYCLES} cycles; \
                 that is a resonance, not an oscillator",
                -change
            )
        },
    );
    Ok((
        check.with_region(frequency_region(near, within)),
        waveform(result, net, near),
    ))
}

/// The frequencies `within` of `near` hertz.
fn frequency_region(near: f64, within: Tolerance) -> Region {
    let band = within.band(near);
    Region::Frequency {
        low: near - band,
        high: near + band,
    }
}

/// Whether `curve` swings at least `minimum` volts either side of its
/// centre over `over`.
fn swing(curve: Curve<'_>, over: RangeInclusive<f64>, minimum: f64) -> Check {
    let low = curve.min(over.clone()).unwrap_or(0.0);
    let high = curve.max(over.clone()).unwrap_or(0.0);
    let (swing, centre) = ((high - low) / 2.0, (high + low) / 2.0);
    let passed = swing >= minimum;
    Check::new(
        passed,
        format!(
            "swings {} either side of {}, {} the {} minimum",
            si(swing, "V"),
            si(centre, "V"),
            if passed { "above" } else { "below" },
            si(minimum, "V"),
        ),
    )
    .with_region(Region::Swing {
        start: *over.start(),
        end: *over.end(),
        centre,
        minimum,
    })
}

/// Whether `curve`'s total harmonic distortion over `over` is at most
/// `maximum`, a fraction of its fundamental.
fn distortion(
    curve: Curve<'_>,
    over: RangeInclusive<f64>,
    maximum: f64,
) -> Check {
    let samples = curve.x().len().next_power_of_two();
    let measured = curve.spectrum(over, samples).and_then(|spectrum| {
        Some((
            spectrum.dominant()?,
            spectrum.total_harmonic_distortion(HARMONICS)?,
        ))
    });
    let Some((fundamental, thd)) = measured else {
        return Check::new(
            false,
            "has no tone to measure harmonic distortion against".to_owned(),
        );
    };
    let passed = thd <= maximum;
    Check::new(
        passed,
        format!(
            "{} harmonic distortion, {} the {} maximum",
            percent(thd),
            if passed { "within" } else { "above" },
            percent(maximum),
        ),
    )
    .with_region(Region::Distortion {
        fundamental: fundamental.hertz,
        amplitude: fundamental.amplitude,
        maximum,
    })
}

/// `fraction` as a percentage with up to two decimals, such as `12.5%`.
fn percent(fraction: f64) -> String {
    let digits = format!("{:.2}", fraction * 100.0);
    format!("{}%", digits.trim_end_matches('0').trim_end_matches('.'))
}

/// `net`'s waveform out of `result`, a run expecting `near` hertz.
fn waveform(result: Transient, net: &Net, near: f64) -> Option<Waveform> {
    let (time, volts) = result.into_node(&net.name)?;
    Some(Waveform {
        time,
        volts,
        expected_hertz: near,
    })
}

/// The largest move away from the starting voltage in the first
/// `KICK_STEP_CYCLES`, which is the step the kick caused.
fn kick_step(signal: &Signal<'_>, hertz: f64) -> f64 {
    let time = signal.time();
    let values = signal.values();
    let Some(&start) = values.first() else {
        return 0.0;
    };
    let end = time.first().copied().unwrap_or(0.0) + KICK_STEP_CYCLES / hertz;
    time.iter()
        .zip(values)
        .take_while(|(t, _)| **t <= end)
        .map(|(_, v)| (v - start).abs())
        .fold(0.0, f64::max)
}

/// Amplitudes of the last three windows of a run, oldest first.
struct Envelope {
    windows: [f64; 3],
}

impl Envelope {
    fn measure(signal: &Signal<'_>, hertz: f64) -> Self {
        let time = signal.time();
        let end = time.last().copied().unwrap_or(0.0);
        let width = WINDOW_CYCLES / hertz;
        let windows = [3.0, 2.0, 1.0].map(|back| {
            let start = time.partition_point(|t| *t < end - back * width);
            let stop =
                time.partition_point(|t| *t < end - (back - 1.0) * width);
            Signal::new(&time[start..stop], &signal.values()[start..stop])
                .dominant_tone()
                .amplitude()
                .volts()
        });
        Self { windows }
    }

    /// The newest window's amplitude.
    fn amplitude(&self) -> f64 {
        self.windows[2]
    }

    /// Fractional change from the middle window to the newest.
    fn change(&self) -> f64 {
        self.windows[2] / self.windows[1] - 1.0
    }

    fn steady(&self) -> bool {
        let [a, b, c] = self.windows;
        (b / a - 1.0).abs() <= STEADY_CHANGE
            && (c / b - 1.0).abs() <= STEADY_CHANGE
    }

    /// The check this run settles, or `None` when a longer run is needed.
    ///
    /// The frequency is `within` of `near` hertz, given as `(near, within)`.
    fn verdict(
        &self,
        cycles: f64,
        (near, within): (f64, Tolerance),
        limits: Limits,
        signal: &Signal<'_>,
    ) -> Option<Check> {
        let amplitude = self.amplitude();
        if amplitude < MIN_OSCILLATION_VOLTS && self.change() <= STEADY_CHANGE {
            let decay = if self.change() < -STEADY_CHANGE {
                format!(
                    "; the kick rang it, falling {:.0}% every {WINDOW_CYCLES} \
                     cycles, so it is a resonance, not an oscillator",
                    -self.change() * 100.0
                )
            } else {
                String::new()
            };
            return Some(
                Check::new(
                    false,
                    format!(
                        "no oscillation: {} amplitude after {cycles} cycles, \
                         below the {} floor{decay}",
                        si(amplitude, "V"),
                        si(MIN_OSCILLATION_VOLTS, "V"),
                    ),
                )
                .with_region(frequency_region(near, within)),
            );
        }
        if !self.steady() {
            return None;
        }
        let time = signal.time();
        let end = time.last().copied().unwrap_or(0.0);
        let start =
            time.partition_point(|t| *t < end - 2.0 * WINDOW_CYCLES / near);
        let steady = Signal::new(&time[start..], &signal.values()[start..]);
        let tone = steady.dominant_tone();
        let frequency =
            Check::near(tone.frequency().hertz(), near, within, "Hz");
        let frequency = Check::new(
            frequency.passed(),
            format!(
                "{frequency}, at a steady {} amplitude",
                si(amplitude, "V")
            ),
        )
        .with_region(frequency_region(near, within));
        let curve = Curve::new(steady.time(), steady.values());
        let over = time.get(start).copied().unwrap_or(end)..=end;
        let swing = limits
            .min_swing
            .map(|minimum| swing(curve, over.clone(), minimum));
        let distortion = limits
            .max_thd
            .map(|maximum| distortion(curve, over, maximum));
        Some(Check::all(
            [Some(frequency), swing, distortion].into_iter().flatten(),
        ))
    }
}

/// True if `component` is a quartz crystal: a `Y` reference, as KiCad
/// annotates crystals, or a symbol from KiCad's crystal symbols.
fn is_crystal(component: &Component) -> bool {
    let mut reference = component.reference.chars();
    let y_reference = reference.next() == Some('Y')
        && reference.next().is_some_and(|c| c.is_ascii_digit());
    y_reference
        || component.library.library == "Crystal"
        || component.library.part.starts_with("Crystal")
}

/// The failed check for an oscillation probe on a design with `crystal`.
fn crystal_unsupported(crystal: &str) -> Check {
    let longest = RUN_CYCLES[RUN_CYCLES.len() - 1];
    Check::new(
        false,
        format!(
            "{crystal} is a crystal, and oscillates cannot check a crystal \
             oscillator yet: a crystal's Q of 10^4 to 10^6 needs far more \
             cycles to start than kitest's longest run of {longest}, so the \
             check would read \"still starting\" or pass a resonance that is \
             only ringing"
        ),
    )
}

fn missing(net: &Net) -> Check {
    Check::new(
        false,
        format!("net {} is not in the simulation results", net.name),
    )
}

/// The `.kicad_sch` for `path`, a schematic or a directory holding one
/// `.kicad_pro`.
fn find_schematic(path: &Path) -> Result<PathBuf, CheckError> {
    let not_project = |reason: &str| CheckError::Project {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    };
    if path
        .extension()
        .is_some_and(|extension| extension == "kicad_sch")
    {
        return Ok(path.to_path_buf());
    }
    let entries = std::fs::read_dir(path)
        .map_err(|error| not_project(&format!("cannot read it: {error}")))?;
    let mut projects: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|candidate| {
            candidate
                .extension()
                .is_some_and(|extension| extension == "kicad_pro")
        })
        .collect();
    projects.sort();
    match projects.as_slice() {
        [project] => Ok(project.with_extension("kicad_sch")),
        [] => Err(not_project("it holds no .kicad_pro file")),
        _ => Err(not_project(
            "it holds more than one .kicad_pro file; name the .kicad_sch to check",
        )),
    }
}

fn expect_message(problems: &[(String, ExpectError)]) -> String {
    let mut message = String::from("cannot read probe expectations:");
    for (reference, error) in problems {
        message.push_str(&format!("\n  - {reference} Expect: {error}"));
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    const TANK: &str = include_str!("../../../../examples/spice/tank.cir");
    const NO_LIMITS: Limits = Limits {
        min_swing: None,
        max_thd: None,
    };

    fn tank_check(near: f64) -> Check {
        let net = Net {
            name: "tank".into(),
            nodes: Vec::new(),
        };
        oscillation(
            &Ngspice::default(),
            TANK,
            vec![TranSource::dc("vcc", 9.0)],
            &net,
            (near, Tolerance::percent(5.0)),
            NO_LIMITS,
        )
        .expect("simulation ran")
        .0
    }

    #[test]
    fn a_kicked_passive_tank_is_a_resonance_not_an_oscillator() {
        let check = tank_check(10.38e6);
        assert!(!check.passed(), "{check}");
        assert!(check.message().contains("resonance"), "{check}");
    }

    /// A 1 MHz sine whose amplitude changes by `per_window` every window.
    fn sine(per_window: f64, cycles: f64) -> (Vec<f64>, Vec<f64>) {
        let hertz = 1e6;
        let samples = (cycles * SAMPLES_PER_CYCLE) as usize;
        let time: Vec<f64> = (0..samples)
            .map(|i| i as f64 / (hertz * SAMPLES_PER_CYCLE))
            .collect();
        let values = time
            .iter()
            .map(|t| {
                let windows = t * hertz / WINDOW_CYCLES;
                per_window.powf(windows)
                    * (std::f64::consts::TAU * hertz * t).sin()
            })
            .collect();
        (time, values)
    }

    fn verdict(per_window: f64) -> Option<Check> {
        let (time, values) = sine(per_window, 200.0);
        let signal = Signal::new(&time, &values);
        Envelope::measure(&signal, 1e6).verdict(
            200.0,
            (1e6, Tolerance::percent(1.0)),
            NO_LIMITS,
            &signal,
        )
    }

    #[test]
    fn a_steady_envelope_settles_the_check() {
        let check = verdict(1.0).expect("steady");
        assert!(check.passed(), "{check}");
    }

    #[test]
    fn a_growing_or_slowly_decaying_envelope_needs_a_longer_run() {
        assert_eq!(verdict(1.05), None);
        assert_eq!(verdict(0.95), None);
    }

    #[test]
    fn swing_and_distortion_are_each_judged_and_drawn_on_their_own() {
        let (time, sine) = sine(1.0, 200.0);
        let values: Vec<f64> = time
            .iter()
            .zip(&sine)
            .map(|(t, v)| {
                2.0 + v + 0.1 * (2.0 * std::f64::consts::TAU * 1e6 * t).sin()
            })
            .collect();
        let signal = Signal::new(&time, &values);
        let check = |min_swing, max_thd| {
            Envelope::measure(&signal, 1e6)
                .verdict(
                    200.0,
                    (1e6, Tolerance::percent(1.0)),
                    Limits { min_swing, max_thd },
                    &signal,
                )
                .expect("steady")
        };

        let loose = check(Some(0.9), Some(0.2));
        assert!(loose.passed(), "{loose}");
        let tight = check(Some(1.5), Some(0.05));
        assert!(!tight.passed(), "{tight}");
        let verdicts: Vec<(bool, &str)> = tight
            .expectations("out")
            .iter()
            .map(|expectation| {
                let kind = match expectation.region {
                    Region::Frequency { .. } => "frequency",
                    Region::Swing { .. } => "swing",
                    Region::Distortion { .. } => "distortion",
                    Region::Band { .. } => "band",
                };
                (expectation.passed, kind)
            })
            .collect();
        assert_eq!(
            verdicts,
            [(true, "frequency"), (false, "swing"), (false, "distortion")]
        );
        assert!(
            tight
                .message()
                .contains("harmonic distortion, above the 5% maximum"),
            "{tight}"
        );
        let Region::Swing { centre, .. } = tight.expectations("out")[1].region
        else {
            unreachable!("the second part is the swing")
        };
        assert!((centre - 2.0).abs() < 0.05, "{centre}");
    }

    /// Ngspice, recording each kick's current and the step it caused on `n`.
    struct Recording {
        kicks: std::cell::RefCell<Vec<(String, f64)>>,
        hertz: f64,
    }

    impl Backend for Recording {
        type Error = crate::NgspiceError;

        fn run_op(
            &self,
            netlist: &str,
            supplies: &[crate::DcSupply],
        ) -> Result<crate::OperatingPoint, Self::Error> {
            Ngspice::default().run_op(netlist, supplies)
        }

        fn run_tran(
            &self,
            netlist: &str,
            sources: &[TranSource],
            params: Tran,
        ) -> Result<crate::Transient, Self::Error> {
            let result =
                Ngspice::default().run_tran(netlist, sources, params)?;
            let kick = sources
                .last()
                .map(|kick| kick.spice_line("Ikt", params.step_seconds()))
                .unwrap_or_default();
            let step =
                kick_step(&result.node("n").expect("n present"), self.hertz);
            self.kicks.borrow_mut().push((kick, step));
            Ok(result)
        }

        fn run_ac(
            &self,
            netlist: &str,
            supplies: &[crate::AcSupply],
            params: crate::Ac,
        ) -> Result<crate::Spectra, Self::Error> {
            Ngspice::default().run_ac(netlist, supplies, params)
        }
    }

    #[test]
    fn a_kick_on_a_high_impedance_net_is_scaled_to_a_small_step() {
        // 1 mH with 10 pF rings at 1.59 MHz; 1 mA into it steps several volts.
        let hertz = 1.0 / (std::f64::consts::TAU * (1e-3f64 * 10e-12).sqrt());
        let backend = Recording {
            kicks: Default::default(),
            hertz,
        };
        let net = Net {
            name: "n".into(),
            nodes: Vec::new(),
        };
        oscillation(
            &backend,
            "* high impedance tank\nl1 n 0 1m\nc1 n 0 10p\nr1 n 0 1meg\n",
            Vec::new(),
            &net,
            (hertz, Tolerance::percent(5.0)),
            NO_LIMITS,
        )
        .expect("simulation ran");
        let kicks = backend.kicks.borrow();
        assert!(kicks[0].1 > MAX_KICK_VOLTS, "{kicks:?}");
        let (_, sized) = kicks[1];
        assert!(sized <= MAX_KICK_VOLTS && sized > 0.05, "{kicks:?}");
    }
}
