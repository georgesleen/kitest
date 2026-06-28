//! Type of SPICE analysis a backend runs.

#[derive(Debug, Clone)]
pub enum Analysis {
    /// DC operating point.
    Op,
}
