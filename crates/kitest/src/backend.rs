//! The simulation backend seam.

use crate::{
    Ac, DcSupply, OperatingPoint, Spectra, Tran, TranSource, Waveforms, stimulus::AcSupply,
};

/// A SPICE engine kitest can drive. Netlists carry no analysis directive of their
/// own; the analysis called supplies it and decides the result type.
pub trait Backend {
    type Error: std::error::Error + 'static;

    /// DC operating point.
    fn run_op(&self, netlist: &str, supplies: &[DcSupply]) -> Result<OperatingPoint, Self::Error>;

    /// Transient analysis.
    fn run_tran(
        &self,
        netlist: &str,
        sources: &[TranSource],
        params: Tran,
    ) -> Result<Waveforms, Self::Error>;

    /// Small-signal AC sweep.
    fn run_ac(
        &self,
        netlist: &str,
        supplies: &[AcSupply],
        params: Ac,
    ) -> Result<Spectra, Self::Error>;
}
