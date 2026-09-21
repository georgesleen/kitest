//! Test-side stimulus injected into a netlist body.

/// Prefix for injected sources, kept distinct from a design's own `V` elements.
const SUPPLY_PREFIX: &str = "Vkt";

/// A source that can render itself as a SPICE element line.
pub(crate) trait Stimulus {
    fn spice_line(&self, name: &str) -> String;
}

/// A DC voltage supply.
#[derive(Clone)]
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
#[derive(Clone)]
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

/// Near-ideal edge that still keeps the transient solver converging.
const IDEAL_EDGE: f64 = 1e-9;
/// A width and period long enough that a stepped pulse never repeats in a run.
const ONE_SHOT: f64 = 1e30;

/// A time-varying source for transient analysis, driving `node`.
#[derive(Clone)]
pub struct TranSource {
    node: String,
    excitation: Excitation,
}

/// The excitation a [`TranSource`] plays. Each variant carries only its own
/// parameters, so a pulse and a sine can never be confused.
#[derive(Clone)]
enum Excitation {
    Dc(f64),
    Pulse(Pulse),
    Sin(Sin),
}

/// Supply noise for a DC source: amplitude in volts, and the interval
/// ngspice draws each new sample over.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Noise {
    pub volts: f64,
    pub interval: f64,
}

impl TranSource {
    /// Hold `node` at a constant `volts`.
    ///
    /// Only a constant source can carry supply noise, so a circuit that
    /// has to start itself, such as an oscillator, wants its supply
    /// driven this way.
    pub fn dc(node: &str, volts: f64) -> Self {
        Self {
            node: node.to_owned(),
            excitation: Excitation::Dc(volts),
        }
    }

    /// Drive `node` with a pulse waveform.
    pub fn pulse(node: &str, pulse: Pulse) -> Self {
        Self {
            node: node.to_owned(),
            excitation: Excitation::Pulse(pulse),
        }
    }

    /// Drive `node` with a sine waveform.
    pub fn sin(node: &str, sin: Sin) -> Self {
        Self {
            node: node.to_owned(),
            excitation: Excitation::Sin(sin),
        }
    }

    /// Render with `noise` added, which only a DC source can carry.
    ///
    /// ngspice silently discards a transient function when `trnoise`
    /// sits beside it, so a pulse or a sine renders unchanged and the
    /// noise is dropped rather than eating the stimulus.
    pub(crate) fn spice_line_noisy(
        &self,
        name: &str,
        noise: Option<Noise>,
    ) -> String {
        let line = self.spice_line(name);
        match (&self.excitation, noise) {
            (Excitation::Dc(_), Some(noise)) => {
                let Noise { volts, interval } = noise;
                format!("{line} trnoise({volts} {interval} 0 0)")
            }
            _ => line,
        }
    }
}

impl Stimulus for TranSource {
    /// Render as a SPICE source line with the given element name.
    fn spice_line(&self, name: &str) -> String {
        let spec = match &self.excitation {
            Excitation::Dc(volts) => format!("dc {volts}"),
            Excitation::Pulse(pulse) => pulse.spec(),
            Excitation::Sin(sin) => sin.spec(),
        };
        format!("{name} {node} 0 {spec}", node = self.node)
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
            &source.spice_line(&format!("{SUPPLY_PREFIX}{}", index + 1)),
        );
    }
    deck
}

/// Inject transient sources, hanging `noise` on the DC ones.
pub(crate) fn inject_tran(
    netlist: &str,
    sources: &[TranSource],
    noise: Option<Noise>,
) -> String {
    let mut deck = netlist.to_owned();
    for (index, source) in sources.iter().enumerate() {
        let name = format!("{SUPPLY_PREFIX}{}", index + 1);
        deck.push('\n');
        deck.push_str(&source.spice_line_noisy(&name, noise));
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
            TranSource::pulse("vin", Pulse::step(0.0, 1.0)).spice_line("Vkt1"),
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
        .spice_line("Vkt1");
        assert_eq!(
            line,
            "Vkt1 clk 0 PULSE(0 3.3 0 0.000000001 0.000000001 0.0000005 0.000001)"
        );
    }

    #[test]
    fn sin_renders_with_default_delay() {
        assert_eq!(
            TranSource::sin("vin", Sin::new(0.0, 1.0, 1e3)).spice_line("Vkt1"),
            "Vkt1 vin 0 SIN(0 1 1000 0)"
        );
    }

    #[test]
    fn inject_numbers_tran_sources() {
        let deck = inject(
            "* net",
            &[
                TranSource::sin("a", Sin::new(0.0, 1.0, 1e3)),
                TranSource::sin("b", Sin::new(0.0, 2.0, 1e3)),
            ],
        );
        assert_eq!(
            deck,
            "* net\nVkt1 a 0 SIN(0 1 1000 0)\nVkt2 b 0 SIN(0 2 1000 0)"
        );
    }

    const NOISE: Noise = Noise {
        volts: 1e-3,
        interval: 1e-9,
    };

    #[test]
    fn dc_source_renders_a_constant() {
        let source = TranSource::dc("vcc", 9.0);
        assert_eq!(source.spice_line("Vkt1"), "Vkt1 vcc 0 dc 9");
    }

    #[test]
    fn noise_rides_a_dc_source() {
        let source = TranSource::dc("vcc", 9.0);
        assert_eq!(
            source.spice_line_noisy("Vkt1", Some(NOISE)),
            "Vkt1 vcc 0 dc 9 trnoise(0.001 0.000000001 0 0)"
        );
    }

    #[test]
    fn noise_never_rides_a_transient_function() {
        // ngspice silently drops the PULSE if trnoise sits beside it,
        // so the stimulus wins and the noise is left off.
        let source = TranSource::pulse("vin", Pulse::step(0.0, 1.0));
        let plain = source.spice_line("Vkt1");
        assert_eq!(source.spice_line_noisy("Vkt1", Some(NOISE)), plain);

        let source = TranSource::sin("vin", Sin::new(0.0, 1.0, 1e3));
        let plain = source.spice_line("Vkt1");
        assert_eq!(source.spice_line_noisy("Vkt1", Some(NOISE)), plain);
    }

    #[test]
    fn an_ideal_run_asks_for_no_noise() {
        let source = TranSource::dc("vcc", 9.0);
        let plain = source.spice_line("Vkt1");
        assert_eq!(source.spice_line_noisy("Vkt1", None), plain);
    }
}
