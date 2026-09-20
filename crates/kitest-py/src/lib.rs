use num_complex::Complex64;
use pyo3::prelude::*;

use ::kitest::{
    Ac, AcSupply, Backend, DcSupply, Frequency, Ngspice, OperatingPoint,
    Pulse, Response, Signal, Sin, Spectra, Sweep, Tolerance, Tone, Tran,
    TranSource, Transient, Voltage,
};

/// Returns the kitest version string.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Export a KiCad schematic to a SPICE netlist body.
#[pyfunction]
fn export_netlist(sch: &str) -> PyResult<String> {
    ::kitest::export_netlist(std::path::Path::new(sch)).map_err(runtime_error)
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
    fn percent(p: f64) -> Self {
        Self {
            inner: Tolerance::percent(p),
        }
    }
}

#[pyclass(name = "Frequency")]
struct PyFrequency {
    inner: Frequency,
}

#[pymethods]
impl PyFrequency {
    /// The value in hertz.
    fn hertz(&self) -> f64 {
        self.inner.hertz()
    }

    /// True if the frequency is within `tolerance` of `expected`.
    fn near(&self, expected: f64, tolerance: PyTolerance) -> bool {
        self.inner.near(expected, tolerance.inner)
    }
}

#[pyclass(name = "Tone")]
struct PyTone {
    inner: Tone,
}

#[pymethods]
impl PyTone {
    /// The frequency of the dominant sinusoid.
    fn frequency(&self) -> PyFrequency {
        PyFrequency {
            inner: self.inner.frequency(),
        }
    }

    /// The amplitude of the dominant sinusoid, zero to peak.
    fn amplitude(&self) -> PyVoltage {
        PyVoltage {
            inner: self.inner.amplitude(),
        }
    }

    /// Simulator samples per cycle of the dominant sinusoid.
    fn samples_per_cycle(&self) -> f64 {
        self.inner.samples_per_cycle()
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
    fn node(&self, node: &str) -> PyResult<PyVoltage> {
        self.inner
            .node(node)
            .map(|v| PyVoltage { inner: v })
            .ok_or_else(|| missing_node(node, &self.inner.nodes()))
    }

    fn nodes(&self) -> Vec<String> {
        self.inner.nodes().into_iter().map(String::from).collect()
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
        let op = self
            .inner
            .run_op(netlist, &supplies)
            .map_err(runtime_error)?;
        Ok(PyOperatingPoint { inner: op })
    }

    fn run_ac(
        &self,
        netlist: &str,
        supplies: Vec<PyAcSupply>,
        params: PyAc,
    ) -> PyResult<PySpectra> {
        let supplies: Vec<AcSupply> =
            supplies.into_iter().map(|s| s.inner).collect();
        let spectra = self
            .inner
            .run_ac(netlist, &supplies, params.inner)
            .map_err(runtime_error)?;
        Ok(PySpectra { inner: spectra })
    }

    fn run_tran(
        &self,
        netlist: &str,
        sources: Vec<PyTranSource>,
        params: PyTran,
    ) -> PyResult<PyTransient> {
        let sources: Vec<TranSource> =
            sources.into_iter().map(|s| s.inner).collect();
        let transient = self
            .inner
            .run_tran(netlist, &sources, params.inner)
            .map_err(runtime_error)?;
        Ok(PyTransient { inner: transient })
    }
}

fn runtime_error(e: impl std::fmt::Display) -> PyErr {
    pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
}

fn missing_node(node: &str, known: &[&str]) -> PyErr {
    pyo3::exceptions::PyKeyError::new_err(format!(
        "no node {node:?}; known: {}",
        known.join(", ")
    ))
}

fn empty_response() -> PyErr {
    pyo3::exceptions::PyValueError::new_err("response has no frequency points")
}

#[pyclass(name = "AcSupply", from_py_object)]
#[derive(Clone)]
struct PyAcSupply {
    inner: AcSupply,
}

#[pymethods]
impl PyAcSupply {
    #[new]
    fn new(node: &str) -> Self {
        Self {
            inner: AcSupply::new(node),
        }
    }

    fn bias(&self, bias: f64) -> Self {
        Self {
            inner: self.inner.clone().bias(bias),
        }
    }

    fn magnitude(&self, magnitude: f64) -> Self {
        Self {
            inner: self.inner.clone().magnitude(magnitude),
        }
    }
}

#[pyclass(name = "Pulse", from_py_object)]
#[derive(Clone)]
struct PyPulse {
    inner: Pulse,
}

#[pymethods]
impl PyPulse {
    #[staticmethod]
    fn step(low: f64, high: f64) -> Self {
        Self {
            inner: Pulse::step(low, high),
        }
    }

    fn delay(&self, delay: f64) -> Self {
        Self {
            inner: self.inner.clone().delay(delay),
        }
    }

    fn rise(&self, rise: f64) -> Self {
        Self {
            inner: self.inner.clone().rise(rise),
        }
    }

    fn fall(&self, fall: f64) -> Self {
        Self {
            inner: self.inner.clone().fall(fall),
        }
    }

    fn width(&self, width: f64) -> Self {
        Self {
            inner: self.inner.clone().width(width),
        }
    }

    fn period(&self, period: f64) -> Self {
        Self {
            inner: self.inner.clone().period(period),
        }
    }
}

#[pyclass(name = "Sin", from_py_object)]
#[derive(Clone)]
struct PySin {
    inner: Sin,
}

#[pymethods]
impl PySin {
    #[new]
    fn new(offset: f64, amplitude: f64, freq: f64) -> Self {
        Self {
            inner: Sin::new(offset, amplitude, freq),
        }
    }

    fn delay(&self, delay: f64) -> Self {
        Self {
            inner: self.inner.clone().delay(delay),
        }
    }
}

#[pyclass(name = "TranSource", from_py_object)]
#[derive(Clone)]
struct PyTranSource {
    inner: TranSource,
}

#[pymethods]
impl PyTranSource {
    #[staticmethod]
    fn pulse(node: &str, pulse: PyPulse) -> Self {
        Self {
            inner: TranSource::pulse(node, pulse.inner),
        }
    }

    #[staticmethod]
    fn sin(node: &str, sin: PySin) -> Self {
        Self {
            inner: TranSource::sin(node, sin.inner),
        }
    }
}

#[pyclass(name = "Sweep", eq, eq_int, from_py_object)]
#[derive(Clone, PartialEq)]
enum PySweep {
    Dec,
    Oct,
    Lin,
}

impl PySweep {
    fn to_kitest(&self) -> Sweep {
        match self {
            PySweep::Dec => Sweep::Dec,
            PySweep::Oct => Sweep::Oct,
            PySweep::Lin => Sweep::Lin,
        }
    }
}

#[pyclass(name = "Ac", from_py_object)]
#[derive(Clone)]
struct PyAc {
    inner: Ac,
}

#[pymethods]
impl PyAc {
    #[new]
    fn new(sweep: PySweep, points: u32, fstart: f64, fstop: f64) -> Self {
        Self {
            inner: Ac {
                sweep: sweep.to_kitest(),
                points,
                fstart,
                fstop,
            },
        }
    }
}

#[pyclass(name = "Tran", from_py_object)]
#[derive(Clone)]
struct PyTran {
    inner: Tran,
}

#[pymethods]
impl PyTran {
    #[new]
    fn new(step: f64, stop: f64) -> Self {
        Self {
            inner: Tran::new(step, stop),
        }
    }

    /// Discard output before `start` seconds.
    fn start(&self, start: f64) -> Self {
        Self {
            inner: self.inner.start(start),
        }
    }
}

#[pyclass(name = "Signal")]
struct PySignal {
    time: Vec<f64>,
    values: Vec<f64>,
}

#[pymethods]
impl PySignal {
    fn time(&self) -> Vec<f64> {
        self.time.clone()
    }

    fn values(&self) -> Vec<f64> {
        self.values.clone()
    }

    fn settles_to(
        &self,
        target: f64,
        tolerance: PyTolerance,
        window: f64,
    ) -> bool {
        Signal::new(&self.time, &self.values).settles_to(
            target,
            tolerance.inner,
            window,
        )
    }

    fn overshoot(&self, target: f64) -> f64 {
        Signal::new(&self.time, &self.values).overshoot(target)
    }

    /// The strongest sinusoid in this waveform.
    fn dominant_tone(&self) -> PyTone {
        PyTone {
            inner: Signal::new(&self.time, &self.values).dominant_tone(),
        }
    }
}

#[pyclass(name = "Response")]
struct PyResponse {
    frequency: Vec<f64>,
    values: Vec<Complex64>,
}

#[pymethods]
impl PyResponse {
    fn frequency(&self) -> Vec<f64> {
        self.frequency.clone()
    }

    fn values(&self) -> Vec<Complex64> {
        self.values.clone()
    }

    fn gain_db_at(&self, frequency: f64) -> PyResult<f64> {
        Response::new(&self.frequency, &self.values)
            .gain_db_at(frequency)
            .ok_or_else(empty_response)
    }

    fn phase_deg_at(&self, frequency: f64) -> PyResult<f64> {
        Response::new(&self.frequency, &self.values)
            .phase_deg_at(frequency)
            .ok_or_else(empty_response)
    }
}

#[pyclass(name = "Transient")]
struct PyTransient {
    inner: Transient,
}

#[pymethods]
impl PyTransient {
    fn node(&self, node: &str) -> PyResult<PySignal> {
        self.inner
            .node(node)
            .map(|s| PySignal {
                time: s.time().to_vec(),
                values: s.values().to_vec(),
            })
            .ok_or_else(|| missing_node(node, &self.inner.nodes()))
    }

    fn nodes(&self) -> Vec<String> {
        self.inner.nodes().into_iter().map(String::from).collect()
    }
}

#[pyclass(name = "Spectra")]
struct PySpectra {
    inner: Spectra,
}

#[pymethods]
impl PySpectra {
    fn node(&self, node: &str) -> PyResult<PyResponse> {
        self.inner
            .node(node)
            .map(|r| PyResponse {
                frequency: r.frequency().to_vec(),
                values: r.values().to_vec(),
            })
            .ok_or_else(|| missing_node(node, &self.inner.nodes()))
    }

    fn nodes(&self) -> Vec<String> {
        self.inner.nodes().into_iter().map(String::from).collect()
    }
}

/// The compiled extension; the kitest package re-exports it.
#[pymodule]
fn _kitest(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(export_netlist, m)?)?;
    m.add_class::<PyDcSupply>()?;
    m.add_class::<PyAcSupply>()?;
    m.add_class::<PyTranSource>()?;
    m.add_class::<PyPulse>()?;
    m.add_class::<PySin>()?;
    m.add_class::<PySweep>()?;
    m.add_class::<PyAc>()?;
    m.add_class::<PyTran>()?;
    m.add_class::<PyNgspice>()?;
    m.add_class::<PyOperatingPoint>()?;
    m.add_class::<PyTransient>()?;
    m.add_class::<PySpectra>()?;
    m.add_class::<PyVoltage>()?;
    m.add_class::<PyFrequency>()?;
    m.add_class::<PyTone>()?;
    m.add_class::<PySignal>()?;
    m.add_class::<PyResponse>()?;
    m.add_class::<PyTolerance>()?;
    Ok(())
}
