//! Analysis parameters a backend can run.

/// Transient analysis parameters, times in seconds.
#[derive(Debug, Clone, Copy)]
pub struct Tran {
    pub step: f64,
    pub stop: f64,
}

/// Small-signal AC sweep parameters, frequencies in Hz.
#[derive(Debug, Clone, Copy)]
pub struct Ac {
    pub sweep: Sweep,
    pub points: u32,
    pub fstart: f64,
    pub fstop: f64,
}

/// Frequency axis spacing for an AC sweep.
#[derive(Debug, Clone, Copy)]
pub enum Sweep {
    /// Points per decade.
    Dec,
    /// Points per octave.
    Oct,
    /// Total points, linearly spaced.
    Lin,
}

impl Sweep {
    pub(crate) fn keyword(self) -> &'static str {
        match self {
            Self::Dec => "dec",
            Self::Oct => "oct",
            Self::Lin => "lin",
        }
    }
}
