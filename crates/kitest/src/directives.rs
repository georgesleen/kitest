//! Analysis parameters a backend can run.

use crate::stimulus::Noise;

/// Default supply noise, in volts RMS.
///
/// Small enough to sit far below any assertion, large enough to start
/// an oscillator quickly. A real 9 V rail is noisier than this.
const DEFAULT_NOISE_VOLTS: f64 = 1e-3;

/// Transient analysis parameters, times in seconds.
///
/// Runs carry a little supply noise by default. SPICE is otherwise
/// perfectly silent, and a silent simulator is not a neutral one: an
/// oscillator's quiescent bias point is a valid solution, so whether
/// a noiseless transient ever leaves it comes down to solver rounding.
/// A high-gain oscillator often does start that way, which is worse
/// than not starting, because the result then depends on tolerances
/// rather than on the circuit. Real oscillators are started by their
/// own thermal noise, so modelling that makes startup a property of
/// the design. Use [`Tran::ideal`] for a silent run.
#[derive(Debug, Clone, Copy)]
pub struct Tran {
    step: f64,
    stop: f64,
    start: f64,
    noise: f64,
}

impl Tran {
    /// Run to `stop` seconds, printing every `step` seconds.
    pub fn new(step: f64, stop: f64) -> Self {
        Self {
            step,
            stop,
            start: 0.0,
            noise: DEFAULT_NOISE_VOLTS,
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

    /// Set the supply noise, in volts RMS.
    pub fn noise(self, volts: f64) -> Self {
        Self {
            noise: volts,
            ..self
        }
    }

    /// Simulate with no noise at all.
    ///
    /// Faster, at the cost of a simulator quieter than any real
    /// circuit. An oscillator may then fail to start, or may start on
    /// the solver's own rounding error, which is not a property of the
    /// circuit and shifts with tolerances and timestep.
    pub fn ideal(self) -> Self {
        Self { noise: 0.0, ..self }
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

    /// The noise to hang on each DC source, if any.
    ///
    /// The interval is the print step. ngspice forces a breakpoint at
    /// every noise sample, so an interval finer than the run's own
    /// resolution is pure cost: a 5 ms run with a 1 ns interval took
    /// 67 seconds against 33 ms with the interval at the print step.
    pub(crate) fn noise_spec(&self) -> Option<Noise> {
        (self.noise > 0.0).then_some(Noise {
            volts: self.noise,
            interval: self.step,
        })
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
