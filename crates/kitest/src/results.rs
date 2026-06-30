//! Parsed output of a simulation run.

use std::collections::BTreeMap;

use num_complex::Complex64;

use crate::Signal;

/// Signals from one analysis.
#[derive(Debug)]
pub struct Results {
    data: Data,
}

#[derive(Debug)]
enum Data {
    Real(BTreeMap<String, Vec<f64>>),
    Complex(BTreeMap<String, Vec<Complex64>>),
}

impl Results {
    pub(crate) fn real(signals: BTreeMap<String, Vec<f64>>) -> Self {
        Self {
            data: Data::Real(signals),
        }
    }

    pub(crate) fn complex(signals: BTreeMap<String, Vec<Complex64>>) -> Self {
        Self {
            data: Data::Complex(signals),
        }
    }

    pub fn signal(&self, name: &str) -> Option<&[f64]> {
        match &self.data {
            Data::Real(m) => m.get(name).map(Vec::as_slice),
            Data::Complex(_) => None,
        }
    }

    pub fn spectrum(&self, name: &str) -> Option<&[Complex64]> {
        match &self.data {
            Data::Complex(m) => m.get(name).map(Vec::as_slice),
            Data::Real(_) => None,
        }
    }

    pub fn node(&self, name: &str) -> Option<Signal<'_>> {
        let key = format!("v({})", name.to_lowercase());
        Some(Signal::new(self.signal("time")?, self.signal(&key)?))
    }
}
