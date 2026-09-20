//! The dominant sinusoid in a waveform, and its assertions.

use crate::{Frequency, Voltage};

/// The strongest sinusoid in a time-domain waveform.
///
/// Found by resampling the waveform onto a uniform grid, removing its DC
/// level, applying a Hann window, and taking the largest peak of the
/// discrete Fourier transform.
///
/// A waveform only holds a dominant sinusoid if it oscillates. A flat or
/// purely noisy node still yields a `Tone`, but its frequency is then an
/// artefact of rounding and means nothing. The amplitude is how to tell
/// the cases apart: it falls to near zero when there is nothing to find.
#[derive(Debug, Clone, Copy)]
pub struct Tone {
    frequency: Frequency,
    amplitude: Voltage,
    samples_per_cycle: f64,
}

impl Tone {
    pub(crate) fn new(
        frequency: Frequency,
        amplitude: Voltage,
        samples_per_cycle: f64,
    ) -> Self {
        Self {
            frequency,
            amplitude,
            samples_per_cycle,
        }
    }

    /// The frequency of the dominant sinusoid.
    ///
    /// Only meaningful for a waveform that oscillates. Assert on the
    /// amplitude as well to rule out a flat node, which reports a
    /// near-zero amplitude at an arbitrary frequency.
    pub fn frequency(&self) -> Frequency {
        self.frequency
    }

    /// The amplitude of the dominant sinusoid, zero to peak.
    ///
    /// This is the coefficient A in A sin(wt), so for a waveform with
    /// harmonics it measures the fundamental, not the swing of the whole
    /// waveform. Near zero means the waveform holds no sinusoid.
    pub fn amplitude(&self) -> Voltage {
        self.amplitude
    }

    /// Simulator samples per cycle of the dominant sinusoid.
    ///
    /// Averaged over the run, from the times the simulator chose. A
    /// waveform sampled too coarsely for its own oscillation aliases,
    /// and the reported frequency is then a plausible but wrong lower
    /// one, with nothing else to give it away. Roughly ten samples per
    /// cycle is the usual minimum to trust a measurement; below about
    /// two, Nyquist is violated outright.
    ///
    /// Assert on this when the frequency matters and the timestep is
    /// not obviously fine enough. Tighten a coarse run with the step
    /// on [`crate::Tran`].
    pub fn samples_per_cycle(&self) -> f64 {
        self.samples_per_cycle
    }
}
