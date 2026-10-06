//! Core types and the simulation backend seam for kitest.

mod analysis;
mod check;
mod config;
mod directives;
mod kicad;
mod models;
mod sim;
mod stimulus;

pub use analysis::{
    Check, Frequency, OperatingPoint, Response, Signal, Spectra, Tolerance,
    Tone, Transient, Voltage,
};
pub use check::{
    CheckError, ExpectError, Expectation, Outcome, Report, Waveform,
    check_project, parse_expectation,
};
pub use config::{CONFIG_FILE, Config, ConfigError};
pub use directives::{Ac, Sweep, Tran};
pub use kicad::{
    Component, Corner, Design, KicadError, LibraryId, LibraryPart, LibraryPin,
    Net, Netlist, NetlistError, Node, PinKind, Power, Probe, ProbeError,
    ProbeProblem, Rail, RailKind, SupplyError, SupplyProblem, Transistor,
    VoltageOrigin, export_design, export_netlist,
};
pub use models::{ModelEntry, ModelError, ModelKind, ModelLibrary};
pub use sim::{Backend, Ngspice, NgspiceError};
pub use stimulus::{AcSupply, DcSupply, Pulse, Sin, TranSource};
