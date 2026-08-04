//! Test-side stimulus injected into a netlist body.

/// Prefix for injected sources, kept distinct from a design's own `V` elements.
const SUPPLY_PREFIX: &str = "Vkt";

/// Append a DC voltage supply per `(node, volts)`, referenced to ground.
pub fn with_supplies(netlist: &str, supplies: &[(&str, f64)]) -> String {
    let mut deck = netlist.to_owned();
    for (index, (node, volts)) in supplies.iter().enumerate() {
        deck.push_str(&format!("\n{SUPPLY_PREFIX}{} {node} 0 {volts}", index + 1));
    }
    deck
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_supply_line() {
        assert_eq!(
            with_supplies("R1 a b 1k", &[("a", 5.0)]),
            "R1 a b 1k\nVkt1 a 0 5"
        );
    }

    #[test]
    fn numbers_multiple_supplies() {
        let deck = with_supplies("* net", &[("a", 5.0), ("b", 3.3)]);
        assert_eq!(deck, "* net\nVkt1 a 0 5\nVkt2 b 0 3.3");
    }

    #[test]
    fn no_supplies_leaves_netlist_unchanged() {
        assert_eq!(with_supplies("R1 a b 1k", &[]), "R1 a b 1k");
    }
}
