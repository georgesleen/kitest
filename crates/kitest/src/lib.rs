//! Core types and the simulation backend seam for kitest.

mod analysis;
mod backend;
mod ngspice;
mod results;
mod signal;

pub use analysis::Analysis;
pub use backend::Backend;
pub use ngspice::{Ngspice, NgspiceError};
pub use results::Results;
pub use signal::{Signal, Tolerance};
