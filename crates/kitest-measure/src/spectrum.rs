//! Amplitude spectra of sampled time-domain curves.

use std::f64::consts::PI;
use std::ops::RangeInclusive;

use realfft::RealFftPlanner;

use crate::Curve;

/// A curve's amplitude spectrum over a time window.
pub struct Spectrum {
    bin_width: f64,
    raw: Vec<f64>,
    amplitudes: Vec<f64>,
}

/// The strongest sinusoid in a spectrum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tone {
    /// Frequency in hertz.
    pub hertz: f64,
    /// Amplitude zero to peak, in the curve's y unit.
    pub amplitude: f64,
}

impl Curve<'_> {
    /// The amplitude spectrum over `over`, resampled onto `samples` uniform
    /// points, with its mean removed and a periodic Hann window applied.
    pub fn spectrum(
        &self,
        over: RangeInclusive<f64>,
        samples: usize,
    ) -> Option<Spectrum> {
        if samples < 2 || self.x().len() < 2 || over.start() > over.end() {
            return None;
        }
        let start = (*over.start()).max(*self.x().first()?);
        let end = (*over.end()).min(*self.x().last()?);
        if start >= end {
            return None;
        }
        let dt = (end - start) / (samples - 1) as f64;
        let mut values: Vec<f64> = (0..samples)
            .map(|index| self.at(start + index as f64 * dt))
            .collect::<Option<_>>()?;
        let mean = values.iter().sum::<f64>() / samples as f64;
        for (index, value) in values.iter_mut().enumerate() {
            *value = (*value - mean) * hann(index, samples);
        }

        let mut planner = RealFftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(samples);
        let mut output = fft.make_output_vec();
        fft.process(&mut values, &mut output).ok()?;
        let raw: Vec<f64> = output.iter().map(|bin| bin.norm()).collect();
        let amplitudes = raw
            .iter()
            .enumerate()
            .map(|(bin, magnitude)| {
                let paired = bin != 0
                    && !(samples.is_multiple_of(2) && bin == samples / 2);
                let factor = if paired { 2.0 } else { 1.0 };
                factor * magnitude / (samples as f64 * coherent_gain())
            })
            .collect();
        Some(Spectrum {
            bin_width: 1.0 / (samples as f64 * dt),
            raw,
            amplitudes,
        })
    }
}

impl Spectrum {
    /// The frequency between adjacent bins, in hertz.
    pub fn bin_width(&self) -> f64 {
        self.bin_width
    }

    /// Each bin's amplitude, zero to peak, corrected for the window's coherent gain.
    pub fn amplitudes(&self) -> &[f64] {
        &self.amplitudes
    }

    /// The strongest sinusoid, with sub-bin frequency and lobe-corrected amplitude.
    pub fn dominant(&self) -> Option<Tone> {
        let (bin, &peak) = self
            .raw
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))?;
        (peak > f64::EPSILON).then(|| self.tone(bin))
    }

    /// The strongest sinusoid within a bin of `hertz`, or `None` when `hertz`
    /// lies beyond the spectrum.
    pub fn tone_near(&self, hertz: f64) -> Option<Tone> {
        let centre = (hertz / self.bin_width).round() as usize;
        let last = self.raw.len().checked_sub(1)?;
        if centre > last {
            return None;
        }
        let bin = (centre.saturating_sub(1)..=(centre + 1).min(last))
            .max_by(|&a, &b| self.raw[a].total_cmp(&self.raw[b]))?;
        Some(self.tone(bin))
    }

    /// The total harmonic distortion: the root sum square of harmonics 2 to
    /// `harmonics` over the fundamental, which is the dominant tone.
    ///
    /// Harmonics beyond the spectrum are left out. Returns `None` for a
    /// spectrum with no tone.
    pub fn total_harmonic_distortion(&self, harmonics: usize) -> Option<f64> {
        let fundamental = self.dominant()?;
        let power: f64 = (2..=harmonics)
            .map_while(|harmonic| {
                self.tone_near(fundamental.hertz * harmonic as f64)
            })
            .map(|tone| tone.amplitude * tone.amplitude)
            .sum();
        Some(power.sqrt() / fundamental.amplitude)
    }

    /// The tone at `bin`, refined between its neighbours.
    fn tone(&self, bin: usize) -> Tone {
        let offset = self.parabolic_offset(bin);
        Tone {
            hertz: (bin as f64 + offset) * self.bin_width,
            amplitude: self.amplitudes[bin] / hann_lobe(offset),
        }
    }

    /// The median amplitude of every non-DC bin.
    pub fn noise_floor(&self) -> f64 {
        let mut amplitudes =
            self.amplitudes.get(1..).unwrap_or_default().to_vec();
        if amplitudes.is_empty() {
            return 0.0;
        }
        amplitudes.sort_by(f64::total_cmp);
        let middle = amplitudes.len() / 2;
        if amplitudes.len().is_multiple_of(2) {
            (amplitudes[middle - 1] + amplitudes[middle]) / 2.0
        } else {
            amplitudes[middle]
        }
    }

    /// Sub-bin position of the peak at `bin`, from its two neighbours, within
    /// half a bin.
    fn parabolic_offset(&self, bin: usize) -> f64 {
        if bin == 0 || bin + 1 >= self.raw.len() {
            return 0.0;
        }
        let (left, peak, right) =
            (self.raw[bin - 1], self.raw[bin], self.raw[bin + 1]);
        if left <= 0.0 || peak <= 0.0 || right <= 0.0 {
            return 0.0;
        }
        let (a, b, g) = (left.log10(), peak.log10(), right.log10());
        let curvature = a - 2.0 * b + g;
        if curvature >= 0.0 {
            0.0
        } else {
            (0.5 * (a - g) / curvature).clamp(-0.5, 0.5)
        }
    }
}

/// Periodic Hann taper at sample `index` of `samples`.
fn hann(index: usize, samples: usize) -> f64 {
    0.5 * (1.0 - (2.0 * PI * index as f64 / samples as f64).cos())
}

/// Mean of the periodic Hann window.
fn coherent_gain() -> f64 {
    0.5
}

/// Hann main lobe relative to its peak, `offset` bins from a bin centre.
fn hann_lobe(offset: f64) -> f64 {
    if offset == 0.0 {
        1.0
    } else {
        let x = PI * offset;
        x.sin() / (x * (1.0 - offset * offset))
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use crate::Curve;

    #[test]
    fn nyquist_amplitude_is_not_doubled() {
        let samples = 64;
        let time: Vec<f64> = (0..samples).map(|index| index as f64).collect();
        let values: Vec<f64> = (0..samples)
            .map(|index| if index % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let spectrum = Curve::new(&time, &values)
            .spectrum(0.0..=time[samples - 1], samples)
            .unwrap();
        let amplitude = spectrum.amplitudes()[samples / 2];
        let dbv = 20.0 * amplitude.log10();
        assert!((amplitude - 1.0).abs() < 1e-12, "{amplitude} V, {dbv} dBV");
        assert!(dbv.abs() < 1e-12, "{dbv} dBV");
        let tone = spectrum.dominant().unwrap();
        assert_eq!(tone.hertz, 0.5);
        assert!((tone.amplitude - 1.0).abs() < 1e-12);
        assert!(
            (spectrum.tone_near(0.5).unwrap().amplitude - 1.0).abs() < 1e-12
        );
    }

    #[test]
    fn dc_amplitude_is_not_doubled() {
        let samples = 64;
        let time: Vec<f64> = (0..samples).map(|index| index as f64).collect();
        let values: Vec<f64> = time
            .iter()
            .map(|time| (2.0 * PI * time / samples as f64).cos())
            .collect();
        let spectrum = Curve::new(&time, &values)
            .spectrum(0.0..=time[samples - 1], samples)
            .unwrap();
        // The Hann taper leaves a DC component of magnitude one half.
        assert!((spectrum.amplitudes()[0] - 0.5).abs() < 1e-12);
        assert!((spectrum.tone(0).amplitude - 0.5).abs() < 1e-12);
    }

    #[test]
    fn paired_bins_are_doubled_in_even_and_odd_transforms() {
        for samples in [64, 65] {
            let time: Vec<f64> =
                (0..samples).map(|index| index as f64).collect();
            for bin in [8, (samples - 1) / 2] {
                let values: Vec<f64> = time
                    .iter()
                    .map(|time| {
                        (2.0 * PI * bin as f64 * time / samples as f64).sin()
                    })
                    .collect();
                let spectrum = Curve::new(&time, &values)
                    .spectrum(0.0..=time[samples - 1], samples)
                    .unwrap();
                // At the odd last bin, the Hann lobes of the sine's pair overlap.
                let expected = if samples % 2 == 1 && bin == samples / 2 {
                    1.5
                } else {
                    1.0
                };
                assert!(
                    (spectrum.amplitudes()[bin] - expected).abs() < 1e-12,
                    "{samples} samples, bin {bin}: {}",
                    spectrum.amplitudes()[bin]
                );
                if samples % 2 == 1 && bin == samples / 2 {
                    assert!(
                        (spectrum.tone(bin).amplitude - expected).abs() < 1e-12
                    );
                }
            }
        }
    }

    #[test]
    fn distortion_uses_the_undoubled_nyquist_harmonic() {
        let samples = 64;
        let time: Vec<f64> = (0..samples).map(|index| index as f64).collect();
        let values: Vec<f64> = time
            .iter()
            .map(|time| {
                (2.0 * PI * 0.25 * time).cos()
                    + 0.1 * (2.0 * PI * 0.5 * time).cos()
            })
            .collect();
        let spectrum = Curve::new(&time, &values)
            .spectrum(0.0..=time[samples - 1], samples)
            .unwrap();
        assert!((spectrum.dominant().unwrap().amplitude - 1.0).abs() < 1e-12);
        assert!(
            (spectrum.tone_near(0.5).unwrap().amplitude - 0.1).abs() < 1e-12
        );
        assert!(
            (spectrum.total_harmonic_distortion(2).unwrap() - 0.1).abs()
                < 1e-12
        );
    }

    #[test]
    fn a_tone_between_bins_keeps_its_frequency_and_amplitude() {
        let samples = 1024;
        let time: Vec<f64> =
            (0..samples).map(|index| index as f64 / 10_000.0).collect();
        let values: Vec<f64> = time
            .iter()
            .map(|time| 2.5 * (2.0 * PI * 997.0 * time).sin())
            .collect();
        let spectrum = Curve::new(&time, &values)
            .spectrum(time[0]..=time[samples - 1], samples)
            .unwrap();
        let tone = spectrum.dominant().unwrap();
        assert!((tone.hertz - 997.0).abs() < 1.0, "{} Hz", tone.hertz);
        assert!((tone.amplitude - 2.5).abs() < 0.03, "{} V", tone.amplitude);
    }

    #[test]
    fn noise_floor_is_the_median_non_dc_bin() {
        let time = [0.0, 1.0, 2.0, 3.0];
        let zero = [0.0; 4];
        let spectrum = Curve::new(&time, &zero).spectrum(0.0..=3.0, 4).unwrap();
        assert_eq!(spectrum.noise_floor(), 0.0);
        assert_eq!(spectrum.dominant(), None);
    }

    #[test]
    fn spectrum_uses_only_the_requested_window() {
        let time: Vec<f64> =
            (0..2001).map(|index| index as f64 / 10_000.0).collect();
        let values: Vec<f64> = time
            .iter()
            .map(|&time| {
                if time < 0.1 {
                    (2.0 * PI * 500.0 * time).sin()
                } else {
                    (2.0 * PI * 1500.0 * time).sin()
                }
            })
            .collect();
        let spectrum = Curve::new(&time, &values)
            .spectrum(0.1..=0.2, 1024)
            .unwrap();
        assert!((spectrum.dominant().unwrap().hertz - 1500.0).abs() < 2.0);
    }

    fn spectrum_of(harmonics: &[(f64, f64)]) -> super::Spectrum {
        let samples = 4096;
        let time: Vec<f64> =
            (0..samples).map(|index| index as f64 / 100_000.0).collect();
        let values: Vec<f64> = time
            .iter()
            .map(|time| {
                harmonics
                    .iter()
                    .map(|(order, amplitude)| {
                        amplitude * (2.0 * PI * 997.0 * order * time).sin()
                    })
                    .sum()
            })
            .collect();
        Curve::new(&time, &values)
            .spectrum(time[0]..=time[samples - 1], samples)
            .unwrap()
    }

    #[test]
    fn distortion_is_the_root_sum_square_of_the_harmonics_over_the_fundamental()
    {
        let thd = spectrum_of(&[(1.0, 2.0), (2.0, 0.2), (3.0, 0.1)])
            .total_harmonic_distortion(10)
            .unwrap();
        let expected = (0.1f64.powi(2) + 0.05f64.powi(2)).sqrt();
        assert!((thd - expected).abs() / expected < 0.02, "{thd}");
    }

    #[test]
    fn a_square_wave_counts_only_the_harmonics_asked_for() {
        let odd: Vec<(f64, f64)> = [1.0, 3.0, 5.0, 7.0, 9.0, 11.0]
            .iter()
            .map(|&k| (k, 1.0 / k))
            .collect();
        let spectrum = spectrum_of(&odd);
        let nine = spectrum.total_harmonic_distortion(9).unwrap();
        let expected = [3.0, 5.0, 7.0, 9.0]
            .iter()
            .map(|k: &f64| 1.0 / (k * k))
            .sum::<f64>()
            .sqrt();
        assert!((nine - expected).abs() / expected < 0.02, "{nine}");
    }

    #[test]
    fn a_pure_sine_has_no_distortion() {
        let thd = spectrum_of(&[(1.0, 1.0)])
            .total_harmonic_distortion(10)
            .unwrap();
        assert!(thd < 1e-4, "{thd}");
    }
}
