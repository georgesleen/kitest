//! Scalar node voltage from an operating point, and its assertion.

use crate::Tolerance;

/// Single value associated with a node at an operating point.
#[derive(Debug, Clone, Copy)]
pub struct Voltage {
    volts: f64,
}

impl Voltage {
    pub(crate) fn new(volts: f64) -> Self {
        Self { volts }
    }

    /// Return the value of the Voltage at a node.
    pub fn volts(&self) -> f64 {
        self.volts
    }

    /// Checks that the voltage at a node is within the specified tolerance
    pub fn near(&self, expected: f64, tolerance: Tolerance) -> bool {
        (self.volts - expected).abs() <= tolerance.band(expected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_accepts_value_inside_absolute_band() {
        let v = Voltage::new(2.49);
        assert!(v.near(2.5, Tolerance::abs(0.05)));
    }

    #[test]
    fn near_rejects_value_outside_absolute_band() {
        let v = Voltage::new(2.4);
        assert!(!v.near(2.5, Tolerance::abs(0.05)));
    }

    #[test]
    fn near_uses_percent_of_target() {
        // 1% of 5.0 is a 0.05 band around the target.
        assert!(Voltage::new(4.96).near(5.0, Tolerance::pct(1.0)));
        assert!(!Voltage::new(4.9).near(5.0, Tolerance::pct(1.0)));
    }

    #[test]
    fn volts_returns_raw_value() {
        assert_eq!(Voltage::new(3.3).volts(), 3.3);
    }
}
