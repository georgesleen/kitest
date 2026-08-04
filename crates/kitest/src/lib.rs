//! Core types and the simulation backend seam for kitest.

mod analysis;
mod backend;
mod kicad;
mod ngspice;
mod results;
mod signal;
mod stimulus;

pub use analysis::{Ac, Sweep, Tran};
pub use backend::Backend;
pub use kicad::{KicadError, export_netlist};
pub use ngspice::{Ngspice, NgspiceError};
pub use results::{OperatingPoint, Spectra, Waveforms};
pub use signal::{Response, Signal, Tolerance};
pub use stimulus::with_supplies;
