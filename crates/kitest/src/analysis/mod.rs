//! Simulation outputs and the assertions over them.

mod check;
mod frequency;
mod response;
mod signal;
mod spectrum;
mod tolerance;
mod tone;
mod voltage;

pub use check::Check;
pub(crate) use check::si;
pub use frequency::Frequency;
pub use response::Response;
pub use signal::Signal;
pub use tolerance::Tolerance;
pub use tone::Tone;
pub use voltage::Voltage;

use std::collections::BTreeMap;

use num_complex::Complex64;

/// DC operating-point voltages, one value per node.
#[derive(Debug)]
pub struct OperatingPoint {
    voltages: BTreeMap<String, f64>,
}

impl OperatingPoint {
    pub(crate) fn new(voltages: BTreeMap<String, f64>) -> Self {
        Self { voltages }
    }

    /// DC voltage at `node`, a SPICE node or schematic net name, or `None`
    /// if that node is absent.
    pub fn node(&self, node: &str) -> Option<Voltage> {
        Some(Voltage::new(self.voltages.get(&key(node)).copied()?))
    }

    /// The node names present in the result.
    pub fn nodes(&self) -> Vec<&str> {
        self.voltages.keys().map(String::as_str).collect()
    }
}

/// Node waveforms from a transient analysis, over a shared time axis.
#[derive(Debug)]
pub struct Transient {
    time: Vec<f64>,
    signals: BTreeMap<String, Vec<f64>>,
}

impl Transient {
    pub(crate) fn new(
        time: Vec<f64>,
        signals: BTreeMap<String, Vec<f64>>,
    ) -> Self {
        Self { time, signals }
    }

    /// Waveform at `node`, a SPICE node or schematic net name, or `None` if
    /// that node is absent.
    pub fn node(&self, node: &str) -> Option<Signal<'_>> {
        Some(Signal::new(&self.time, self.signals.get(&key(node))?))
    }

    /// The time axis and `node`'s values, consuming the result, or `None`
    /// if that node is absent.
    pub(crate) fn into_node(
        mut self,
        node: &str,
    ) -> Option<(Vec<f64>, Vec<f64>)> {
        let values = self.signals.remove(&key(node))?;
        Some((self.time, values))
    }

    /// The node names present in the result.
    pub fn nodes(&self) -> Vec<&str> {
        self.signals.keys().map(String::as_str).collect()
    }
}

/// Frequency-domain responses from an AC analysis.
#[derive(Debug)]
pub struct Spectra {
    frequency: Vec<f64>,
    signals: BTreeMap<String, Vec<Complex64>>,
}

impl Spectra {
    pub(crate) fn new(
        frequency: Vec<f64>,
        signals: BTreeMap<String, Vec<Complex64>>,
    ) -> Self {
        Self { frequency, signals }
    }

    /// Response at `node`, a SPICE node or schematic net name, or `None` if
    /// that node is absent.
    pub fn node(&self, node: &str) -> Option<Response<'_>> {
        Some(Response::new(
            &self.frequency,
            self.signals.get(&key(node))?,
        ))
    }

    /// The node names present in the result.
    pub fn nodes(&self) -> Vec<&str> {
        self.signals.keys().map(String::as_str).collect()
    }
}

/// The result key for `node`: ngspice lowercases node names, and a schematic
/// net name becomes its SPICE node the same way the netlist renames it.
fn key(node: &str) -> String {
    crate::kicad::node_name(node).to_lowercase()
}
