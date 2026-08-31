use pyo3::prelude::*;

use ::kitest::{
    Backend, DcSupply, Ngspice, OperatingPoint, Tolerance, Voltage,
};

/// Returns the kitest version string.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pyclass(name = "DcSupply", from_py_object)]
#[derive(Clone)]
struct PyDcSupply {
    inner: DcSupply,
}

#[pymethods]
impl PyDcSupply {
    #[new]
    fn new(node: &str, volts: f64) -> Self {
        Self {
            inner: DcSupply::new(node, volts),
        }
    }
}

#[pyclass(name = "Tolerance", from_py_object)]
#[derive(Clone)]
struct PyTolerance {
    inner: Tolerance,
}

#[pymethods]
impl PyTolerance {
    /// An absolute tolerance of `v`.
    #[staticmethod]
    fn abs(v: f64) -> Self {
        Self {
            inner: Tolerance::abs(v),
        }
    }

    /// A tolerance of `p` percent of the target.
    #[staticmethod]
    fn pct(p: f64) -> Self {
        Self {
            inner: Tolerance::pct(p),
        }
    }
}

#[pyclass(name = "Voltage")]
struct PyVoltage {
    inner: Voltage,
}

#[pymethods]
impl PyVoltage {
    /// The value in volts.
    fn volts(&self) -> f64 {
        self.inner.volts()
    }

    /// True if the voltage is within `tolerance` of `expected`.
    fn near(&self, expected: f64, tolerance: PyTolerance) -> bool {
        self.inner.near(expected, tolerance.inner)
    }
}

#[pyclass(name = "OperatingPoint")]
struct PyOperatingPoint {
    inner: OperatingPoint,
}

#[pymethods]
impl PyOperatingPoint {
    fn node(&self, node: &str) -> Option<PyVoltage> {
        self.inner.node(node).map(|v| PyVoltage { inner: v })
    }
}

#[pyclass(name = "Ngspice")]
struct PyNgspice {
    inner: Ngspice,
}

#[pymethods]
impl PyNgspice {
    #[new]
    fn new() -> Self {
        Self {
            inner: Ngspice::default(),
        }
    }

    fn run_op(
        &self,
        netlist: &str,
        supplies: Vec<PyDcSupply>,
    ) -> PyResult<PyOperatingPoint> {
        let supplies: Vec<DcSupply> =
            supplies.into_iter().map(|s| s.inner).collect();
        let op = self.inner.run_op(netlist, &supplies).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
        })?;
        Ok(PyOperatingPoint { inner: op })
    }
}

/// The compiled extension; the kitest package re-exports it.
#[pymodule]
fn _kitest(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_class::<PyDcSupply>()?;
    m.add_class::<PyNgspice>()?;
    m.add_class::<PyOperatingPoint>()?;
    m.add_class::<PyVoltage>()?;
    m.add_class::<PyTolerance>()?;
    Ok(())
}
