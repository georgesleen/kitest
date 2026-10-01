//! Component values as KiCad writes them, such as `4k7`, `470pF`, or `1M`.

/// The unit a value may name after its multiplier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unit {
    Ohm,
    Farad,
    Henry,
}

impl Unit {
    /// The spellings of this unit accepted after a value.
    fn symbols(self) -> &'static [&'static str] {
        match self {
            Self::Ohm => &["\u{3a9}", "\u{2126}", "ohm", "ohms", "Ohm", "Ohms"],
            Self::Farad => &["F"],
            Self::Henry => &["H"],
        }
    }

    /// The quantity this unit measures, with example values.
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Self::Ohm => "resistance, such as 4k7 or 10kohm",
            Self::Farad => "capacitance, such as 470pF or 100n",
            Self::Henry => "inductance, such as 1uH or 10m",
        }
    }
}

/// SI multipliers by symbol as powers of ten, `meg` first so it is not read
/// as milli.
const MULTIPLIERS: &[(&str, i32)] = &[
    ("meg", 6),
    ("Meg", 6),
    ("MEG", 6),
    ("f", -15),
    ("p", -12),
    ("n", -9),
    ("u", -6),
    ("\u{b5}", -6),
    ("\u{3bc}", -6),
    ("m", -3),
    ("k", 3),
    ("K", 3),
    ("M", 6),
    ("G", 9),
    ("T", 12),
];

/// The resistance code letter that marks the decimal point, as in `4R7`.
const OHM_POINT: &str = "R";

/// The value of `text` in base units, or `None` if it is not a value in `unit`.
pub(crate) fn parse(text: &str, unit: Unit) -> Option<f64> {
    let text = text.trim();
    let (number, rest) = split_number(text);
    if number.is_empty() {
        return None;
    }
    let suffix = rest.trim_start();
    let multiplier = multiplier(suffix, unit);
    let (exponent, after) = multiplier.unwrap_or((0, suffix));
    let (fraction, tail) = split_digits(after);

    let mantissa: f64 = if fraction.is_empty() {
        number.parse().ok()?
    } else {
        let adjacent = multiplier.is_some() && suffix.len() == rest.len();
        if !adjacent || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        format!("{number}.{fraction}").parse().ok()?
    };

    let tail = tail.trim_start();
    if !(tail.is_empty() || unit.symbols().contains(&tail)) {
        return None;
    }
    format!("{mantissa}e{exponent}").parse().ok()
}

/// Split a leading decimal number, with any exponent, from the rest.
fn split_number(text: &str) -> (&str, &str) {
    let mut end = split_digits(text).0.len();
    if let Some(after_point) = text[end..].strip_prefix('.') {
        end += 1 + split_digits(after_point).0.len();
    }
    if !text[..end].bytes().any(|byte| byte.is_ascii_digit()) {
        return ("", text);
    }
    if let Some(after_marker) = text[end..].strip_prefix(['e', 'E']) {
        let signed = after_marker
            .strip_prefix(['+', '-'])
            .unwrap_or(after_marker);
        let digits = split_digits(signed).0.len();
        if digits > 0 {
            end = text.len() - signed.len() + digits;
        }
    }
    text.split_at(end)
}

/// Split leading ASCII digits from the rest.
fn split_digits(text: &str) -> (&str, &str) {
    let end = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    text.split_at(end)
}

/// The multiplier `text` starts with, and what follows it.
fn multiplier(text: &str, unit: Unit) -> Option<(i32, &str)> {
    if unit == Unit::Ohm
        && let Some(rest) = text.strip_prefix(OHM_POINT)
    {
        return Some((0, rest));
    }
    MULTIPLIERS.iter().find_map(|&(symbol, exponent)| {
        text.strip_prefix(symbol).map(|rest| (exponent, rest))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: Option<f64>, expected: f64) -> bool {
        actual.is_some_and(|value| {
            (value - expected).abs() <= expected.abs() * 1e-12
        })
    }

    #[test]
    fn capital_m_is_mega_and_lowercase_m_is_milli() {
        assert!(close(parse("1M", Unit::Ohm), 1e6));
        assert!(close(parse("1m", Unit::Ohm), 1e-3));
        assert!(close(parse("1meg", Unit::Ohm), 1e6));
        assert!(close(parse("2.2Meg", Unit::Ohm), 2.2e6));
    }

    #[test]
    fn a_multiplier_between_digits_is_the_decimal_point() {
        assert!(close(parse("4k7", Unit::Ohm), 4.7e3));
        assert!(close(parse("4R7", Unit::Ohm), 4.7));
        assert!(close(parse("100R", Unit::Ohm), 100.0));
        assert!(close(parse("2n2", Unit::Farad), 2.2e-9));
        assert!(close(parse("1M5", Unit::Ohm), 1.5e6));
    }

    #[test]
    fn reads_units_prefixes_and_spacing() {
        assert!(close(parse("470pF", Unit::Farad), 470e-12));
        assert!(close(parse("1uH", Unit::Henry), 1e-6));
        assert!(close(parse("1\u{b5}H", Unit::Henry), 1e-6));
        assert!(close(parse("10k\u{3a9}", Unit::Ohm), 10e3));
        assert!(close(parse("47kohm", Unit::Ohm), 47e3));
        assert!(close(parse("4.7 k", Unit::Ohm), 4.7e3));
        assert!(close(parse(" 100n ", Unit::Farad), 100e-9));
        assert!(close(parse("1e-9", Unit::Farad), 1e-9));
        assert!(close(parse("47", Unit::Ohm), 47.0));
    }

    #[test]
    fn lowercase_f_is_femto_and_capital_f_is_farads() {
        assert!(close(parse("1f", Unit::Farad), 1e-15));
        assert!(close(parse("1F", Unit::Farad), 1.0));
        assert!(close(parse("1fF", Unit::Farad), 1e-15));
    }

    #[test]
    fn rejects_a_unit_belonging_to_another_part() {
        assert_eq!(parse("1uH", Unit::Farad), None);
        assert_eq!(parse("470pF", Unit::Ohm), None);
        assert_eq!(parse("4R7", Unit::Farad), None);
    }

    #[test]
    fn rejects_text_that_is_not_a_value() {
        for text in
            ["", "k", "abc", "10k 1%", "4.7k7", "4 k7", ".", "1e", "R47"]
        {
            assert_eq!(parse(text, Unit::Ohm), None, "{text:?}");
        }
    }
}
