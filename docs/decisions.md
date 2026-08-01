# Decisions

- North star is **deduplicating work** and reducing design-time iteration
  friction; CI/regression gating is a byproduct, not the pitch. Principle:
  **deduplicate the mechanical, preserve the oracle** (the assertion stays an
  independent, hand-written intent statement).
- **Python testbenches under pytest** (cocotb-style), not Rust. The Rust engine is
  exposed to Python via PyO3/maturin. pytest gives discovery, fixtures, and CI for
  free, so the earlier Rust proc-macro / `#[sim_test]` / libtest-mimic harness is
  dropped.
- Assertions are **tiered**: static connectivity (no SPICE) -> smoke (zero-config
  sanity) -> behavioral simulation. The cheap tiers are the low-friction front
  door and catch the common bugs.
- Rust engine as a **single crate first** (`kitest`), with `kitest-py` for the
  binding. Split further only when forced.
- Standalone headless engine and CLI, not a GUI plugin. It survives KiCad API
  churn; a KiCad IPC plugin can come later as a frontend.
- ngspice batch (subprocess) backend before FFI. Sidesteps libngspice's
  single-instance global state and is parallel-test safe. Subprocess isolation
  also keeps kitest license-independent from ngspice.
- A `Backend` trait abstracts the engine, leaving room for an FFI backend and Xyce.
- Boundary is declared test-side: a hierarchical sheet (ports are hier pins) or a
  user-defined refdes/net selection (ports are crossing nets). Nothing is added
  inside the design.
- Missing models handled by boundary isolation, behavioral mocks, and explicit
  fidelity tiers. Vendor dialect import is deferred.
- MCU-in-loop runs the **real compiled ELF on Renode** (RP2040 first), reached via
  a throwaway behavioral pin-driver scaffold that de-risks the co-sim boundary
  first. Running the binary makes it language- and RTOS-agnostic.
- **LGPL-3.0-or-later** (changed from AGPL-3.0). kitest's own code stays copyleft,
  but importing it to author testbenches or run firmware imposes nothing on those,
  so companies can use it like they use Linux. AGPL's network clause guarded a
  hosted-service case that does not apply to a local/CI tool and blocked many
  corporate users.
- Edition 2024, a Nix devshell without flake-utils.

## Early constraints (cheap now, expensive later)

- **Tolerances and units are first-class in the assertion API.** No exact-float
  comparison; every assertion takes an explicit tolerance, and quantities are
  unit-aware or unit-explicit (incl. degrees vs radians for phase). Retrofitting
  units into a public API is brutal.
- **The Python API is a versioned public contract.** Expose the minimum from Rust;
  put ergonomics in a thin Python layer iterable without recompiling; design the
  ideal testbench before freezing the binding.
- **Never fail silently.** Diagnostics are the product for users who don't know
  SPICE. Fix the `Results` wrong-domain accessors that return `None` to error
  instead.
- **Keep the boundary abstraction general.** A boundary = a selection + derived
  ports, with sheet / net-refdes selection / (future) board-connector as
  interchangeable sources, so multi-board slots in later with no rework. Full
  schematic (no cut, integration test) and sliced sheet (idealized cut, unit test)
  are both supported; full board + selective stubs is the middle ground.
- **Pin KiCad and keep golden netlist fixtures.** The `kicad-cli` export format is
  the shakier seam; pin the version, snapshot netlists, and route all parsing
  through one adapter.
- **Runs are hermetic.** Explicit `.include` resolution, temp dirs, no ambient
  config; prove one green CI run on the walking skeleton early.
- **Settle and reserve the name before publishing.** `kitest` is tentative;
  confirm it and reserve PyPI/crates.io before anything importable ships.
- **Licensing/commercial:** LGPL-3.0-or-later now; add a CLA (or a founders'
  agreement with the friend) before accepting outside contributions to preserve
  the dual-license/commercial option; trademark the name later.
