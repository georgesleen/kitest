//! Scalar frequency measurement, and its assertion.

use crate::Tolerance;

/// A frequency measured from a waveform.
#[derive(Debug, Clone, Copy)]
pub struct Frequency {
    hertz: f64,
}

impl Frequency {
    pub(crate) fn new(hertz: f64) -> Self {
        Self { hertz }
    }

    /// Return the value of the Frequency, in hertz.
    pub fn hertz(&self) -> f64 {
        self.hertz
    }

    /// Checks that the frequency is within the specified tolerance
    pub fn near(&self, expected: f64, tolerance: Tolerance) -> bool {
        (self.hertz - expected).abs() <= tolerance.band(expected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_accepts_value_inside_absolute_band() {
        let f = Frequency::new(10.4e6);
        assert!(f.near(10.4e6 + 500.0, Tolerance::abs(1000.0)));
    }

    #[test]
    fn near_rejects_value_outside_absolute_band() {
        let f = Frequency::new(10.4e6);
        assert!(!f.near(10.4e6 + 2000.0, Tolerance::abs(1000.0)));
    }

    #[test]
    fn near_uses_percent_of_target() {
        // 1% of 10 MHz is a 100 kHz band around the target.
        assert!(Frequency::new(10.05e6).near(10e6, Tolerance::percent(1.0)));
        assert!(!Frequency::new(10.2e6).near(10e6, Tolerance::percent(1.0)));
    }

    #[test]
    fn hertz_returns_raw_value() {
        assert_eq!(Frequency::new(10.4e6).hertz(), 10.4e6);
    }
}
