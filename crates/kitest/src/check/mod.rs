//! Running every probe's expectation against a design and reporting each.

mod expect;

pub use expect::{ExpectError, Expectation, parse as parse_expectation};

use std::path::{Path, PathBuf};

use crate::analysis::si;
use crate::kicad::node_name;
use crate::{
    Backend, Check, Config, ConfigError, Corner, Design, KicadError,
    ModelError, ModelLibrary, Net, NetlistError, Ngspice, Probe, ProbeError,
    Signal, SupplyError, Tolerance, Tran, TranSource, export_design,
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
/// Current impulse into the probe's net that starts every oscillation run.
const KICK_AMPS: f64 = 1e-3;

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

        let mut outcomes = Vec::new();
        for corner in power.corners() {
            let corner_outcomes = check_corner(
                &netlist.text,
                &corner,
                &probes,
                &expectations,
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
    netlist: &str,
    corner: &Corner<'_>,
    probes: &[Probe<'_>],
    expectations: &[Option<Expectation>],
    backend: &B,
) -> Result<Vec<Outcome>, B::Error> {
    let voltages: Vec<(String, f64)> = corner
        .voltages()
        .map(|(node, volts)| (node.to_owned(), volts))
        .collect();
    let needs_op = expectations
        .iter()
        .any(|expectation| matches!(expectation, Some(Expectation::Dc { .. })));
    let op = if needs_op {
        Some(backend.run_op(netlist, &corner.dc_supplies())?)
    } else {
        None
    };

    let mut outcomes = Vec::new();
    for (probe, expectation) in probes.iter().zip(expectations) {
        let node = &probe.net().name;
        let check = match *expectation {
            None => None,
            Some(Expectation::Dc { near, within }) => {
                let op =
                    op.as_ref().expect("an operating point ran for a dc check");
                Some(match op.node(node) {
                    Some(voltage) => {
                        Check::near(voltage.volts(), near, within, "V")
                    }
                    None => missing(probe.net()),
                })
            }
            Some(Expectation::Oscillates { near, within }) => {
                Some(oscillation(
                    backend,
                    netlist,
                    corner.tran_sources(),
                    probe.net(),
                    near,
                    within,
                )?)
            }
        };
        outcomes.push(Outcome {
            probe: probe.name().to_owned(),
            reference: probe.reference().to_owned(),
            net: probe.net().name.clone(),
            corner: voltages.clone(),
            check,
        });
    }
    Ok(outcomes)
}

/// Whether `net` sustains an oscillation within `within` of `near` hertz.
///
/// The net is kicked at the start, so startup does not hang on numerical
/// noise, and the run lengthens until the amplitude is steady. A steady
/// amplitude is an oscillation; one that decays is a resonance the kick
/// rang. Decay slower than `STEADY_CHANGE` per window, a resonance with a
/// Q above about 8000 such as a crystal, still reads as steady.
fn oscillation<B: Backend>(
    backend: &B,
    netlist: &str,
    mut sources: Vec<TranSource>,
    net: &Net,
    near: f64,
    within: Tolerance,
) -> Result<Check, B::Error> {
    sources.push(TranSource::kick(&node_name(&net.name), KICK_AMPS));
    let mut last = None;
    for cycles in RUN_CYCLES {
        let tran = Tran::new(1.0 / (near * SAMPLES_PER_CYCLE), cycles / near);
        let result = backend.run_tran(netlist, &sources, tran)?;
        let Some(signal) = result.node(&net.name) else {
            return Ok(missing(net));
        };
        let envelope = Envelope::measure(&signal, near);
        if let Some(check) = envelope.verdict(cycles, near, within, &signal) {
            return Ok(check);
        }
        last = Some(envelope);
    }
    let envelope = last.expect("at least one run");
    let cycles = RUN_CYCLES[RUN_CYCLES.len() - 1];
    let amplitude = si(envelope.amplitude(), "V");
    let change = envelope.change() * 100.0;
    Ok(Check::new(
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
    ))
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
    fn verdict(
        &self,
        cycles: f64,
        near: f64,
        within: Tolerance,
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
            return Some(Check::new(
                false,
                format!(
                    "no oscillation: {} amplitude after {cycles} cycles, below \
                     the {} floor{decay}",
                    si(amplitude, "V"),
                    si(MIN_OSCILLATION_VOLTS, "V"),
                ),
            ));
        }
        if !self.steady() {
            return None;
        }
        let time = signal.time();
        let end = time.last().copied().unwrap_or(0.0);
        let start =
            time.partition_point(|t| *t < end - 2.0 * WINDOW_CYCLES / near);
        let tone = Signal::new(&time[start..], &signal.values()[start..])
            .dominant_tone();
        let frequency =
            Check::near(tone.frequency().hertz(), near, within, "Hz");
        Some(Check::new(
            frequency.passed(),
            format!(
                "{frequency}, at a steady {} amplitude",
                si(amplitude, "V")
            ),
        ))
    }
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
            near,
            Tolerance::percent(5.0),
        )
        .expect("simulation ran")
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
            1e6,
            Tolerance::percent(1.0),
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
}
