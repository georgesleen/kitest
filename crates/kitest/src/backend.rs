//! The simulation backend seam.

use crate::{Ac, OperatingPoint, Spectra, Tran, Waveforms};

/// A SPICE engine kitest can drive. Netlists carry no analysis directive of their
/// own; the analysis called supplies it and decides the result type.
pub trait Backend {
    type Error: std::error::Error + 'static;

    /// DC operating point.
    fn run_op(&self, netlist: &str) -> Result<OperatingPoint, Self::Error>;

    /// Transient analysis.
    fn run_tran(&self, netlist: &str, params: Tran) -> Result<Waveforms, Self::Error>;

    /// Small-signal AC sweep.
    fn run_ac(&self, netlist: &str, params: Ac) -> Result<Spectra, Self::Error>;
}
