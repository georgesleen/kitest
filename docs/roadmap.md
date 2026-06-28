# Roadmap

Firmware-in-loop is a near-term priority, gated only on the analog sim being
trustworthy, not deferred to a distant release.

## v0.1: analog unit-test runner (MVP)

- ngspice batch backend: run a netlist, parse results.
- Domain assertion library (`settles_to`, `overshoot`, `ripple_pp`, `gain_at`,
  `phase_margin`, `current_draw`, `no_node_exceeds`).
- `kicad-cli` export wrapper and sheet or selection boundary slicing.
- `#[sim_test]` discovery under `cargo test`.
- Scaffolding (`kitest new`) and an auto smoke test (DC convergence, no floating
  nodes, no node over abs-max).

## v0.2: MCU behavioral stub

- A scriptable pin-driver standing in for an MCU at the I/O boundary (GPIO as
  sources, ADC as node reads), no real firmware yet.

## Firmware-in-loop (once the analog sim is trusted)

- A QEMU or Renode bridge at the I/O boundary, building on ngspice `d_cosim` and
  PICSimLab patterns. Real firmware drives a simulated board.

## Toward v1.0

- Golden and snapshot testing, corners and Monte-Carlo sweeps, a KiCad IPC plugin
  frontend, an Xyce backend, and vendor model dialect import.
