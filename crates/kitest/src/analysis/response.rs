//! Frequency-domain response view and its assertions.

use num_complex::Complex64;

/// A frequency-domain response: complex values over a (real) frequency axis.
pub struct Response<'a> {
    frequency: &'a [f64],
    values: &'a [Complex64],
}

impl<'a> Response<'a> {
    /// Decibels per decade of amplitude ratio.
    const DB_PER_DECADE: f64 = 20.0;

    pub fn new(frequency: &'a [f64], values: &'a [Complex64]) -> Self {
        Self { frequency, values }
    }

    /// The frequency axis, in Hz.
    pub fn frequency(&self) -> &[f64] {
        self.frequency
    }

    /// The complex response values.
    pub fn values(&self) -> &[Complex64] {
        self.values
    }

    /// Index of the sweep point whose frequency is nearest `frequency` Hz.
    fn nearest(&self, frequency: f64) -> Option<usize> {
        (0..self.frequency.len()).min_by(|&a, &b| {
            (self.frequency[a] - frequency)
                .abs()
                .total_cmp(&(self.frequency[b] - frequency).abs())
        })
    }

    /// Gain in dB at the sweep point nearest `frequency`.
    pub fn gain_db_at(&self, frequency: f64) -> Option<f64> {
        let i = self.nearest(frequency)?;
        Some(Self::DB_PER_DECADE * self.values[i].norm().log10())
    }

    /// Phase in degrees at the sweep point nearest `frequency`.
    pub fn phase_deg_at(&self, frequency: f64) -> Option<f64> {
        let i = self.nearest(frequency)?;
        Some(self.values[i].arg().to_degrees())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREQ: [f64; 2] = [1.0, 10.0];

    #[test]
    fn gain_db_reads_nearest_point() {
        let values = [Complex64::new(1.0, 0.0), Complex64::new(0.1, 0.0)];
        let r = Response::new(&FREQ, &values);
        assert!((r.gain_db_at(1.0).unwrap()).abs() < 1e-9);
        assert!((r.gain_db_at(9.0).unwrap() + 20.0).abs() < 1e-9);
    }

    #[test]
    fn phase_deg_reads_angle() {
        let values = [Complex64::new(0.0, 1.0), Complex64::new(0.0, -1.0)];
        let r = Response::new(&FREQ, &values);
        assert!((r.phase_deg_at(1.0).unwrap() - 90.0).abs() < 1e-9);
        assert!((r.phase_deg_at(10.0).unwrap() + 90.0).abs() < 1e-9);
    }

    #[test]
    fn response_empty_is_none() {
        let r = Response::new(&[], &[]);
        assert!(r.gain_db_at(1.0).is_none());
    }
}
