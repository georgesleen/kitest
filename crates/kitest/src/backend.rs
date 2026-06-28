use crate::{Analysis, Results};

/// A SPICE engine that runs a netlist and returns its results.
pub trait Backend {
    type Error: std::error::Error + 'static;

    /// Run `analysis` on `netlist`, raw SPICE text carrying no analysis directive
    /// of its own.
    fn run(&self, netlist: &str, analysis: Analysis) -> Result<Results, Self::Error>;
}
