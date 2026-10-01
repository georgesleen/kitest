//! The dominant sinusoid in a waveform, and its assertions.

use crate::{Frequency, Voltage};

/// The strongest sinusoid in a time-domain waveform.
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
    /// Meaningful only for a waveform that oscillates. A flat node reports
    /// an arbitrary frequency at near-zero amplitude.
    pub fn frequency(&self) -> Frequency {
        self.frequency
    }

    /// The amplitude of the dominant sinusoid, zero to peak.
    ///
    /// This is the coefficient A in A sin(wt). A waveform with harmonics
    /// therefore measures its fundamental, not its whole swing.
    pub fn amplitude(&self) -> Voltage {
        self.amplitude
    }

    /// Simulator samples per cycle of the dominant sinusoid.
    pub fn samples_per_cycle(&self) -> f64 {
        self.samples_per_cycle
    }
}
