use std::collections::BTreeMap;
use std::path::Path;

use num_complex::Complex64;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::{
    gen_stub_pyclass, gen_stub_pyclass_enum, gen_stub_pyfunction,
    gen_stub_pymethods,
};

use ::kitest::{
    Ac, AcSupply, Backend, Capture, Check, Config, Corner, DcSupply, Design,
    Frequency, ModelLibrary, Ngspice, OperatingPoint, Outcome, Power, Probe,
    Pulse, Rail, RailKind, Response, Signal, Sin, Spectra, Sweep, Tolerance,
    Tone, Tran, TranSource, Transient, Voltage, VoltageOrigin,
};

/// Returns the kitest version string.
#[gen_stub_pyfunction(module = "kitest._kitest")]
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Read the KiCad schematic at `sch` as kitest sees it.
#[gen_stub_pyfunction(module = "kitest._kitest")]
#[pyfunction]
fn export_design(sch: &str) -> PyResult<PyDesign> {
    ::kitest::export_design(Path::new(sch))
        .map(|inner| PyDesign { inner })
        .map_err(raise::<KicadError>)
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Design")]
struct PyDesign {
    inner: Design,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyDesign {
    /// Net names created by power symbols, sorted.
    fn rails(&self) -> Vec<String> {
        self.inner.rails.clone()
    }

    /// The SPICE netlist, binding models from the library files at
    /// `libraries` first and the bundled library after them.
    #[pyo3(signature = (libraries = Vec::new()))]
    fn netlist(&self, libraries: Vec<String>) -> PyResult<PyNetlist> {
        let mut loaded = libraries
            .iter()
            .map(|path| ModelLibrary::load(Path::new(path)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(raise::<ModelError>)?;
        loaded.push(ModelLibrary::bundled());
        let netlist =
            self.inner.netlist(&loaded).map_err(raise::<NetlistError>)?;
        Ok(PyNetlist {
            text: netlist.text,
            defaulted: netlist.defaulted,
        })
    }

    /// Every probe in the schematic, in component order.
    fn probes(&self) -> PyResult<Vec<PyProbe>> {
        let probes = self.inner.probes().map_err(raise::<ProbeError>)?;
        Ok(probes.iter().map(PyProbe::from).collect())
    }

    /// The probe named `name`.
    fn probe(&self, name: &str) -> PyResult<PyProbe> {
        let probe = self.inner.probe(name).map_err(raise::<ProbeError>)?;
        Ok(PyProbe::from(&probe))
    }

    /// Resolve power rails against voltages declared by full net name, as
    /// in `kitest.toml`'s `[supplies]`.
    #[pyo3(signature = (supplies = BTreeMap::new()))]
    fn power(&self, supplies: BTreeMap<String, Vec<f64>>) -> PyResult<PyPower> {
        let inner =
            self.inner.power(&supplies).map_err(raise::<SupplyError>)?;
        Ok(PyPower { inner })
    }
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Config")]
struct PyConfig {
    inner: Config,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyConfig {
    /// Load `kitest.toml` from `directory`, or an empty config if it has none.
    #[staticmethod]
    fn for_project(directory: &str) -> PyResult<Self> {
        let inner = Config::for_project(Path::new(directory))
            .map_err(raise::<ConfigError>)?;
        Ok(Self { inner })
    }

    /// Load the config file at `path`.
    #[staticmethod]
    fn load(path: &str) -> PyResult<Self> {
        let inner =
            Config::load(Path::new(path)).map_err(raise::<ConfigError>)?;
        Ok(Self { inner })
    }

    /// Voltages for each rail by full net name.
    fn supplies(&self) -> BTreeMap<String, Vec<f64>> {
        self.inner.supplies.clone()
    }

    /// Project model library paths, resolved against the config's directory.
    fn model_libraries(&self) -> Vec<String> {
        self.inner
            .model_libraries
            .iter()
            .map(|path| path.display().to_string())
            .collect()
    }
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Power")]
struct PyPower {
    inner: Power,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyPower {
    /// Every resolved rail, sorted by full net name.
    fn rails(&self) -> Vec<PyRail> {
        self.inner.rails().iter().map(PyRail::from).collect()
    }

    /// Every combination of sourced rail voltages.
    fn corners(&self) -> Vec<PyCorner> {
        self.inner
            .corners()
            .map(|corner| PyCorner::from(&corner))
            .collect()
    }
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Rail")]
struct PyRail {
    net: String,
    kind: &'static str,
    voltages: Vec<f64>,
    origin: Option<&'static str>,
    driven_by: Vec<String>,
}

impl From<&Rail> for PyRail {
    fn from(rail: &Rail) -> Self {
        let (kind, voltages, origin, driven_by) = match rail.kind() {
            RailKind::Driven { by } => ("driven", Vec::new(), None, by.clone()),
            RailKind::Source { voltages, origin } => {
                let origin = match origin {
                    VoltageOrigin::Declared => "declared",
                    VoltageOrigin::Inferred => "inferred",
                };
                ("source", voltages.clone(), Some(origin), Vec::new())
            }
            RailKind::Ground => ("ground", Vec::new(), None, Vec::new()),
        };
        Self {
            net: rail.net().to_owned(),
            kind,
            voltages,
            origin,
            driven_by,
        }
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl PyRail {
    /// The rail's full net name.
    fn net(&self) -> String {
        self.net.clone()
    }

    /// How the rail is powered: `"driven"`, `"source"` or `"ground"`.
    #[gen_stub(override_return_type(
        type_repr = "typing.Literal['driven', 'source', 'ground']",
        imports = ("typing")
    ))]
    fn kind(&self) -> &'static str {
        self.kind
    }

    /// One voltage per corner for a source rail; empty otherwise.
    fn voltages(&self) -> Vec<f64> {
        self.voltages.clone()
    }

    /// `"declared"` or `"inferred"` for a source rail; `None` otherwise.
    #[gen_stub(override_return_type(
        type_repr = "typing.Literal['declared', 'inferred'] | None",
        imports = ("typing")
    ))]
    fn origin(&self) -> Option<&'static str> {
        self.origin
    }

    /// The pins driving a driven rail; empty otherwise.
    fn driven_by(&self) -> Vec<String> {
        self.driven_by.clone()
    }
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Corner")]
struct PyCorner {
    voltages: Vec<(String, f64)>,
    dc_supplies: Vec<DcSupply>,
    tran_sources: Vec<TranSource>,
    ac_supplies: Vec<AcSupply>,
}

impl From<&Corner<'_>> for PyCorner {
    fn from(corner: &Corner<'_>) -> Self {
        Self {
            voltages: corner
                .voltages()
                .map(|(node, volts)| (node.to_owned(), volts))
                .collect(),
            dc_supplies: corner.dc_supplies(),
            tran_sources: corner.tran_sources(),
            ac_supplies: corner.ac_supplies(),
        }
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl PyCorner {
    /// The SPICE node and voltage of every source kitest adds.
    fn voltages(&self) -> Vec<(String, f64)> {
        self.voltages.clone()
    }

    /// Supplies for `Ngspice.run_op`.
    fn dc_supplies(&self) -> Vec<PyDcSupply> {
        self.dc_supplies
            .iter()
            .map(|inner| PyDcSupply {
                inner: inner.clone(),
            })
            .collect()
    }

    /// Constant sources for `Ngspice.run_tran`.
    fn tran_sources(&self) -> Vec<PyTranSource> {
        self.tran_sources
            .iter()
            .map(|inner| PyTranSource {
                inner: inner.clone(),
            })
            .collect()
    }

    /// Biased supplies with no AC stimulus, for `Ngspice.run_ac`.
    fn ac_supplies(&self) -> Vec<PyAcSupply> {
        self.ac_supplies
            .iter()
            .map(|inner| PyAcSupply {
                inner: inner.clone(),
            })
            .collect()
    }
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Netlist")]
struct PyNetlist {
    text: String,
    defaulted: Vec<String>,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyNetlist {
    /// The netlist body to pass to `Ngspice`.
    fn text(&self) -> String {
        self.text.clone()
    }

    /// Parts simulated on a default model because no library covers them.
    fn defaulted(&self) -> Vec<String> {
        self.defaulted.clone()
    }
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Probe")]
struct PyProbe {
    name: String,
    net: String,
    expect: Option<String>,
    reference: String,
}

impl From<&Probe<'_>> for PyProbe {
    fn from(probe: &Probe<'_>) -> Self {
        Self {
            name: probe.name().to_owned(),
            net: probe.net().name.clone(),
            expect: probe.expect().map(str::to_owned),
            reference: probe.reference().to_owned(),
        }
    }
}

#[gen_stub_pymethods]
#[pymethods]
impl PyProbe {
    /// The probe's Value, or its net's name when Value is empty.
    fn name(&self) -> String {
        self.name.clone()
    }

    /// The name of the net the probe sits on.
    fn net(&self) -> String {
        self.net.clone()
    }

    /// The probe's Expect field, if it has one.
    fn expect(&self) -> Option<String> {
        self.expect.clone()
    }

    /// The probe's reference designator, such as `PRB1`.
    fn reference(&self) -> String {
        self.reference.clone()
    }
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "DcSupply", from_py_object)]
#[derive(Clone)]
struct PyDcSupply {
    inner: DcSupply,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyDcSupply {
    #[new]
    fn new(node: &str, volts: f64) -> Self {
        Self {
            inner: DcSupply::new(node, volts),
        }
    }
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Tolerance", from_py_object)]
#[derive(Clone)]
struct PyTolerance {
    inner: Tolerance,
}

#[gen_stub_pymethods]
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

    fn __repr__(&self) -> String {
        match self.inner {
            Tolerance::Abs(v) => format!("Tolerance.abs({v})"),
            Tolerance::Percent(p) => format!("Tolerance.percent({p})"),
        }
    }
}

/// The outcome of an assertion: truthy when it passed, and printed as what
/// was measured against what was expected.
#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Check")]
struct PyCheck {
    inner: Check,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyCheck {
    fn __bool__(&self) -> bool {
        self.inner.passed()
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        let outcome = if self.inner.passed() { "pass" } else { "fail" };
        format!("Check({outcome}: {})", self.inner)
    }
}

/// A scope capture, with expectations explicitly attached before it is saved.
#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Capture")]
struct PyCapture {
    inner: Capture,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyCapture {
    /// Attach `check` to `trace`.
    fn expect(&mut self, trace: &str, check: &PyCheck) -> PyResult<()> {
        if !self.inner.trace_names().contains(&trace) {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "capture has no trace {trace}"
            )));
        }
        let Some(expectation) = check.inner.expectation(trace) else {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "check has no region to draw",
            ));
        };
        self.inner.expectations.push(expectation);
        Ok(())
    }

    /// Save the capture as versioned JSON.
    fn save(&self, path: &str) -> PyResult<()> {
        self.inner.save(Path::new(path)).map_err(|error| {
            pyo3::exceptions::PyOSError::new_err(error.to_string())
        })
    }
}

/// Check every probe in the KiCad project at `project`, a directory or a
/// `.kicad_sch` file, against its `Expect` field.
#[gen_stub_pyfunction(module = "kitest._kitest")]
#[pyfunction]
fn check_project(project: &str) -> PyResult<PyReport> {
    let report = ::kitest::check_project(Path::new(project))
        .map_err(raise::<CheckError>)?;
    Ok(PyReport {
        outcomes: report.outcomes,
    })
}

/// Every probe's outcome at every corner.
#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Report")]
struct PyReport {
    outcomes: Vec<Outcome>,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyReport {
    /// Each probe's outcome, in probe then corner order.
    fn outcomes(&self) -> Vec<PyOutcome> {
        self.outcomes
            .iter()
            .map(|inner| PyOutcome {
                inner: inner.clone(),
            })
            .collect()
    }

    /// Whether no check failed.
    fn __bool__(&self) -> bool {
        self.outcomes.iter().all(Outcome::passed)
    }
}

/// One probe's result at one corner.
#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Outcome")]
struct PyOutcome {
    inner: Outcome,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyOutcome {
    /// The probe's name.
    fn probe(&self) -> String {
        self.inner.probe.clone()
    }

    /// The probe's reference designator.
    fn reference(&self) -> String {
        self.inner.reference.clone()
    }

    /// The net the probe sits on.
    fn net(&self) -> String {
        self.inner.net.clone()
    }

    /// The voltage of each rail kitest sourced, by SPICE node.
    fn corner(&self) -> Vec<(String, f64)> {
        self.inner.corner.clone()
    }

    /// The check, or `None` for a probe with no `Expect`.
    fn check(&self) -> Option<PyCheck> {
        self.inner.check.clone().map(|inner| PyCheck { inner })
    }

    /// For a failed check, what the corner's operating point says about it:
    /// the probe net's bias, each transistor's bias, and undriven supplies.
    fn diagnosis(&self) -> Vec<String> {
        self.inner.diagnosis.clone()
    }

    /// False only for a check that ran and failed.
    fn __bool__(&self) -> bool {
        self.inner.passed()
    }
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Frequency")]
struct PyFrequency {
    inner: Frequency,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyFrequency {
    /// The value in hertz.
    fn hertz(&self) -> f64 {
        self.inner.hertz()
    }

    /// Whether the frequency is within `tolerance` of `expected`.
    fn near(&self, expected: f64, tolerance: PyTolerance) -> PyCheck {
        PyCheck {
            inner: Check::near(
                self.inner.hertz(),
                expected,
                tolerance.inner,
                "Hz",
            ),
        }
    }

    fn __repr__(&self) -> String {
        format!("Frequency({})", self.inner)
    }
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Tone")]
struct PyTone {
    inner: Tone,
}

#[gen_stub_pymethods]
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

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Voltage")]
struct PyVoltage {
    inner: Voltage,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyVoltage {
    /// The value in volts.
    fn volts(&self) -> f64 {
        self.inner.volts()
    }

    /// Whether the voltage is within `tolerance` of `expected`.
    fn near(&self, expected: f64, tolerance: PyTolerance) -> PyCheck {
        PyCheck {
            inner: Check::near(
                self.inner.volts(),
                expected,
                tolerance.inner,
                "V",
            ),
        }
    }

    fn __repr__(&self) -> String {
        format!("Voltage({})", self.inner)
    }
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "OperatingPoint")]
struct PyOperatingPoint {
    inner: OperatingPoint,
}

#[gen_stub_pymethods]
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

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Ngspice")]
struct PyNgspice {
    inner: Ngspice,
}

#[gen_stub_pymethods]
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
            .map_err(raise::<SimulationError>)?;
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
            .map_err(raise::<SimulationError>)?;
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
            .map_err(raise::<SimulationError>)?;
        Ok(PyTransient { inner: transient })
    }
}

/// A Python exception in `kitest._kitest` whose stub names its base by module.
macro_rules! exception {
    ($name:ident, $base:ty, $doc:expr) => {
        pyo3::create_exception!(kitest._kitest, $name, $base, $doc);

        impl pyo3_stub_gen::PyStubType for $name {
            fn type_output() -> pyo3_stub_gen::TypeInfo {
                pyo3_stub_gen::TypeInfo::locally_defined(
                    stringify!($name),
                    "kitest._kitest".into(),
                )
            }
        }

        pyo3_stub_gen::inventory::submit! {
            pyo3_stub_gen::type_info::PyClassInfo {
                pyclass_name: stringify!($name),
                struct_id: std::any::TypeId::of::<$name>,
                getters: &[],
                setters: &[],
                module: Some("kitest._kitest"),
                doc: $doc,
                bases: &[|| <$base as pyo3_stub_gen::PyStubType>::type_output()],
                has_eq: false,
                has_ord: false,
                has_hash: false,
                has_str: false,
                subclass: true,
            }
        }
    };
}

exception!(
    KitestError,
    pyo3::exceptions::PyException,
    "Base class of every error kitest raises."
);
exception!(KicadError, KitestError, "KiCad export failed.");
exception!(ConfigError, KitestError, "kitest.toml is invalid.");
exception!(ModelError, KitestError, "A model library is invalid.");
exception!(
    NetlistError,
    KitestError,
    "A design cannot be turned into a netlist."
);
exception!(SupplyError, KitestError, "Power rails cannot be resolved.");
exception!(ProbeError, KitestError, "A probe cannot be read.");
exception!(SimulationError, KitestError, "The simulator failed.");
exception!(
    CheckError,
    KitestError,
    "A project's probes cannot be checked: a bad Expect field, or a failure \
     reading or simulating the design."
);

/// Raise `error` as exception `E`, with every underlying cause appended.
fn raise<E: pyo3::PyTypeInfo>(error: impl std::error::Error) -> PyErr {
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(source) = cause {
        message.push_str("\ncaused by: ");
        message.push_str(&source.to_string());
        cause = source.source();
    }
    PyErr::new::<E, _>(message)
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

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "AcSupply", from_py_object)]
#[derive(Clone)]
struct PyAcSupply {
    inner: AcSupply,
}

#[gen_stub_pymethods]
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

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Pulse", from_py_object)]
#[derive(Clone)]
struct PyPulse {
    inner: Pulse,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyPulse {
    #[staticmethod]
    #[pyo3(signature = (*, low, high))]
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

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Sin", from_py_object)]
#[derive(Clone)]
struct PySin {
    inner: Sin,
}

#[gen_stub_pymethods]
#[pymethods]
impl PySin {
    #[new]
    #[pyo3(signature = (*, offset, amplitude, freq))]
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

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "TranSource", from_py_object)]
#[derive(Clone)]
struct PyTranSource {
    inner: TranSource,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyTranSource {
    /// Hold `node` at a constant `volts`.
    #[staticmethod]
    fn dc(node: &str, volts: f64) -> Self {
        Self {
            inner: TranSource::dc(node, volts),
        }
    }

    /// Hold `node` at `volts`, with `noise` volts RMS of supply noise.
    #[staticmethod]
    #[pyo3(signature = (node, volts, *, noise))]
    fn noisy_dc(node: &str, volts: f64, noise: f64) -> Self {
        Self {
            inner: TranSource::noisy_dc(node, volts, noise),
        }
    }

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

    /// Inject a brief current impulse of `amps` into `node`, to start an
    /// oscillator without moving its operating point.
    #[staticmethod]
    fn kick(node: &str, amps: f64) -> Self {
        Self {
            inner: TranSource::kick(node, amps),
        }
    }
}

#[gen_stub_pyclass_enum]
#[pyclass(
    module = "kitest._kitest",
    name = "Sweep",
    eq,
    eq_int,
    from_py_object
)]
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

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Ac", from_py_object)]
#[derive(Clone)]
struct PyAc {
    inner: Ac,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyAc {
    #[new]
    #[pyo3(signature = (sweep, *, points, fstart, fstop))]
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

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Tran", from_py_object)]
#[derive(Clone)]
struct PyTran {
    inner: Tran,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyTran {
    #[new]
    #[pyo3(signature = (*, step, stop))]
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

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Signal")]
struct PySignal {
    time: Vec<f64>,
    values: Vec<f64>,
}

#[gen_stub_pymethods]
#[pymethods]
impl PySignal {
    fn time(&self) -> Vec<f64> {
        self.time.clone()
    }

    fn values(&self) -> Vec<f64> {
        self.values.clone()
    }

    /// Whether the signal stays within `tolerance` of `target` over the last
    /// `window` seconds.
    #[pyo3(signature = (target, tolerance, *, window))]
    fn settles_to(
        &self,
        target: f64,
        tolerance: PyTolerance,
        window: f64,
    ) -> PyCheck {
        let signal = Signal::new(&self.time, &self.values);
        PyCheck {
            inner: Check::settles(&signal, target, tolerance.inner, window),
        }
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

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Response")]
struct PyResponse {
    frequency: Vec<f64>,
    values: Vec<Complex64>,
}

#[gen_stub_pymethods]
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

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Transient")]
struct PyTransient {
    inner: Transient,
}

#[gen_stub_pymethods]
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

    /// Every node's waveform as a scope capture named `name`.
    fn capture(&self, name: &str) -> PyCapture {
        PyCapture {
            inner: self.inner.capture(name),
        }
    }
}

#[gen_stub_pyclass]
#[pyclass(module = "kitest._kitest", name = "Spectra")]
struct PySpectra {
    inner: Spectra,
}

#[gen_stub_pymethods]
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

    /// Every node's response as a scope capture named `name`.
    fn capture(&self, name: &str) -> PyCapture {
        PyCapture {
            inner: self.inner.capture(name),
        }
    }
}

/// The compiled extension; the kitest package re-exports it.
#[pymodule]
fn _kitest(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(export_design, m)?)?;
    m.add_function(wrap_pyfunction!(check_project, m)?)?;
    m.add_class::<PyReport>()?;
    m.add_class::<PyOutcome>()?;
    m.add_class::<PyDesign>()?;
    m.add_class::<PyNetlist>()?;
    m.add_class::<PyProbe>()?;
    m.add_class::<PyConfig>()?;
    m.add_class::<PyPower>()?;
    m.add_class::<PyRail>()?;
    m.add_class::<PyCorner>()?;
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
    m.add_class::<PyCapture>()?;
    m.add_class::<PyVoltage>()?;
    m.add_class::<PyFrequency>()?;
    m.add_class::<PyTone>()?;
    m.add_class::<PySignal>()?;
    m.add_class::<PyResponse>()?;
    m.add_class::<PyTolerance>()?;
    m.add_class::<PyCheck>()?;
    let py = m.py();
    m.add("KitestError", py.get_type::<KitestError>())?;
    m.add("KicadError", py.get_type::<KicadError>())?;
    m.add("ConfigError", py.get_type::<ConfigError>())?;
    m.add("ModelError", py.get_type::<ModelError>())?;
    m.add("NetlistError", py.get_type::<NetlistError>())?;
    m.add("SupplyError", py.get_type::<SupplyError>())?;
    m.add("ProbeError", py.get_type::<ProbeError>())?;
    m.add("SimulationError", py.get_type::<SimulationError>())?;
    m.add("CheckError", py.get_type::<CheckError>())?;
    Ok(())
}

pyo3_stub_gen::define_stub_info_gatherer!(stub_info);
