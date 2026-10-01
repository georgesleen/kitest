//! Test-side stimulus injected into a netlist body.

/// Prefix for injected sources, kept distinct from a design's own elements.
const SUPPLY_PREFIX: &str = "kt";

/// A source that can render itself as a SPICE element line.
pub(crate) trait Stimulus {
    fn spice_line(&self, name: &str) -> String;

    /// The kind of source this renders as.
    fn element(&self) -> Element;
}

/// The kind of independent source kitest injects.
#[derive(Clone, Copy)]
pub(crate) enum Element {
    Voltage,
    Current,
}

impl Element {
    /// The SPICE element letter that selects this source type.
    fn letter(self) -> char {
        match self {
            Self::Voltage => 'V',
            Self::Current => 'I',
        }
    }
}

/// A DC voltage supply.
#[derive(Clone)]
pub struct DcSupply {
    node: String,
    element: Element,
    volts: f64,
}

impl DcSupply {
    /// A DC supply holding `node` at `volts`.
    pub fn new(node: &str, volts: f64) -> Self {
        Self {
            node: node.to_owned(),
            element: Element::Voltage,
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

    fn element(&self) -> Element {
        self.element
    }
}

/// An AC supply.
#[derive(Clone)]
pub struct AcSupply {
    node: String,
    element: Element,
    bias: f64,
    magnitude: f64,
}

impl AcSupply {
    /// An AC voltage source on `node`, of unit magnitude and zero bias.
    pub fn new(node: &str) -> Self {
        Self {
            node: node.to_owned(),
            element: Element::Voltage,
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

    fn element(&self) -> Element {
        self.element
    }
}

/// Near-ideal edge that still keeps the transient solver converging.
const IDEAL_EDGE: f64 = 1e-9;
/// A width and period long enough that a stepped pulse never repeats in a run.
const ONE_SHOT: f64 = 1e30;
/// Print steps a kick lasts for, keeping it well inside one cycle.
const KICK_STEPS: f64 = 10.0;

/// A time-varying source for transient analysis, driving `node`.
#[derive(Clone)]
pub struct TranSource {
    node: String,
    element: Element,
    excitation: Excitation,
}

/// The excitation a [`TranSource`] plays. Each variant carries only its own
/// parameters, so a pulse and a sine can never be confused.
#[derive(Clone)]
enum Excitation {
    /// A constant rail, carrying `noise` volts RMS of supply noise.
    Dc {
        volts: f64,
        noise: f64,
    },
    /// A current impulse of `amps`, timed against the run's print step.
    Kick {
        amps: f64,
    },
    Pulse(Pulse),
    Sin(Sin),
}

impl TranSource {
    /// Hold `node` at a constant `volts`.
    pub fn dc(node: &str, volts: f64) -> Self {
        Self::noisy_dc(node, volts, 0.0)
    }

    /// Hold `node` at `volts`, with `noise` volts RMS of supply noise.
    ///
    /// For a noise floor, not for starting a circuit.
    pub fn noisy_dc(node: &str, volts: f64, noise: f64) -> Self {
        Self {
            node: node.to_owned(),
            element: Element::Voltage,
            excitation: Excitation::Dc { volts, noise },
        }
    }

    /// Drive `node` with a pulse waveform.
    pub fn pulse(node: &str, pulse: Pulse) -> Self {
        Self {
            node: node.to_owned(),
            element: Element::Voltage,
            excitation: Excitation::Pulse(pulse),
        }
    }

    /// Drive `node` with a sine waveform.
    pub fn sin(node: &str, sin: Sin) -> Self {
        Self {
            node: node.to_owned(),
            element: Element::Voltage,
            excitation: Excitation::Sin(sin),
        }
    }

    /// Inject a brief current impulse of `amps` into `node`.
    ///
    /// The impulse carries no DC, so the operating point is unchanged.
    /// Its width and delay follow the run's print step.
    pub fn kick(node: &str, amps: f64) -> Self {
        Self {
            node: node.to_owned(),
            element: Element::Current,
            excitation: Excitation::Kick { amps },
        }
    }

    /// Render as a SPICE element line, timed against a print step of `interval`.
    pub(crate) fn spice_line(&self, name: &str, interval: f64) -> String {
        let node = &self.node;
        match &self.excitation {
            Excitation::Dc { volts, noise } if *noise > 0.0 => {
                format!(
                    "{name} {node} 0 dc {volts} trnoise({noise} {interval} 0 0)"
                )
            }
            Excitation::Dc { volts, .. } => {
                format!("{name} {node} 0 dc {volts}")
            }
            Excitation::Kick { amps } => {
                let spec = Pulse::step(0.0, *amps)
                    .delay(interval)
                    .width(KICK_STEPS * interval)
                    .period(ONE_SHOT)
                    .spec();
                format!("{name} {node} 0 {spec}")
            }
            Excitation::Pulse(pulse) => {
                format!("{name} {node} 0 {}", pulse.spec())
            }
            Excitation::Sin(sin) => format!("{name} {node} 0 {}", sin.spec()),
        }
    }

    pub(crate) fn element(&self) -> Element {
        self.element
    }
}

/// A SPICE `PULSE` waveform. Defaults to a one-shot step; the builder methods
/// shape it into a repeating pulse.
#[derive(Clone)]
pub struct Pulse {
    from: f64,
    to: f64,
    delay: f64,
    rise: f64,
    fall: f64,
    width: f64,
    period: f64,
}

impl Pulse {
    /// A one-shot step from `from` to `to` at t=0 that then holds.
    pub fn step(from: f64, to: f64) -> Self {
        Self {
            from,
            to,
            delay: 0.0,
            rise: IDEAL_EDGE,
            fall: IDEAL_EDGE,
            width: ONE_SHOT,
            period: ONE_SHOT,
        }
    }

    /// Set the delay before the first edge.
    pub fn delay(self, delay: f64) -> Self {
        Self { delay, ..self }
    }

    /// Set the rise time.
    pub fn rise(self, rise: f64) -> Self {
        Self { rise, ..self }
    }

    /// Set the fall time.
    pub fn fall(self, fall: f64) -> Self {
        Self { fall, ..self }
    }

    /// Set the high time, making the pulse repeat once `period` is also set.
    pub fn width(self, width: f64) -> Self {
        Self { width, ..self }
    }

    /// Set the repeat period.
    pub fn period(self, period: f64) -> Self {
        Self { period, ..self }
    }

    fn spec(&self) -> String {
        format!(
            "PULSE({} {} {} {} {} {} {})",
            self.from,
            self.to,
            self.delay,
            self.rise,
            self.fall,
            self.width,
            self.period
        )
    }
}

/// A SPICE `SIN` waveform: a sine of `amplitude` about `offset` at `freq`.
#[derive(Clone)]
pub struct Sin {
    offset: f64,
    amplitude: f64,
    freq: f64,
    delay: f64,
}

impl Sin {
    /// A sine of `amplitude` about `offset` at `freq` hertz, starting at t=0.
    pub fn new(offset: f64, amplitude: f64, freq: f64) -> Self {
        Self {
            offset,
            amplitude,
            freq,
            delay: 0.0,
        }
    }

    /// Set the delay before the sine starts.
    pub fn delay(self, delay: f64) -> Self {
        Self { delay, ..self }
    }

    fn spec(&self) -> String {
        format!(
            "SIN({} {} {} {})",
            self.offset, self.amplitude, self.freq, self.delay
        )
    }
}

pub(crate) fn inject<S: Stimulus>(netlist: &str, sources: &[S]) -> String {
    let mut deck = netlist.to_owned();
    for (index, source) in sources.iter().enumerate() {
        deck.push('\n');
        deck.push_str(
            &source.spice_line(&element_name(source.element(), index)),
        );
    }
    deck
}

/// The unique element name kitest gives the `index`th injected source.
fn element_name(element: Element, index: usize) -> String {
    format!(
        "{letter}{SUPPLY_PREFIX}{n}",
        letter = element.letter(),
        n = index + 1
    )
}

/// Inject transient sources, timed against a print step of `interval`.
pub(crate) fn inject_tran(
    netlist: &str,
    sources: &[TranSource],
    interval: f64,
) -> String {
    let mut deck = netlist.to_owned();
    for (index, source) in sources.iter().enumerate() {
        let name = element_name(source.element(), index);
        deck.push('\n');
        deck.push_str(&source.spice_line(&name, interval));
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
        let deck = inject(
            "* net",
            &[DcSupply::new("a", 5.0), DcSupply::new("b", 3.3)],
        );
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
        let deck = inject(
            "* net",
            &[AcSupply::new("a"), AcSupply::new("b").bias(1.0)],
        );
        assert_eq!(deck, "* net\nVkt1 a 0 dc 0 ac 1\nVkt2 b 0 dc 1 ac 1");
    }

    #[test]
    fn pulse_step_renders_one_shot() {
        assert_eq!(
            TranSource::pulse("vin", Pulse::step(0.0, 1.0))
                .spice_line("Vkt1", 1e-9),
            "Vkt1 vin 0 PULSE(0 1 0 0.000000001 0.000000001 1000000000000000000000000000000 1000000000000000000000000000000)"
        );
    }

    #[test]
    fn pulse_builder_shapes_a_repeating_pulse() {
        let line = TranSource::pulse(
            "clk",
            Pulse::step(0.0, 3.3)
                .rise(1e-9)
                .fall(1e-9)
                .width(5e-7)
                .period(1e-6),
        )
        .spice_line("Vkt1", 1e-9);
        assert_eq!(
            line,
            "Vkt1 clk 0 PULSE(0 3.3 0 0.000000001 0.000000001 0.0000005 0.000001)"
        );
    }

    #[test]
    fn sin_renders_with_default_delay() {
        assert_eq!(
            TranSource::sin("vin", Sin::new(0.0, 1.0, 1e3))
                .spice_line("Vkt1", 1e-9),
            "Vkt1 vin 0 SIN(0 1 1000 0)"
        );
    }

    #[test]
    fn inject_numbers_tran_sources() {
        let deck = inject_tran(
            "* net",
            &[
                TranSource::sin("a", Sin::new(0.0, 1.0, 1e3)),
                TranSource::sin("b", Sin::new(0.0, 2.0, 1e3)),
            ],
            1e-9,
        );
        assert_eq!(
            deck,
            "* net\nVkt1 a 0 SIN(0 1 1000 0)\nVkt2 b 0 SIN(0 2 1000 0)"
        );
    }

    #[test]
    fn dc_source_renders_a_constant() {
        let source = TranSource::dc("vcc", 9.0);
        assert_eq!(source.spice_line("Vkt1", 1e-9), "Vkt1 vcc 0 dc 9");
    }

    #[test]
    fn a_kick_carries_no_dc_and_follows_the_print_step() {
        let deck =
            inject_tran("* net", &[TranSource::kick("tank", 1e-6)], 1e-9);
        assert_eq!(
            deck,
            "* net\nIkt1 tank 0 PULSE(0 0.000001 0.000000001 0.000000001 0.000000001 0.00000001 1000000000000000000000000000000)"
        );
    }

    #[test]
    fn mixed_sources_keep_unique_names() {
        let deck = inject_tran(
            "* net",
            &[TranSource::dc("vcc", 9.0), TranSource::kick("tank", 1e-6)],
            1e-9,
        );
        let names: Vec<&str> = deck
            .lines()
            .skip(1)
            .map(|line| line.split(' ').next().expect("element name"))
            .collect();
        assert_eq!(names, ["Vkt1", "Ikt2"]);
    }

    #[test]
    fn a_rail_carries_its_noise() {
        let source = TranSource::noisy_dc("vcc", 9.0, 1e-3);
        assert_eq!(
            source.spice_line("Vkt1", 1e-9),
            "Vkt1 vcc 0 dc 9 trnoise(0.001 0.000000001 0 0)"
        );
    }

    #[test]
    fn an_ideal_rail_carries_none() {
        let source = TranSource::dc("vcc", 9.0);
        assert_eq!(source.spice_line("Vkt1", 1e-9), "Vkt1 vcc 0 dc 9");
    }

    #[test]
    fn only_a_rail_carries_noise() {
        let pulse = TranSource::pulse("vin", Pulse::step(0.0, 1.0));
        assert!(!pulse.spice_line("Vkt1", 1e-9).contains("trnoise"));

        let sin = TranSource::sin("vin", Sin::new(0.0, 1.0, 1e3));
        assert!(!sin.spice_line("Vkt1", 1e-9).contains("trnoise"));
    }
}
