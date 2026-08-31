use pyo3::prelude::*;

use ::kitest::{Backend, DcSupply, Ngspice, OperatingPoint};

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

#[pyclass(name = "OperatingPoint")]
struct PyOperatingPoint {
    inner: OperatingPoint,
}

#[pymethods]
impl PyOperatingPoint {
    fn node(&self, node: &str) -> Option<f64> {
        self.inner.node(node).map(|v| v.volts())
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

    fn run_op(&self, netlist: &str, supplies: Vec<PyDcSupply>) -> PyResult<PyOperatingPoint> {
        let supplies: Vec<DcSupply> = supplies.into_iter().map(|s| s.inner).collect();
        let op = self
            .inner
            .run_op(netlist, &supplies)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(PyOperatingPoint { inner: op })
    }
}

/// the `kitest` Python module.
#[pymodule]
fn kitest(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_class::<PyDcSupply>()?;
    m.add_class::<PyNgspice>()?;
    m.add_class::<PyOperatingPoint>()?;
    Ok(())
}
