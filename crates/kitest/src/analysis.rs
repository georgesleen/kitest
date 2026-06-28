//! Type of SPICE analysis a backend runs.

#[derive(Debug, Clone)]
pub enum Analysis {
    /// DC operating point.
    Op,
    /// Transient analysis, times in seconds.
    Tran { step: f64, stop: f64 },
}
