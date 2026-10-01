//! What a power rail's name says about it: a voltage, or ground.

/// Names that mean ground, compared ignoring ASCII case.
const GROUND_NAMES: &[&str] = &[
    "GND", "GNDA", "GNDD", "GNDREF", "GNDPWR", "GNDS", "AGND", "DGND", "PGND",
    "SGND", "CGND", "0V", "0", "EARTH",
];

/// Node names ngspice itself treats as ground, compared ignoring ASCII case.
const SPICE_GROUND_NODES: &[&str] = &["0", "gnd"];

/// Letters that may follow a rail voltage to mark its domain, as in `+5VA`.
const DOMAIN_SUFFIXES: &[char] = &['A', 'D'];

/// The voltage `net`'s name states, if its last path segment states exactly one.
pub(crate) fn stated_volts(net: &str) -> Option<f64> {
    let mut stated = last_segment(net).split('_').filter_map(segment_volts);
    let volts = stated.next()?;
    stated.next().is_none().then_some(volts)
}

/// True if `net`'s last path segment is a ground name.
pub(crate) fn is_ground(net: &str) -> bool {
    let name = last_segment(net);
    GROUND_NAMES
        .iter()
        .any(|ground| ground.eq_ignore_ascii_case(name))
}

/// True if ngspice treats SPICE node `node` as ground without a source.
pub(crate) fn is_spice_ground(node: &str) -> bool {
    SPICE_GROUND_NODES
        .iter()
        .any(|ground| ground.eq_ignore_ascii_case(node))
}

fn last_segment(net: &str) -> &str {
    net.rsplit('/').next().unwrap_or(net)
}

/// The voltage one `_`-separated segment spells, as in `+5V`, `3V3`, or `1.8V`.
fn segment_volts(segment: &str) -> Option<f64> {
    let (negative, unsigned) = match segment.strip_prefix(['+', '-']) {
        Some(rest) => (segment.starts_with('-'), rest),
        None => (false, segment),
    };
    let unsigned = unsigned.strip_suffix(DOMAIN_SUFFIXES).unwrap_or(unsigned);
    let (whole, fraction) = unsigned.split_once('V')?;
    let (whole, fraction) = match whole.split_once('.') {
        Some((integer, decimals)) if fraction.is_empty() => (integer, decimals),
        Some(_) => return None,
        None => (whole, fraction),
    };
    let digits = |text: &str| text.bytes().all(|byte| byte.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(fraction) {
        return None;
    }
    let magnitude: f64 = if fraction.is_empty() {
        whole.parse().ok()?
    } else {
        format!("{whole}.{fraction}").parse().ok()?
    };
    Some(if negative { -magnitude } else { magnitude })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_voltage_a_rail_name_states() {
        let cases = [
            ("+5V", 5.0),
            ("5V", 5.0),
            ("-15V", -15.0),
            ("+3V3", 3.3),
            ("+3.3V", 3.3),
            ("+1V8", 1.8),
            ("12V", 12.0),
            ("+5VA", 5.0),
            ("+3V3D", 3.3),
            ("VDD_1V8", 1.8),
            ("/A/+5V", 5.0),
        ];
        for (name, volts) in cases {
            assert_eq!(stated_volts(name), Some(volts), "{name}");
        }
    }

    #[test]
    fn a_name_without_exactly_one_voltage_states_none() {
        for name in [
            "VCC", "VDD", "VSS", "VBUS", "VBAT", "+BATT", "-VDC", "3V3_5V",
            "V5", "+5VX", "+", "V", "1.8V3", "/5V/OUT",
        ] {
            assert_eq!(stated_volts(name), None, "{name}");
        }
    }

    #[test]
    fn recognises_ground_names_ignoring_case() {
        for name in [
            "GND", "gnd", "GNDA", "AGND", "PGND", "0V", "Earth", "/A/GND",
        ] {
            assert!(is_ground(name), "{name}");
        }
        for name in ["VSS", "VCC", "GNDX", "SIGNAL_GND"] {
            assert!(!is_ground(name), "{name}");
        }
    }

    #[test]
    fn only_zero_and_gnd_are_ground_to_ngspice_itself() {
        assert!(is_spice_ground("GND"));
        assert!(is_spice_ground("0"));
        assert!(!is_spice_ground("AGND"));
        assert!(!is_spice_ground("/A/GND"));
    }
}
