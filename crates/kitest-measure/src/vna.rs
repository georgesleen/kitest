//! Gain margin, phase margin, and group delay of frequency responses.

use crate::Curve;

/// Where phase crosses an odd multiple of -180 degrees and the gain margin there.
///
/// Returns x in the curves' own coordinate and margin in decibels.
pub fn gain_margin(
    magnitude_db: Curve<'_>,
    phase_degrees: Curve<'_>,
) -> Option<(f64, f64)> {
    for (x, phase) in phase_degrees
        .x()
        .windows(2)
        .zip(phase_degrees.y().windows(2))
    {
        let (low, high) = (phase[0].min(phase[1]), phase[0].max(phase[1]));
        let first = ((low + 180.0) / 360.0).ceil() as i64;
        let last = ((high + 180.0) / 360.0).floor() as i64;
        for turn in first..=last {
            let level = -180.0 + 360.0 * turn as f64;
            if phase[0] == phase[1] {
                continue;
            }
            let at = x[0]
                + (x[1] - x[0]) * (level - phase[0]) / (phase[1] - phase[0]);
            let magnitude = magnitude_db.at(at)?;
            return Some((at, -magnitude));
        }
    }
    None
}

/// Where magnitude falls through 0 dB and the phase margin there.
///
/// Returns x in the curves' own coordinate and margin in degrees.
pub fn phase_margin(
    magnitude_db: Curve<'_>,
    phase_degrees: Curve<'_>,
) -> Option<(f64, f64)> {
    let over = *magnitude_db.x().first()?..=*magnitude_db.x().last()?;
    let crossing = magnitude_db
        .crossings(0.0, over)
        .into_iter()
        .find(|crossing| !crossing.rising)?;
    Some((crossing.x, 180.0 + phase_degrees.at(crossing.x)?))
}

/// Group delay at each frequency, in seconds.
///
/// `frequency` is in ascending hertz and `phase_degrees` is unwrapped. The
/// result uses central differences inside and one-sided differences at its ends.
pub fn group_delay(frequency: &[f64], phase_degrees: &[f64]) -> Vec<f64> {
    if frequency.len() != phase_degrees.len() || frequency.len() < 2 {
        return Vec::new();
    }
    (0..frequency.len())
        .map(|index| {
            let (before, after) = if index == 0 {
                (0, 1)
            } else if index + 1 == frequency.len() {
                (index - 1, index)
            } else {
                (index - 1, index + 1)
            };
            -(phase_degrees[after] - phase_degrees[before])
                / (360.0 * (frequency[after] - frequency[before]))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use crate::{Curve, gain_margin, group_delay, phase_margin};

    #[test]
    fn first_order_lowpass_group_delay_matches_its_closed_form() {
        let tau = 1e-3;
        let frequency = [10.0, 100.0, 1000.0, 10_000.0];
        let phase: Vec<f64> = frequency
            .iter()
            .map(|f| -(2.0 * PI * f * tau).atan().to_degrees())
            .collect();
        let delay = group_delay(&frequency, &phase);
        for index in 1..frequency.len() - 1 {
            let expected =
                tau / (1.0 + (2.0 * PI * frequency[index] * tau).powi(2));
            let relative = (delay[index] - expected).abs() / expected;
            assert!(relative < 0.75, "{} != {expected}", delay[index]);
        }
    }

    #[test]
    fn margins_interpolate_the_crossings() {
        let x = [0.0, 1.0, 2.0, 3.0];
        let magnitude = [20.0, 5.0, -5.0, -20.0];
        let phase = [-90.0, -135.0, -180.0, -225.0];
        let magnitude = Curve::new(&x, &magnitude);
        let phase = Curve::new(&x, &phase);
        let (phase_at, degrees) = phase_margin(magnitude, phase).unwrap();
        assert!((phase_at - 1.5).abs() < 1e-12);
        assert!((degrees - 22.5).abs() < 1e-12);
        let (gain_at, decibels) = gain_margin(magnitude, phase).unwrap();
        assert_eq!(gain_at, 2.0);
        assert_eq!(decibels, 5.0);
    }
}
