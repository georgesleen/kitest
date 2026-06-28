# Decisions

- Standalone Rust workspace and CLI, not a GUI plugin. Headless and CI-first, and
  it survives KiCad API churn; a plugin can come later as a frontend.
- ngspice batch (subprocess) backend before FFI. Sidesteps libngspice's
  single-instance global state and is parallel-test safe.
- A `Backend` trait abstracts the engine, leaving room for an FFI backend and Xyce.
- Rust-native testbenches (cocotb-style); the engine stays data-driven so a
  declarative layer can be added over it later.
- Boundary is declared test-side: a hierarchical sheet (ports are hier pins) or a
  user-defined refdes/net selection (ports are crossing nets). Nothing is added
  inside the design.
- Missing models handled by boundary isolation, behavioral mocks, and explicit
  fidelity tiers. Vendor dialect import is deferred.
- Edition 2024, AGPL-3.0, a Nix devshell without flake-utils.
