//! What a failed check's operating point says about why it failed.

use crate::kicad::node_name;
use crate::{OperatingPoint, Transistor};

/// Base-emitter voltage below which a silicon BJT is counted as off.
const BJT_ON_VOLTS: f64 = 0.5;
/// Collector-emitter voltage below which a conducting BJT is saturated.
const BJT_SATURATED_VOLTS: f64 = 0.2;
/// Largest DC voltage, either sign, counted as an unpowered net.
const DEAD_VOLTS: f64 = 1e-3;

/// One line per finding: the bias of `probe_net`, when the failed check is
/// not itself a DC voltage, each transistor's bias, and any labelled net
/// that sits at 0 V while feeding a collector or drain.
///
/// `labelled` are the nets named by a label that kitest neither sources nor
/// grounds: a supply reaching the circuit through one of those is undriven.
pub(crate) fn diagnose(
    op: &OperatingPoint,
    probe_net: Option<&str>,
    transistors: &[Transistor],
    labelled: &[&str],
) -> Vec<String> {
    let volts = |node: &str| op.node(node).map(|voltage| voltage.volts());
    let mut lines = Vec::new();
    if let Some(net) = probe_net
        && let Some(bias) = volts(net)
    {
        lines.push(format!("{net} sits at {bias:.3} V DC"));
    }
    lines.extend(
        transistors
            .iter()
            .filter_map(|transistor| bias(transistor, &volts)),
    );
    lines.extend(
        labelled
            .iter()
            .filter_map(|&net| undriven(net, transistors, &volts)),
    );
    let dead = op
        .nodes()
        .iter()
        .all(|&node| volts(node).is_some_and(|v| v.abs() < DEAD_VOLTS));
    if dead {
        lines.push(
            "every net sits at 0 V DC: nothing powers the circuit".to_owned(),
        );
    }
    lines
}

/// `transistor`'s terminal voltages, and for a BJT whether it is off,
/// saturated, or active.
fn bias(
    transistor: &Transistor,
    volts: &impl Fn(&str) -> Option<f64>,
) -> Option<String> {
    let [first, control, common] =
        [0, 1, 2].map(|i| volts(&transistor.nodes[i]));
    let (first, control, common) = (first?, control?, common?);
    let name = &transistor.reference;
    let kind = transistor.model_type;
    Some(match kind {
        "npn" | "pnp" => {
            let sign = if kind == "npn" { 1.0 } else { -1.0 };
            let vbe = sign * (control - common);
            let vce = sign * (first - common);
            let state = if vbe < BJT_ON_VOLTS {
                "off"
            } else if vce < BJT_SATURATED_VOLTS {
                "saturated"
            } else {
                "active"
            };
            let (vbe, vce) = (control - common, first - common);
            format!(
                "{name} ({kind}) is {state}: Vbe {vbe:.3} V, Vce {vce:.3} V"
            )
        }
        _ => {
            let (vgs, vds) = (control - common, first - common);
            format!("{name} ({kind}): Vgs {vgs:.3} V, Vds {vds:.3} V")
        }
    })
}

/// A finding for labelled `net` if it sits at 0 V and feeds a collector or
/// drain, the signature of a supply nothing drives.
fn undriven(
    net: &str,
    transistors: &[Transistor],
    volts: &impl Fn(&str) -> Option<f64>,
) -> Option<String> {
    if volts(net)?.abs() >= DEAD_VOLTS {
        return None;
    }
    let node = node_name(net);
    let fed: Vec<String> = transistors
        .iter()
        .filter(|transistor| transistor.nodes[0].eq_ignore_ascii_case(&node))
        .map(|transistor| {
            let terminal = match transistor.model_type {
                "npn" | "pnp" => "collector",
                _ => "drain",
            };
            format!("{}'s {terminal}", transistor.reference)
        })
        .collect();
    (!fed.is_empty()).then(|| {
        format!(
            "{net} sits at 0 V DC and feeds {}; if it is a supply, give its \
             voltage under [supplies] in kitest.toml",
            fed.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn op(voltages: &[(&str, f64)]) -> OperatingPoint {
        let voltages: BTreeMap<String, f64> = voltages
            .iter()
            .map(|&(node, volts)| (node.to_lowercase(), volts))
            .collect();
        OperatingPoint::new(voltages)
    }

    fn bjt(model_type: &'static str) -> Transistor {
        Transistor {
            reference: "Q1".into(),
            model_type,
            nodes: vec!["c".into(), "b".into(), "e".into()],
        }
    }

    fn state(model_type: &'static str, c: f64, b: f64, e: f64) -> String {
        let op = op(&[("c", c), ("b", b), ("e", e)]);
        diagnose(&op, None, &[bjt(model_type)], &[]).join("\n")
    }

    #[test]
    fn a_bjt_reads_off_saturated_or_active_by_its_polarity() {
        assert!(state("npn", 5.0, 0.3, 0.0).contains("is off"));
        assert!(state("npn", 0.1, 0.7, 0.0).contains("is saturated"));
        assert!(state("npn", 5.0, 0.7, 0.0).contains("is active"));
        assert!(state("pnp", -5.0, -0.7, 0.0).contains("is active"));
        assert!(state("pnp", 5.0, 0.7, 0.0).contains("is off"));
    }

    #[test]
    fn a_labelled_net_at_zero_volts_on_a_collector_reads_as_an_undriven_supply()
    {
        let op = op(&[("VCC", 0.0), ("c", 0.0), ("b", 0.0), ("e", 0.0)]);
        let mut q1 = bjt("npn");
        q1.nodes[0] = "VCC".into();
        let lines = diagnose(&op, None, &[q1], &["VCC", "/IN"]);
        let undriven: Vec<&String> = lines
            .iter()
            .filter(|line| line.contains("[supplies]"))
            .collect();
        assert_eq!(undriven.len(), 1, "{lines:?}");
        assert!(
            undriven[0].starts_with("VCC")
                && undriven[0].contains("Q1's collector")
        );
    }
}
