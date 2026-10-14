# kitest architecture

kitest makes simulation-based testing a low-friction part of KiCad design. It is a
Rust engine, exposed to Python, that drives `kicad-cli` and a SPICE engine, runs
headless, and reports pass/fail, so a circuit's intended behavior can be checked
as easily as a software unit test.

## North star

The goal is to **deduplicate work**. Today the design lives in the schematic and
the intent of a test (stimulus, supplies, what "correct" means) lives nowhere
durable, so every iteration you rebuild it by hand in two places. kitest makes the
test a persistent, one-command re-runnable artifact next to the design that
survives schematic edits. Regression testing and CI fall out of that for free;
they are a byproduct, not the pitch.

Framed against KiCad's existing checks: **DRC checks manufacturability, ERC checks
wiring, kitest checks intent.** It is executable design intent.

## Principles

- **Deduplicate the mechanical, preserve the oracle.** Collapse the mechanical
  duplication (port/interface re-declaration, per-iteration sim setup, an MCU
  model maintained alongside its firmware) to a single source. Keep the assertion
  hand-written and independent: a test *derived from* the design catches nothing.
- Slot into existing KiCad. Hierarchical sheets are the test unit, hierarchical
  pins are the port interface, and KiCad's own Simulation Model assignment carries
  models. No machinery is added inside the design; tests live beside the project.
- Boundaries are declared test-side. A unit under test is a component/net
  selection with an auto-derived port list (the nets crossing the cut). A sheet is
  the common case (selection is the sheet contents, ports are its hierarchical
  pins).
- Headless first. The core needs no GUI; a KiCad IPC plugin is an optional later
  frontend.
- Fidelity is explicit. Each part declares a tier: real, behavioral, ideal, or
  stub.

## Assertion tiers

Checks are layered cheapest-first, so the front door needs no SPICE knowledge and
catches the common month-wasting bugs before any modelling is involved:

1. **Static / connectivity** off the exported netlist. No SPICE, no models. "These
   nets are one," "no unconnected power pins," "this pin lands on that rail." This
   is where drift bugs like a `+1V` / `1V` mixup get caught deterministically.
2. **Smoke.** Zero-config sanity: DC convergence, no floating nodes, nothing over
   abs-max. Requires no test authoring.
3. **Behavioral simulation.** Domain assertions on waveforms (`settles_to`,
   `ripple_pp`, `gain_at`, `phase_margin`, `current_draw`). The north-star depth,
   and where the hard model problem lives.

## Testbench model

Testbenches are **Python, under pytest**, in the style of cocotb: a test function
binds to a boundary, applies stimulus and supplies, runs an analysis, and asserts
on named nodes. pytest supplies discovery, fixtures, and CI for free. The Rust
engine is exposed to Python via PyO3/maturin; results are plain data.

## Components

Single crate first, split only when forced (for example a pure publishable core,
or to enforce a boundary). Module seams inside the crate mark the eventual splits.

- `kitest` (Rust): the engine. The `Backend` trait, the ngspice batch (subprocess)
  backend, the live ngspice session, the `kicad-cli` wrapper, boundary/port
  derivation and the slicer, and the assertion primitives. An FFI backend and an
  Xyce backend can follow behind the trait.
- `kitest-py` (PyO3/maturin): the Python binding and the `import kitest` surface
  the testbenches use.

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
  be sliced by hierarchical path. (Confirming this naming is stable across KiCad
  versions is an open de-risking item.)
- Passives and sources map to native SPICE lines; ground is node `0`.
- An `.include` of `Simulation_SPICE.sp` is added for built-ins (for example
  `kicad_builtin_opamp`); its path is environment-specific, so includes need
  resolving for portable and CI runs.

## Live simulation session

`Ngspice::session` starts a transient run that the host advances, reads and
changes, for co-simulation that cannot run to completion first. It sits beside
the batch backend and does not replace it.

- **Process.** One `ngspice -p` child per session, driven over a pipe. Sessions
  share no state, so they run in parallel. The deck file is the netlist, one DC
  voltage source per declared `Drive`, and a `.tran` with an explicit maximum
  step. The child runs without `DISPLAY` and holds a pseudo-terminal as its
  controlling terminal, because ngspice with no controlling terminal and no X
  display crashes before it reads a command. Its home and working directory are
  the session tempdir, which isolates its configuration and command history.
- **Time.** Virtual and monotonic; there is no rollback. `advance_to(t)` sets
  a solver breakpoint at `t` and a separate stop condition on the time vector,
  with a relative 1e-12 tolerance. The minimum solver breakpoint spacing is
  explicitly set to 1e-10 times the maximum step, including on resume.
- **Resolution.** Two stop times must be at least a thousandth of the maximum
  step apart. A closer request is an error. A missed landing stops at the first
  point beyond the tolerance window and returns its actual time, not a run to
  the end of the analysis.
- **Events.** `set(node, volts)` alters a drive at the current time. The
  current time is a breakpoint, where ngspice drops to first-order
  integration, so the change acts as a step at that time and is not
  interpolated from the previous point. A read at the time of a change returns
  the value before it. Changes at one time apply in call order. Voltage
  accuracy just after a fast edge is bounded by the solver's local truncation
  error. An exact event timestamp does not guarantee an exact voltage; choose
  a maximum step below the fastest circuit time constant when needed.
- **Time zero.** The session solves the operating point at start, from the
  declared drive values. A drive cannot change at time zero, because ngspice
  solves the operating point again on resume.
- **Errors.** ngspice error lines and aborted runs fail the call. Model warnings
  do not fail a session, consistent with the batch backend.
- **Limits.** The run ends at the stop time given at start; a drive cannot change
  there. ngspice keeps every accepted point in memory. Its expression parser
  copies complete vectors for indexed reads, so each read and advance costs
  O(stored points), and repeated advances have quadratic total cost. A call
  blocks until ngspice answers; there is no timeout. Drives are ground-referenced
  voltage sources only.

## MCU in the loop

For mixed-signal designs the MCU is driven at its pin boundary (GPIO as sources,
ADC as node reads, PWM as a source), in two fidelity steps:

- **Behavioral pin-driver** (a Python mock): a scaffold that drives and reads pins
  on the sim timeline. It proves the co-sim boundary and time-synchronization
  against ngspice. It is deliberately throwaway, since it duplicates firmware
  logic, and is not a user-facing artifact.
- **Real firmware.** The actual compiled ELF runs on Renode (RP2040 first), and
  its peripheral accesses are bridged to the same pin boundary. Because it runs
  the compiled binary rather than parsing source, it is language- and
  RTOS-agnostic for free (C, C++, Rust, Embassy, FreeRTOS, Zephyr all lower to the
  same instructions). The hard parts are the emulator's peripheral fidelity and
  the ngspice co-sim clock bridge, not firmware ingestion.

## Missing models

Most friction in analog testing is model wrangling. kitest addresses it with
boundary isolation (everything outside the cut becomes stimulus and loads), a
parametric behavioral mock library attached through KiCad's Simulation Model
assignment, and explicit fidelity tiers with diagnostics that suggest the fix.
