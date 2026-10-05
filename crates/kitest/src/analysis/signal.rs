//! Time-domain waveform view and its assertions.

use crate::{Tolerance, Tone};

/// A time-domain waveform: real values over a time axis.
pub struct Signal<'a> {
    time: &'a [f64],
    values: &'a [f64],
}

impl<'a> Signal<'a> {
    pub fn new(time: &'a [f64], values: &'a [f64]) -> Self {
        Self { time, values }
    }

    /// The time axis, in seconds.
    pub fn time(&self) -> &[f64] {
        self.time
    }

    /// The sampled values.
    pub fn values(&self) -> &[f64] {
        self.values
    }

    /// The strongest sinusoid in this waveform.
    pub fn dominant_tone(&self) -> Tone {
        super::spectrum::dominant_tone(self)
    }

    /// True if signal stays within `tolerance` of `target` over the last `window` seconds.
    pub fn settles_to(
        &self,
        target: f64,
        tolerance: Tolerance,
        window: f64,
    ) -> bool {
        self.worst_deviation(target, window)
            .is_some_and(|deviation| deviation <= tolerance.band(target))
    }

    /// The largest distance from `target` over the last `window` seconds, or
    /// `None` when the signal is shorter than `window`.
    pub fn worst_deviation(&self, target: f64, window: f64) -> Option<f64> {
        let (&first, &last) = (self.time.first()?, self.time.last()?);
        if last - first < window {
            return None;
        }
        let start = last - window;
        self.time
            .iter()
            .zip(self.values)
            .filter(|(t, _)| **t >= start)
            .map(|(_, v)| (*v - target).abs())
            .reduce(f64::max)
    }

    /// Peak value above `target`, as a fraction of `target`.
    pub fn overshoot(&self, target: f64) -> f64 {
        match self.values.iter().copied().reduce(f64::max) {
            Some(peak) => ((peak - target) / target.abs()).max(0.0),
            None => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIME: [f64; 5] = [0.0, 1.0, 2.0, 3.0, 4.0];

    #[test]
    fn settles_when_tail_in_band() {
        let values = [0.0, 0.6, 0.97, 0.99, 1.0];
        let s = Signal::new(&TIME, &values);
        assert!(s.settles_to(1.0, Tolerance::abs(0.05), 2.0));
    }

    #[test]
    fn rejects_ringing_tail() {
        let values = [0.0, 0.6, 1.1, 0.9, 1.1];
        let s = Signal::new(&TIME, &values);
        assert!(!s.settles_to(1.0, Tolerance::abs(0.05), 2.0));
    }

    #[test]
    fn rejects_window_longer_than_data() {
        let values = [0.0, 0.6, 0.97, 0.99, 1.0];
        let s = Signal::new(&TIME, &values);
        assert!(!s.settles_to(1.0, Tolerance::abs(0.05), 10.0));
    }

    #[test]
    fn rejects_empty_signal() {
        let s = Signal::new(&[], &[]);
        assert!(!s.settles_to(1.0, Tolerance::abs(0.05), 1.0));
    }

    #[test]
    fn worst_deviation_is_the_largest_miss_inside_the_window() {
        let values = [5.0, 0.6, 1.1, 0.9, 1.05];
        let s = Signal::new(&TIME, &values);
        let worst = s.worst_deviation(1.0, 2.0).expect("window fits");
        assert!((worst - 0.1).abs() < 1e-9, "{worst}");
        assert_eq!(s.worst_deviation(1.0, 10.0), None);
    }

    #[test]
    fn overshoot_measures_peak_above_target() {
        let values = [0.0, 1.2, 1.0];
        let s = Signal::new(&TIME[..3], &values);
        assert!((s.overshoot(1.0) - 0.2).abs() < 1e-9);
    }

    #[test]
    fn overshoot_zero_when_monotonic() {
        let values = [0.0, 0.5, 1.0];
        let s = Signal::new(&TIME[..3], &values);
        assert_eq!(s.overshoot(1.0), 0.0);
    }
}
