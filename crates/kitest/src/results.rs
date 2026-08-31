//! Parsed simulation outputs, one type per analysis domain.

use std::collections::BTreeMap;

use num_complex::Complex64;

use crate::{Response, Signal, signal::Voltage};

/// DC operating-point voltages, one value per node.
#[derive(Debug)]
pub struct OperatingPoint {
    voltages: BTreeMap<String, f64>,
}

impl OperatingPoint {
    pub(crate) fn new(voltages: BTreeMap<String, f64>) -> Self {
        Self { voltages }
    }

    /// DC voltage at `node`, or `None` if that node is absent.
    pub fn node(&self, node: &str) -> Option<Voltage> {
        Some(Voltage::new(
            self.voltages.get(&node.to_lowercase()).copied()?,
        ))
    }
}

/// Time-domain waveforms from a transient analysis.
#[derive(Debug)]
pub struct Waveforms {
    time: Vec<f64>,
    signals: BTreeMap<String, Vec<f64>>,
}

impl Waveforms {
    pub(crate) fn new(
        time: Vec<f64>,
        signals: BTreeMap<String, Vec<f64>>,
    ) -> Self {
        Self { time, signals }
    }

    /// Waveform at `node`, or `None` if that node is absent.
    pub fn node(&self, node: &str) -> Option<Signal<'_>> {
        Some(Signal::new(
            &self.time,
            self.signals.get(&node.to_lowercase())?,
        ))
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

    /// Response at `node`, or `None` if that node is absent.
    pub fn node(&self, node: &str) -> Option<Response<'_>> {
        Some(Response::new(
            &self.frequency,
            self.signals.get(&node.to_lowercase())?,
        ))
    }
}
