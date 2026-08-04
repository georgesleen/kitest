//! Core types and the simulation backend seam for kitest.

mod analysis;
mod backend;
mod ngspice;
mod results;
mod signal;

pub use analysis::{Ac, Sweep, Tran};
pub use backend::Backend;
pub use ngspice::{Ngspice, NgspiceError};
pub use results::{OperatingPoint, Spectra, Waveforms};
pub use signal::{Response, Signal, Tolerance};
