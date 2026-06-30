//! Type of SPICE analysis a backend runs.

/// Types of analysis to run on a circuit.
#[derive(Debug, Clone)]
pub enum Analysis {
    /// DC operating point.
    Op,
    /// Transient analysis, times in seconds.
    Tran { step: f64, stop: f64 },
    /// Small-signal AC sweep, frequencies in Hz.
    Ac {
        sweep: Sweep,
        points: u32,
        fstart: f64,
        fstop: f64,
    },
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
