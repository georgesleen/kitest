//! Waveform views and assertions over [Results].

/// How close a value must be to a target.
#[derive(Debug, Clone, Copy)]
pub enum Tolerance {
    Abs(f64),
    Pct(f64),
}

impl Tolerance {
    pub fn abs(v: f64) -> Self {
        Self::Abs(v)
    }

    pub fn pct(p: f64) -> Self {
        Self::Pct(p)
    }

    fn band(self, target: f64) -> f64 {
        match self {
            Self::Abs(v) => v,
            Self::Pct(p) => p / 100.0 * target.abs(),
        }
    }
}

pub struct Signal<'a> {
    time: &'a [f64],
    values: &'a [f64],
}

impl<'a> Signal<'a> {
    pub(crate) fn new(time: &'a [f64], values: &'a [f64]) -> Self {
        Self { time, values }
    }

    /// True if signal stays within `tol` of `target` over the last `window` seconds.
    pub fn settles_to(&self, target: f64, tol: Tolerance, window: f64) -> bool {
        let band = tol.band(target);
        let (Some(&first), Some(&last)) = (self.time.first(), self.time.last()) else {
            return false;
        };
        if last - first < window {
            return false;
        }
        let start = last - window;
        self.time
            .iter()
            .zip(self.values)
            .filter(|(t, _)| **t >= start)
            .all(|(_, v)| (*v - target).abs() <= band)
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
