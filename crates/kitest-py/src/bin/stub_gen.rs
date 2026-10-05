//! Writes `python/kitest/_kitest.pyi` from the binding's annotations.

fn main() -> pyo3_stub_gen::Result<()> {
    kitest_py::stub_info()?.generate()
}
