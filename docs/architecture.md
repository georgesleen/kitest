# kitest architecture

kitest makes simulation-based testing a first-class part of KiCad design. It is a
standalone Rust workspace and CLI that drives `kicad-cli` and a SPICE engine, runs
headless, and reports pass/fail, so circuit behavior can be unit tested in CI the
way software is.

## Principles

- Slot into existing KiCad. Hierarchical sheets are the test unit, hierarchical
  pins are the port interface, and KiCad's own Simulation Model assignment carries
  models. No machinery is added inside the design; tests live beside the project.
- Boundaries are declared test-side. A unit under test is a component/net
  selection with an auto-derived port list (the nets crossing the cut). A sheet is
  the common case (selection is the sheet contents, ports are its hierarchical
  pins).
- Headless and CI-first. The core needs no GUI; a KiCad IPC plugin is an optional
  later frontend.
- Fidelity is explicit. Each part declares a tier: real, behavioral, ideal, or
  stub.

## Components

- `kitest-core`: domain types, the `Backend` trait, and the assertion library.
  Host-testable, no platform dependencies.
- `kitest-ngspice`: `Backend` implementations. A batch (subprocess) backend runs
  first; an FFI backend can follow.
- `kitest-kicad`: the `kicad-cli` wrapper, the boundary abstraction, port
  derivation, and the slicer.
- `kitest-macros`: the `#[sim_test]` attribute and discovery under `cargo test`.
- `kitest`: the CLI (`run`, `new`, `smoke`).

## Pipeline

A KiCad schematic is exported to a SPICE netlist by `kicad-cli`. kitest resolves
the boundary, derives its ports, slices the subcircuit, wraps it with stimulus,
supplies, and any behavioral mocks, runs an analysis through a `Backend`, and
checks the resulting waveforms with assertions.

## KiCad netlist export (the seam)

`kicad-cli sch export netlist --format spice <sheet>` emits a netlist with
properties kitest relies on:

- No analysis directive is included; the netlist ends at `.end`. kitest supplies
  the analysis (`.op`, `.tran`, `.ac`) from the testbench.
- Hierarchical sheet paths appear in node names as `/sheet/NET`, so a boundary can
  be sliced by hierarchical path.
- Passives and sources map to native SPICE lines; ground is node `0`.
- An `.include` of `Simulation_SPICE.sp` is added for built-ins (for example
  `kicad_builtin_opamp`); its path is environment-specific, so includes need
  resolving for portable and CI runs.

## Testbench model

Testbenches are Rust, in the style of cocotb: a function annotated with
`#[sim_test]` binds to a boundary, applies stimulus and supplies, runs an
analysis, and asserts on named nodes. Results are plain data, leaving room for a
declarative testbench layer over the same engine.

## Missing models

Most friction in analog testing is model wrangling. kitest addresses it with
boundary isolation (everything outside the cut becomes stimulus and loads), a
parametric behavioral mock library attached through KiCad's Simulation Model
assignment, and explicit fidelity tiers with diagnostics that suggest the fix.
