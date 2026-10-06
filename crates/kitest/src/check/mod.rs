//! Running every probe's expectation against a design and reporting each.

mod expect;

pub use expect::{ExpectError, Expectation, parse as parse_expectation};

use std::path::{Path, PathBuf};

use crate::kicad::node_name;
use crate::{
    Backend, Check, Config, ConfigError, Corner, Design, KicadError,
    ModelError, ModelLibrary, Net, NetlistError, Ngspice, Probe, ProbeError,
    SupplyError, Tran, export_design,
};

/// Cycles discarded while an oscillator starts, before measuring.
const SETTLE_CYCLES: f64 = 100.0;
/// Cycles measured once the oscillator has started.
const MEASURE_CYCLES: f64 = 50.0;
/// Output samples per expected cycle.
const SAMPLES_PER_CYCLE: f64 = 100.0;
/// Smallest amplitude, zero to peak, counted as oscillating.
const MIN_OSCILLATION_VOLTS: f64 = 1e-3;

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
        let node = node_name(&probe.net().name);
        let check = match *expectation {
            None => None,
            Some(Expectation::Dc { near, within }) => {
                let op =
                    op.as_ref().expect("an operating point ran for a dc check");
                Some(match op.node(&node) {
                    Some(voltage) => {
                        Check::near(voltage.volts(), near, within, "V")
                    }
                    None => missing(probe.net()),
                })
            }
            Some(Expectation::Oscillates { near, within }) => {
                let tran = Tran::new(
                    1.0 / (near * SAMPLES_PER_CYCLE),
                    (SETTLE_CYCLES + MEASURE_CYCLES) / near,
                )
                .start(SETTLE_CYCLES / near);
                let result =
                    backend.run_tran(netlist, &corner.tran_sources(), tran)?;
                Some(match result.node(&node) {
                    Some(signal) => {
                        oscillation(signal.dominant_tone(), near, within)
                    }
                    None => missing(probe.net()),
                })
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

/// Whether `tone` is an oscillation within `within` of `near` hertz.
fn oscillation(
    tone: crate::Tone,
    near: f64,
    within: crate::Tolerance,
) -> Check {
    let amplitude = tone.amplitude();
    if amplitude.volts() < MIN_OSCILLATION_VOLTS {
        return Check::new(
            false,
            format!(
                "no oscillation: the strongest tone is {amplitude} at {}, \
                 below the {} floor",
                tone.frequency(),
                crate::analysis::si(MIN_OSCILLATION_VOLTS, "V"),
            ),
        );
    }
    let frequency = Check::near(tone.frequency().hertz(), near, within, "Hz");
    Check::new(
        frequency.passed(),
        format!("{frequency}, at {amplitude} amplitude"),
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
