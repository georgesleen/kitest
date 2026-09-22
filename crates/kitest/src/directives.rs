//! Analysis parameters a backend can run.

/// Transient analysis parameters, times in seconds.
#[derive(Debug, Clone, Copy)]
pub struct Tran {
    step: f64,
    stop: f64,
    start: f64,
}

impl Tran {
    /// Run to `stop` seconds, printing every `step` seconds.
    pub fn new(step: f64, stop: f64) -> Self {
        Self {
            step,
            stop,
            start: 0.0,
        }
    }

    /// Discard output before `start` seconds.
    ///
    /// The simulation still runs from zero, so the circuit behaves the
    /// same and only the saved output is trimmed. Use this to keep an
    /// oscillator's startup out of a steady-state measurement.
    pub fn start(self, start: f64) -> Self {
        Self { start, ..self }
    }

    pub(crate) fn step_seconds(&self) -> f64 {
        self.step
    }

    pub(crate) fn stop_seconds(&self) -> f64 {
        self.stop
    }

    pub(crate) fn start_seconds(&self) -> f64 {
        self.start
    }
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
