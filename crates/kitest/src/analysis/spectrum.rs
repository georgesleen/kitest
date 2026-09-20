//! Frequency-domain measurement of a time-domain signal.

use std::f64::consts::PI;

use realfft::RealFftPlanner;

use crate::{Frequency, Signal, Tone, Voltage};

/// A window function, with the corrections its shape forces on a
/// measurement.
///
/// A window is three coupled things: the taper itself, the mean it
/// leaves behind, and the shape of its main lobe. All three must agree,
/// so a new variant cannot be added without filling in every match.
#[derive(Debug, Clone, Copy)]
enum Window {
    Hann,
}

impl Window {
    /// Taper at sample `k` of `n`.
    fn taper(self, k: usize, n: usize) -> f64 {
        let turns = k as f64 / n as f64;
        match self {
            // Periodic, so an on-bin tone leaks into two bins only.
            Self::Hann => 0.5 * (1.0 - (2.0 * PI * turns).cos()),
        }
    }

    /// Mean of the window. Amplitude recovery divides by this.
    fn coherent_gain(self) -> f64 {
        match self {
            Self::Hann => 0.5,
        }
    }

    /// Main lobe relative to its peak, `offset` bins from a bin centre.
    ///
    /// A tone between bins sits on the flank of its own lobe, so the
    /// peak magnitude under-reads by this factor. Hann loses up to 15%
    /// at half a bin.
    fn lobe(self, offset: f64) -> f64 {
        if offset == 0.0 {
            return 1.0;
        }
        match self {
            Self::Hann => {
                let x = PI * offset;
                x.sin() / (x * (1.0 - offset * offset))
            }
        }
    }
}

/// One point on a waveform.
#[derive(Debug, Clone, Copy)]
struct Point {
    time: f64,
    value: f64,
}

impl Point {
    /// The point at index `i`.
    fn at(time: &[f64], values: &[f64], i: usize) -> Self {
        Self {
            time: time[i],
            value: values[i],
        }
    }
}

/// Samples at a constant time step, as the transform requires.
struct Waveform {
    dt: f64,
    values: Vec<f64>,
}

impl Waveform {
    /// Resample a signal onto a uniform time grid.
    ///
    /// The grid has `n` points and spans the whole time axis, so the
    /// first output value is the first input value exactly. The time
    /// axis must be strictly increasing.
    ///
    /// # Panics
    /// If the two axes differ in length, if the signal has fewer than
    /// two samples, if the time axis is not strictly increasing, or if
    /// `n` is less than two.
    fn resample(signal: &Signal, n: usize) -> Self {
        let time = signal.time();
        let values = signal.values();
        assert!(
            time.len() == values.len(),
            "time and values must have the same length"
        );
        assert!(time.len() >= 2, "resample needs at least two samples");
        assert!(n >= 2, "resample needs at least two output points");
        assert!(
            time.windows(2).all(|pair| pair[0] < pair[1]),
            "time must be strictly increasing"
        );

        let time_initial = time[0];
        let time_final = time[time.len() - 1];
        let dt = (time_final - time_initial) / (n - 1) as f64;

        let mut out = Vec::with_capacity(n);

        let mut bracket = 0;
        for i in 0..n {
            let t = time_initial + i as f64 * dt;
            // Hold time[bracket] <= t <= time[bracket + 1].
            while bracket + 2 < time.len() && time[bracket + 1] < t {
                bracket += 1;
            }

            let before = Point::at(time, values, bracket);
            let after = Point::at(time, values, bracket + 1);
            out.push(linear_interpolate(before, after, t));
        }

        Self { dt, values: out }
    }

    /// Subtract the mean, so a DC bias doesn't dominate the spectrum.
    fn remove_mean(mut self) -> Self {
        let sum: f64 = self.values.iter().sum();
        let mean = sum / self.values.len() as f64;

        for value in &mut self.values {
            *value -= mean;
        }
        self
    }

    /// Window the samples and transform them to magnitudes.
    ///
    /// The window is applied here rather than in its own step so that
    /// the spectrum always knows which one produced it, and cannot be
    /// corrected with the wrong one.
    fn spectrum(mut self, window: Window) -> Spectrum {
        let samples = self.values.len();
        for (k, value) in self.values.iter_mut().enumerate() {
            *value *= window.taper(k, samples);
        }

        let mut planner = RealFftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(samples);
        let mut output = fft.make_output_vec();
        fft.process(&mut self.values, &mut output)
            .expect("buffer sized by the plan");

        Spectrum {
            bin_width: 1.0 / (samples as f64 * self.dt),
            samples,
            window,
            magnitudes: output.iter().map(|bin| bin.norm()).collect(),
        }
    }
}

/// Magnitudes at a constant frequency step.
struct Spectrum {
    bin_width: f64,
    samples: usize,
    window: Window,
    magnitudes: Vec<f64>,
}

impl Spectrum {
    /// Bin index of the largest magnitude.
    fn peak_bin(&self) -> usize {
        self.magnitudes
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(bin, _)| bin)
            .expect("a spectrum always has at least one bin")
    }

    /// Sub-bin position of the peak at `bin`, from its two neighbours.
    fn parabolic_offset(&self, bin: usize) -> f64 {
        if bin == 0 || bin + 1 >= self.magnitudes.len() {
            return 0.0;
        }
        let left = self.magnitudes[bin - 1];
        let peak = self.magnitudes[bin];
        let right = self.magnitudes[bin + 1];
        if left <= 0.0 || peak <= 0.0 || right <= 0.0 {
            return 0.0;
        }

        // Fit on the log magnitude, where the Hann lobe is closer to a
        // parabola. The factor of 20 in dB cancels in the ratio.
        let a = left.log10();
        let b = peak.log10();
        let g = right.log10();

        let curvature = a - 2.0 * b + g;
        if curvature == 0.0 {
            return 0.0;
        }
        0.5 * (a - g) / curvature
    }

    /// Frequency of the peak at `bin`, `offset` bins from its centre.
    fn frequency(&self, bin: usize, offset: f64) -> f64 {
        (bin as f64 + offset) * self.bin_width
    }

    /// Amplitude of the peak at `bin`, zero to peak.
    ///
    /// Undoes three things: the transform's length, the window's mean,
    /// and the lobe flank the tone sits on when it falls between bins.
    /// The factor of two is the negative half of the real spectrum.
    fn amplitude(&self, bin: usize, offset: f64) -> f64 {
        let gain = self.window.coherent_gain() * self.window.lobe(offset);
        2.0 * self.magnitudes[bin] / (self.samples as f64 * gain)
    }
}

/// Interpolate a value on the line through `a` and `b` at `time`.
fn linear_interpolate(a: Point, b: Point, time: f64) -> f64 {
    let slope = (b.value - a.value) / (b.time - a.time);
    a.value + slope * (time - a.time)
}

/// Find the dominant sinusoid in an arbitrary waveform.
pub(crate) fn dominant_tone(signal: &Signal) -> Tone {
    // Powers of two are the fastest length for the transform.
    let n = signal.time().len().next_power_of_two();
    let spectrum = Waveform::resample(signal, n)
        .remove_mean()
        .spectrum(Window::Hann);

    let bin = spectrum.peak_bin();
    let offset = spectrum.parabolic_offset(bin);
    let hertz = spectrum.frequency(bin, offset);

    Tone::new(
        Frequency::new(hertz),
        Voltage::new(spectrum.amplitude(bin, offset)),
        // The simulator's own rate, not the resampled one: interpolation
        // cannot recover a cycle the simulator never sampled.
        sample_rate(signal) / hertz,
    )
}

/// Mean samples per second over the times the simulator chose.
fn sample_rate(signal: &Signal) -> f64 {
    let time = signal.time();
    let span = time[time.len() - 1] - time[0];
    (time.len() - 1) as f64 / span
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use rand_distr::{Distribution, Normal};
    use std::f64::consts::PI;

    const HANN: Window = Window::Hann;

    /// v = 10 t, sampled unevenly. A line interpolates exactly.
    const RAMP_TIME: [f64; 5] = [0.0, 0.1, 0.3, 0.4, 0.5];
    const RAMP_VALUES: [f64; 5] = [0.0, 1.0, 3.0, 4.0, 5.0];

    /// Tight cluster then one long stride, as ngspice emits at a fast edge.
    const CLUSTER_TIME: [f64; 6] = [0.0, 0.001, 0.002, 0.003, 0.004, 0.5];
    const CLUSTER_VALUES: [f64; 6] = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0];

    const EPS: f64 = 1e-12;

    /// Steps cycle through this table, so the sampling is uneven but fixed.
    const JITTER: [f64; 4] = [1.0e-6, 0.5e-6, 1.7e-6, 0.3e-6];
    const SINE_HZ: f64 = 1000.0;
    const SINE_SPAN: f64 = 5e-3;

    /// A 1 kHz sine sampled at unevenly spaced times.
    fn uneven_sine() -> (Vec<f64>, Vec<f64>) {
        let mut time = Vec::new();
        let mut t = 0.0;
        let mut step = 0;
        while t < SINE_SPAN {
            time.push(t);
            t += JITTER[step % JITTER.len()];
            step += 1;
        }
        time.push(SINE_SPAN);
        let values = time
            .iter()
            .map(|t| (2.0 * PI * SINE_HZ * t).sin())
            .collect();
        (time, values)
    }

    /// The ramp signal, v = 10 t.
    fn ramp() -> Signal<'static> {
        Signal::new(&RAMP_TIME, &RAMP_VALUES)
    }

    /// The clustered signal.
    fn cluster() -> Signal<'static> {
        Signal::new(&CLUSTER_TIME, &CLUSTER_VALUES)
    }

    /// A waveform whose step does not matter to the test.
    fn wave(values: Vec<f64>) -> Waveform {
        Waveform { dt: 1.0, values }
    }

    /// A spectrum whose provenance does not matter to the test.
    fn spectrum(magnitudes: Vec<f64>) -> Spectrum {
        Spectrum {
            bin_width: 1.0,
            samples: magnitudes.len(),
            window: HANN,
            magnitudes,
        }
    }

    #[test]
    fn returns_the_requested_number_of_points() {
        let out = Waveform::resample(&ramp(), 11).values;
        assert_eq!(out.len(), 11);
    }

    #[test]
    fn step_divides_the_span_into_n_minus_one_gaps() {
        let dt = Waveform::resample(&ramp(), 11).dt;
        assert_eq!(dt, 0.5 / 10.0);
    }

    #[test]
    fn output_count_is_independent_of_input_count() {
        let resampled = Waveform::resample(&cluster(), 10);
        assert_eq!(resampled.values.len(), 10);
        assert_eq!(resampled.dt, 0.5 / 9.0);
    }

    #[test]
    fn preserves_the_first_sample_exactly() {
        let out = Waveform::resample(&ramp(), 11).values;
        assert_eq!(out[0], RAMP_VALUES[0]);
    }

    #[test]
    fn lands_on_the_final_sample() {
        let out = Waveform::resample(&ramp(), 11).values;
        let last = out[out.len() - 1];
        assert!((last - 5.0).abs() < EPS, "last = {last}");
    }

    #[test]
    fn reproduces_a_straight_line() {
        let resampled = Waveform::resample(&ramp(), 11);
        for (i, got) in resampled.values.iter().enumerate() {
            let want = 10.0 * (i as f64 * resampled.dt);
            assert!((got - want).abs() < EPS, "i = {i}, got {got}");
        }
    }

    #[test]
    fn advances_over_clustered_samples() {
        let resampled = Waveform::resample(&cluster(), 10);
        assert_eq!(resampled.values[0], 0.0);
        // Every later point falls in the final bracket, a 0 to 1 ramp.
        for (i, got) in resampled.values.iter().enumerate().skip(1) {
            let t = i as f64 * resampled.dt;
            let want = (t - 0.004) / (0.5 - 0.004);
            assert!((got - want).abs() < EPS, "i = {i}, got {got}");
        }
    }

    #[test]
    fn matches_a_sine_sampled_unevenly() {
        let (time, values) = uneven_sine();
        let signal = Signal::new(&time, &values);
        let resampled = Waveform::resample(&signal, 8192);
        for (i, got) in resampled.values.iter().enumerate() {
            let t = i as f64 * resampled.dt;
            let want = (2.0 * PI * SINE_HZ * t).sin();
            assert!((got - want).abs() < 1e-4, "i = {i}, got {got}");
        }
    }

    #[test]
    #[should_panic(expected = "same length")]
    fn rejects_mismatched_lengths() {
        Waveform::resample(&Signal::new(&RAMP_TIME, &RAMP_VALUES[..3]), 8);
    }

    #[test]
    #[should_panic(expected = "at least two samples")]
    fn rejects_a_single_sample() {
        Waveform::resample(&Signal::new(&[0.0], &[1.0]), 8);
    }

    #[test]
    #[should_panic(expected = "at least two output points")]
    fn rejects_fewer_than_two_output_points() {
        Waveform::resample(&ramp(), 1);
    }

    /// The taper the implementation must produce.
    fn hann_reference(n: usize) -> Vec<f64> {
        (0..n)
            .map(|k| 0.5 * (1.0 - (2.0 * PI * k as f64 / n as f64).cos()))
            .collect()
    }

    #[test]
    fn hann_taper_matches_the_periodic_definition() {
        let n = 8;
        let want = hann_reference(n);
        for (k, expected) in want.iter().enumerate() {
            let got = HANN.taper(k, n);
            assert!((got - expected).abs() < EPS, "k = {k}, got {got}");
        }
    }

    #[test]
    fn hann_taper_starts_at_zero() {
        assert_eq!(HANN.taper(0, 8), 0.0);
    }

    #[test]
    fn hann_taper_peaks_at_one_in_the_middle() {
        assert_eq!(HANN.taper(4, 8), 1.0);
    }

    #[test]
    fn hann_taper_is_symmetric_about_the_centre() {
        let n = 64;
        for k in 1..n {
            let got = HANN.taper(k, n);
            let mirrored = HANN.taper(n - k, n);
            assert!((got - mirrored).abs() < 1e-15, "k = {k}, got {got}");
        }
    }

    #[test]
    fn hann_taper_mean_is_the_coherent_gain() {
        let n = 1024;
        let sum: f64 = (0..n).map(|k| HANN.taper(k, n)).sum();
        let mean = sum / n as f64;
        let want = HANN.coherent_gain();
        assert!((mean - want).abs() < EPS, "mean = {mean}");
    }

    #[test]
    fn hann_lobe_is_one_at_a_bin_centre() {
        assert_eq!(HANN.lobe(0.0), 1.0);
    }

    #[test]
    fn hann_lobe_loses_fifteen_percent_at_half_a_bin() {
        let got = HANN.lobe(0.5);
        assert!((got - 0.848_826).abs() < 1e-6, "lobe = {got}");
    }

    #[test]
    fn hann_lobe_is_symmetric() {
        for step in 1..=5 {
            let offset = step as f64 / 10.0;
            let difference = (HANN.lobe(offset) - HANN.lobe(-offset)).abs();
            assert!(difference < EPS, "offset = {offset}");
        }
    }

    const FFT_N: usize = 1024;
    const TONE_BIN: usize = 37;
    const TONE_AMPLITUDE: f64 = 2.0;
    const DC_BIAS: f64 = 5.0;

    /// A `TONE_AMPLITUDE` sine of `bin` cycles per buffer, on `bias`.
    fn tone_on(bias: f64, bin: f64) -> Waveform {
        wave(
            (0..FFT_N)
                .map(|k| {
                    let turns = bin * k as f64 / FFT_N as f64;
                    bias + TONE_AMPLITUDE * (2.0 * PI * turns).sin()
                })
                .collect(),
        )
    }

    /// The spectrum of a sine of `bin` cycles per buffer.
    fn tone_spectrum_at(bin: f64) -> Spectrum {
        tone_on(0.0, bin).spectrum(HANN)
    }

    /// The spectrum of a sine sitting exactly on `TONE_BIN`.
    fn tone_spectrum() -> Spectrum {
        tone_spectrum_at(TONE_BIN as f64)
    }

    #[test]
    fn magnitudes_cover_half_the_spectrum_plus_one() {
        assert_eq!(tone_spectrum().magnitudes.len(), FFT_N / 2 + 1);
    }

    #[test]
    fn bin_width_is_one_over_the_record_length() {
        let resampled = Waveform::resample(&ramp(), 16);
        let dt = resampled.dt;
        let spectrum = resampled.spectrum(HANN);
        assert_eq!(spectrum.bin_width, 1.0 / (16.0 * dt));
    }

    #[test]
    fn magnitudes_peak_at_the_tone_bin() {
        assert_eq!(tone_spectrum().peak_bin(), TONE_BIN);
    }

    #[test]
    fn an_on_bin_tone_leaks_into_two_neighbours_only() {
        let mags = tone_spectrum().magnitudes;
        let peak = mags[TONE_BIN];
        let below = mags[TONE_BIN - 1] / peak;
        let above = mags[TONE_BIN + 1] / peak;
        assert!((below - 0.5).abs() < 1e-9, "below = {below}");
        assert!((above - 0.5).abs() < 1e-9, "above = {above}");
        for (bin, magnitude) in mags.iter().enumerate() {
            if bin + 1 < TONE_BIN || bin > TONE_BIN + 1 {
                assert!(*magnitude < 1e-9, "bin = {bin}, got {magnitude}");
            }
        }
    }

    #[test]
    fn a_constant_signal_lands_in_bin_zero() {
        let spectrum = wave(vec![1.0; FFT_N]).spectrum(HANN);
        let want = FFT_N as f64 * HANN.coherent_gain();
        let got = spectrum.magnitudes[0];
        assert!((got - want).abs() < 1e-9, "bin 0 = {got}");
    }

    #[test]
    fn amplitude_recovers_an_on_bin_tone() {
        let spectrum = tone_spectrum();
        let got = spectrum.amplitude(TONE_BIN, 0.0);
        assert!((got - TONE_AMPLITUDE).abs() < 1e-9, "amplitude = {got}");
    }

    #[test]
    fn amplitude_recovers_a_tone_between_bins() {
        // Without the lobe correction this reads 15% low at half a bin.
        for step in 0..=10 {
            let wanted = TONE_BIN as f64 + step as f64 / 20.0;
            let spectrum = tone_spectrum_at(wanted);
            let bin = spectrum.peak_bin();
            let offset = spectrum.parabolic_offset(bin);
            let got = spectrum.amplitude(bin, offset);
            let error = (got - TONE_AMPLITUDE).abs() / TONE_AMPLITUDE;
            assert!(error < 0.01, "at {wanted}, amplitude = {got}");
        }
    }

    #[test]
    fn remove_mean_centres_the_samples() {
        let centred = wave(vec![1.0, 2.0, 3.0, 4.0]).remove_mean();
        assert_eq!(centred.values, vec![-1.5, -0.5, 0.5, 1.5]);
    }

    #[test]
    fn remove_mean_leaves_the_tone_amplitude_alone() {
        let biased = tone_on(DC_BIAS, TONE_BIN as f64)
            .remove_mean()
            .spectrum(HANN);
        let plain = tone_spectrum();
        let difference =
            (biased.magnitudes[TONE_BIN] - plain.magnitudes[TONE_BIN]).abs();
        assert!(difference < 1e-9, "difference = {difference}");
    }

    #[test]
    fn a_dc_bias_dominates_a_spectrum_that_keeps_it() {
        // Why remove_mean exists: Hann puts half of bin 0 into bin 1,
        // so skipping bin 0 alone would not save the measurement.
        let spectrum = tone_on(DC_BIAS, TONE_BIN as f64).spectrum(HANN);
        assert_eq!(spectrum.peak_bin(), 0);
        let leaked = spectrum.magnitudes[1];
        assert!(leaked > spectrum.magnitudes[TONE_BIN], "bin 1 = {leaked}");
    }

    #[test]
    fn removing_the_mean_restores_the_tone_as_the_peak() {
        let spectrum = tone_on(DC_BIAS, TONE_BIN as f64)
            .remove_mean()
            .spectrum(HANN);
        assert_eq!(spectrum.peak_bin(), TONE_BIN);
    }

    #[test]
    fn peak_bin_finds_the_largest_magnitude() {
        assert_eq!(spectrum(vec![0.1, 0.4, 9.0, 0.2]).peak_bin(), 2);
    }

    #[test]
    fn parabolic_offset_is_zero_on_bin() {
        let offset = tone_spectrum().parabolic_offset(TONE_BIN);
        assert!(offset.abs() < 1e-9, "offset = {offset}");
    }

    #[test]
    fn parabolic_offset_recovers_a_fractional_bin() {
        let wanted = TONE_BIN as f64 + 0.35;
        let spectrum = tone_spectrum_at(wanted);
        let bin = spectrum.peak_bin();
        let got = bin as f64 + spectrum.parabolic_offset(bin);
        assert!((got - wanted).abs() < 0.02, "got {got}");
    }

    #[test]
    fn parabolic_offset_stays_accurate_across_a_bin() {
        for step in 0..=20 {
            let wanted = TONE_BIN as f64 - 0.5 + step as f64 / 20.0;
            let spectrum = tone_spectrum_at(wanted);
            let bin = spectrum.peak_bin();
            let got = bin as f64 + spectrum.parabolic_offset(bin);
            assert!((got - wanted).abs() < 0.02, "wanted {wanted}, got {got}");
        }
    }

    #[test]
    fn parabolic_offset_is_zero_without_two_neighbours() {
        let spectrum = spectrum(vec![1.0, 2.0, 3.0]);
        assert_eq!(spectrum.parabolic_offset(0), 0.0);
        assert_eq!(spectrum.parabolic_offset(2), 0.0);
    }

    #[test]
    fn parabolic_offset_is_zero_on_a_flat_spectrum() {
        assert_eq!(spectrum(vec![1.0; 4]).parabolic_offset(1), 0.0);
    }

    #[test]
    fn parabolic_offset_is_zero_on_an_empty_spectrum() {
        assert_eq!(spectrum(vec![0.0; 3]).parabolic_offset(1), 0.0);
    }

    const MEASURED_HZ: f64 = 1234.0;
    const MEASURED_SPAN: f64 = 20e-3;

    /// A biased sine at `MEASURED_HZ`, sampled as ngspice would.
    fn measured_signal() -> (Vec<f64>, Vec<f64>) {
        let mut time = Vec::new();
        let mut t = 0.0;
        let mut step = 0;
        while t < MEASURED_SPAN {
            time.push(t);
            t += JITTER[step % JITTER.len()];
            step += 1;
        }
        time.push(MEASURED_SPAN);
        let values = time
            .iter()
            .map(|t| {
                let turns = MEASURED_HZ * t;
                DC_BIAS + TONE_AMPLITUDE * (2.0 * PI * turns).sin()
            })
            .collect();
        (time, values)
    }

    #[test]
    fn dominant_tone_measures_an_unevenly_sampled_sine() {
        let (time, values) = measured_signal();
        let tone = dominant_tone(&Signal::new(&time, &values));

        let hertz = tone.frequency().hertz();
        let error = (hertz - MEASURED_HZ).abs() / MEASURED_HZ;
        assert!(error < 0.001, "frequency = {hertz}");

        let volts = tone.amplitude().volts();
        let error = (volts - TONE_AMPLITUDE).abs() / TONE_AMPLITUDE;
        assert!(error < 0.01, "amplitude = {volts}");
    }

    #[test]
    fn dominant_tone_ignores_a_dc_bias() {
        // The bias is 2.5 times the tone, and would otherwise win.
        let (time, values) = measured_signal();
        let biased: Vec<f64> = values.iter().map(|v| v + 100.0).collect();
        let tone = dominant_tone(&Signal::new(&time, &biased));

        let hertz = tone.frequency().hertz();
        let error = (hertz - MEASURED_HZ).abs() / MEASURED_HZ;
        assert!(error < 0.001, "frequency = {hertz}");
    }

    #[test]
    #[should_panic(expected = "strictly increasing")]
    fn rejects_a_repeated_time() {
        let time = [0.0, 1.0, 1.0, 2.0];
        let values = [0.0, 1.0, 2.0, 3.0];
        Waveform::resample(&Signal::new(&time, &values), 8);
    }

    #[test]
    #[should_panic(expected = "strictly increasing")]
    fn rejects_a_backwards_time() {
        let time = [0.0, 2.0, 1.0, 3.0];
        let values = [0.0, 1.0, 2.0, 3.0];
        Waveform::resample(&Signal::new(&time, &values), 8);
    }

    /// `count` evenly spaced times spanning `MEASURED_SPAN`.
    fn even_times(count: usize) -> Vec<f64> {
        (0..count)
            .map(|i| MEASURED_SPAN * i as f64 / (count - 1) as f64)
            .collect()
    }

    #[test]
    fn measures_the_fundamental_of_a_square_wave() {
        // A unit square holds a fundamental of 4 / pi, not 1.
        let time = even_times(4001);
        let values: Vec<f64> = time
            .iter()
            .map(|t| (2.0 * PI * MEASURED_HZ * t).sin().signum())
            .collect();
        let tone = dominant_tone(&Signal::new(&time, &values));

        let hertz = tone.frequency().hertz();
        assert!(
            (hertz - MEASURED_HZ).abs() / MEASURED_HZ < 0.001,
            "frequency = {hertz}"
        );

        let want = 4.0 / PI;
        let volts = tone.amplitude().volts();
        assert!((volts - want).abs() / want < 0.01, "amplitude = {volts}");
    }

    #[test]
    fn dominant_means_the_largest_of_several_tones() {
        let time = even_times(4001);
        let loud = 2.5 * MEASURED_HZ;
        let values: Vec<f64> = time
            .iter()
            .map(|t| {
                (2.0 * PI * MEASURED_HZ * t).sin()
                    + 3.0 * (2.0 * PI * loud * t).sin()
            })
            .collect();
        let tone = dominant_tone(&Signal::new(&time, &values));

        let hertz = tone.frequency().hertz();
        assert!((hertz - loud).abs() / loud < 0.001, "frequency = {hertz}");

        let volts = tone.amplitude().volts();
        assert!((volts - 3.0).abs() / 3.0 < 0.01, "amplitude = {volts}");
    }

    #[test]
    fn a_flat_signal_reports_no_amplitude() {
        // The documented way to tell there is no tone to find.
        let time = even_times(1001);
        let values = vec![DC_BIAS; time.len()];
        let tone = dominant_tone(&Signal::new(&time, &values));

        let volts = tone.amplitude().volts();
        assert!(volts < 1e-9, "amplitude = {volts}");
    }

    #[test]
    fn samples_per_cycle_reports_the_simulator_rate() {
        let time = even_times(4001);
        let values: Vec<f64> = time
            .iter()
            .map(|t| (2.0 * PI * MEASURED_HZ * t).sin())
            .collect();
        let tone = dominant_tone(&Signal::new(&time, &values));

        // 4000 gaps over 20 ms is 200 kHz, against a 1234 Hz tone.
        let want = (4000.0 / MEASURED_SPAN) / MEASURED_HZ;
        let got = tone.samples_per_cycle();
        assert!((got - want).abs() / want < 0.01, "got {got}");
    }

    #[test]
    fn samples_per_cycle_exposes_an_aliased_measurement() {
        // 8 kHz sampled at 10 kHz folds down to 2 kHz, and the tone
        // looks entirely credible. Only the sample rate gives it away.
        let time = even_times(201);
        let values: Vec<f64> =
            time.iter().map(|t| (2.0 * PI * 8000.0 * t).sin()).collect();
        let tone = dominant_tone(&Signal::new(&time, &values));

        let hertz = tone.frequency().hertz();
        assert!((hertz - 2000.0).abs() < 50.0, "alias at {hertz}");

        let got = tone.samples_per_cycle();
        assert!(got < 10.0, "samples per cycle = {got}");
    }

    /// StdRng is not portable, so a rand upgrade may change these
    /// samples. The tolerances carry enough margin for that.
    const NOISE_SEED: u64 = 7;

    /// The measured tone buried in Gaussian noise at `snr_db`.
    fn noisy_tone(snr_db: f64) -> (Vec<f64>, Vec<f64>) {
        let sigma = TONE_AMPLITUDE / 10f64.powf(snr_db / 20.0);
        let normal = Normal::new(0.0, sigma).expect("sigma is finite");
        let mut rng = StdRng::seed_from_u64(NOISE_SEED);

        let time = even_times(4001);
        let values = time
            .iter()
            .map(|t| {
                let clean = 2.0 * PI * MEASURED_HZ * t;
                TONE_AMPLITUDE * clean.sin() + normal.sample(&mut rng)
            })
            .collect();
        (time, values)
    }

    #[test]
    fn frequency_survives_noise_as_loud_as_the_tone() {
        // The transform concentrates the tone into one lobe while the
        // noise spreads over every bin, so 0 dB in the time domain is
        // still a clear peak. Measured 0.008% to 0.243% over eight
        // seeds.
        let (time, values) = noisy_tone(0.0);
        let tone = dominant_tone(&Signal::new(&time, &values));

        let hertz = tone.frequency().hertz();
        let error = (hertz - MEASURED_HZ).abs() / MEASURED_HZ;
        assert!(error < 0.01, "frequency = {hertz}");
    }

    #[test]
    fn frequency_is_lost_once_the_noise_buries_the_tone() {
        // The floor sits between -10 dB, where the error grows to
        // 0.3%, and -20 dB, where the peak is simply the loudest noise
        // bin. Pinned so the limit is a documented fact, not folklore.
        let (time, values) = noisy_tone(-20.0);
        let tone = dominant_tone(&Signal::new(&time, &values));

        let hertz = tone.frequency().hertz();
        let error = (hertz - MEASURED_HZ).abs() / MEASURED_HZ;
        assert!(error > 0.01, "frequency = {hertz}");
    }
}
