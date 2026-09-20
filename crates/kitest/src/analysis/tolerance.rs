//! Closeness tolerance shared by the result assertions.

/// How close a value must be to a target.
#[derive(Debug, Clone, Copy)]
pub enum Tolerance {
    /// Absolute distance from the target.
    Abs(f64),
    /// Percentage of the target.
    Percent(f64),
}

impl Tolerance {
    /// An absolute tolerance of `v`.
    pub fn abs(v: f64) -> Self {
        Self::Abs(v)
    }

    /// A tolerance of `p` percent of the target.
    pub fn percent(p: f64) -> Self {
        Self::Percent(p)
    }

    /// The allowed distance from `target`: absolute as-is, percent of `|target|`.
    pub(crate) fn band(self, target: f64) -> f64 {
        match self {
            Self::Abs(v) => v,
            Self::Percent(p) => p / 100.0 * target.abs(),
        }
    }
}
