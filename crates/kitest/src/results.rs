//! Parsed output of a simulation run.

use std::collections::BTreeMap;

use crate::Signal;

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

    pub fn node(&self, name: &str) -> Option<Signal<'_>> {
        let key = format!("v({})", name.to_lowercase());
        Some(Signal::new(self.signal("time")?, self.signal(&key)?))
    }
}
