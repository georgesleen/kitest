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
}

impl Tone {
    pub(crate) fn new(frequency: Frequency, amplitude: Voltage) -> Self {
        Self {
            frequency,
            amplitude,
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
}
