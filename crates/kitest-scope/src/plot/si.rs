//! Numbers written with a unit, with or without an SI prefix.

/// The SI prefixes, smallest first, each a symbol and a power of ten.
const PREFIXES: [(&str, i32); 9] = [
    ("p", -12),
    ("n", -9),
    ("µ", -6),
    ("m", -3),
    ("", 0),
    ("k", 3),
    ("M", 6),
    ("G", 9),
    ("T", 12),
];

/// `value` in `unit`, with the prefix that suits `magnitude` and the decimals
/// that resolve `resolution`.
///
/// Labels that share a `magnitude` share a prefix.
pub fn format(
    value: f64,
    resolution: f64,
    magnitude: f64,
    unit: &str,
) -> String {
    let (prefix, power) = prefix(magnitude);
    let scale = 10f64.powi(power);
    let number = fixed(value / scale, resolution / scale);
    format!("{number} {prefix}{unit}")
}

/// `value` without a prefix, to the decimals that resolve `resolution`,
/// followed by `suffix`.
pub fn plain(value: f64, resolution: f64, suffix: &str) -> String {
    format!("{}{suffix}", fixed(value, resolution))
}

/// `value` to the decimals that resolve `resolution`, with no negative zero.
fn fixed(value: f64, resolution: f64) -> String {
    let decimals =
        (-(resolution.abs().log10() + 1e-6).floor()).clamp(0.0, 12.0) as usize;
    let value = if value.abs() < 0.5 * 10f64.powi(-(decimals as i32)) {
        0.0
    } else {
        value
    };
    format!("{value:.decimals$}")
}

/// The prefix symbol and power of ten that suit `magnitude`.
fn prefix(magnitude: f64) -> (&'static str, i32) {
    let magnitude = magnitude.abs() * (1.0 + 1e-9);
    if magnitude == 0.0 || !magnitude.is_finite() {
        return ("", 0);
    }
    PREFIXES
        .iter()
        .rev()
        .find(|(_, power)| magnitude >= 10f64.powi(*power))
        .copied()
        .unwrap_or(PREFIXES[0])
}

#[cfg(test)]
mod tests {
    use super::{format, plain};

    #[test]
    fn prefix_follows_magnitude() {
        assert_eq!(format(0.0015, 0.0005, 0.005, "s"), "1.5 ms");
        assert_eq!(format(2e-7, 1e-7, 2e-7, "s"), "200 ns");
        assert_eq!(format(159.0, 1.0, 159.0, "Hz"), "159 Hz");
        assert_eq!(format(1000.0, 100.0, 1000.0, "Hz"), "1.0 kHz");
        let below = 999_999.999_999_9;
        assert_eq!(format(below, below, below, "Hz"), "1 MHz");
    }

    #[test]
    fn zero_takes_the_shared_prefix() {
        assert_eq!(format(0.0, 0.0005, 0.005, "s"), "0.0 ms");
        assert_eq!(format(-1e-19, 0.0005, 0.005, "s"), "0.0 ms");
        assert_eq!(format(0.0, 1.0, 0.0, "V"), "0 V");
    }

    #[test]
    fn decimals_resolve_the_step() {
        assert_eq!(format(0.25, 0.05, 1.0, "V"), "0.25 V");
        assert_eq!(format(-0.3, 0.2, 1.0, "V"), "-0.3 V");
    }

    #[test]
    fn plain_numbers_take_no_prefix() {
        assert_eq!(plain(-3.0103, 0.01, " dB"), "-3.01 dB");
        assert_eq!(plain(-45.0, 20.0, "°"), "-45°");
        assert_eq!(plain(-0.001, 1.0, "°"), "0°");
    }
}
