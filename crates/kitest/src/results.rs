//! Parsed output of a simulation run.

use std::collections::BTreeMap;

/// Signals from one analysis.
#[derive(Debug)]
pub struct Results {
    signals: BTreeMap<String, Vec<f64>>,
}

impl Results {
    pub(crate) fn new(signals: BTreeMap<String, Vec<f64>>) -> Self {
        Self { signals }
    }
    pub fn signal(&self, name: &str) -> Option<&[f64]> {
        self.signals.get(name).map(Vec::as_slice)
    }
}
