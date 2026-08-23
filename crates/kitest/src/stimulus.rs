//! Test-side stimulus injected into a netlist body.

/// Prefix for injected sources, kept distinct from a design's own `V` elements.
const SUPPLY_PREFIX: &str = "Vkt";

/// A source that can render itself as a SPICE element line.
pub(crate) trait Stimulus {
    fn spice_line(&self, name: &str) -> String;
}

/// A DC voltage supply.
pub struct DcSupply {
    node: String,
    volts: f64,
}

impl DcSupply {
    /// DC supply of `volts` on `node`.
    pub fn new(node: &str, volts: f64) -> Self {
        Self {
            node: node.to_owned(),
            volts,
        }
    }
}

impl Stimulus for DcSupply {
    /// Render as a SPICE source line with the given element name.
    fn spice_line(&self, name: &str) -> String {
        format!(
            "{name} {node} 0 {volts}",
            node = self.node,
            volts = self.volts
        )
    }
}

/// An AC supply.
pub struct AcSupply {
    node: String,
    bias: f64,
    magnitude: f64,
}

impl AcSupply {
    /// Create a new AcSupply with defaults of unit drive and zero bias.
    pub fn new(node: &str) -> Self {
        Self {
            node: node.to_owned(),
            bias: 0.0,
            magnitude: 1.0,
        }
    }

    /// Set the DC bias point.
    pub fn bias(self, bias: f64) -> Self {
        Self { bias, ..self }
    }

    /// Set the small-signal magnitude.
    pub fn magnitude(self, magnitude: f64) -> Self {
        Self { magnitude, ..self }
    }
}

impl Stimulus for AcSupply {
    /// Render as a SPICE source line with the given element name.
    fn spice_line(&self, name: &str) -> String {
        format!(
            "{name} {node} 0 dc {bias} ac {magnitude}",
            node = self.node,
            bias = self.bias,
            magnitude = self.magnitude
        )
    }
}

pub(crate) fn inject<S: Stimulus>(netlist: &str, sources: &[S]) -> String {
    let mut deck = netlist.to_owned();
    for (index, source) in sources.iter().enumerate() {
        deck.push('\n');
        deck.push_str(&source.spice_line(&format!("{SUPPLY_PREFIX}{}", index + 1)));
    }
    deck
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_supply_line() {
        assert_eq!(
            inject("R1 a b 1k", &[DcSupply::new("a", 5.0)]),
            "R1 a b 1k\nVkt1 a 0 5"
        );
    }

    #[test]
    fn numbers_multiple_supplies() {
        let deck = inject("* net", &[DcSupply::new("a", 5.0), DcSupply::new("b", 3.3)]);
        assert_eq!(deck, "* net\nVkt1 a 0 5\nVkt2 b 0 3.3");
    }

    #[test]
    fn no_supplies_leaves_netlist_unchanged() {
        assert_eq!(inject::<DcSupply>("R1 a b 1k", &[]), "R1 a b 1k");
    }

    #[test]
    fn ac_supply_defaults_to_unit_drive() {
        assert_eq!(
            AcSupply::new("vin").spice_line("Vkt1"),
            "Vkt1 vin 0 dc 0 ac 1"
        );
    }

    #[test]
    fn ac_supply_builder_sets_bias_and_magnitude() {
        assert_eq!(
            AcSupply::new("vin")
                .bias(2.5)
                .magnitude(0.5)
                .spice_line("Vkt1"),
            "Vkt1 vin 0 dc 2.5 ac 0.5"
        );
    }

    #[test]
    fn inject_numbers_ac_supplies() {
        let deck = inject("* net", &[AcSupply::new("a"), AcSupply::new("b").bias(1.0)]);
        assert_eq!(deck, "* net\nVkt1 a 0 dc 0 ac 1\nVkt2 b 0 dc 1 ac 1");
    }
}
