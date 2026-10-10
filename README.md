# KiTest
Simulation-based tests for KiCad designs, with a pure Rust engine and Python
bindings.

- [Probe Expect fields](docs/expect.md): DC and oscillation checks, tolerances,
  swing and harmonic-distortion limits, and simulation limits.
- [Scope reference](docs/scope.md): Rust/Python captures, JSON v2, waveform,
  spectrum and Bode views, controls, measurements, exports, and live commands.
- [Pending scope designs](docs/scope-next.md): proposals, not implemented APIs.
- [Architecture](docs/architecture.md)
- [Roadmap](docs/roadmap.md)

`kitest --show PROBE [PROJECT]` runs checks and opens the probe's waveform in
`kitest-scope`; `kitest --sketch PROBE [PROJECT]` prints it in the terminal.
Open a saved capture directly with `kitest-scope FILE`.
