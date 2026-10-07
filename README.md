# kitest

[![CI](https://github.com/georgesleen/kitest/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/georgesleen/kitest/actions/workflows/ci.yml)

kitest is a simulation-based test framework for KiCad designs. You place probe
symbols in the schematic, and each probe states what a net must do. kitest
exports the schematic with `kicad-cli`, simulates it in ngspice, and reports
which probes pass. The engine is in Rust, and a thin PyO3 binding exposes it
to Python test benches.

## Quick start

The nix dev shell supplies Rust, KiCad, ngspice, and Python.

```sh
nix develop
make test
cargo run -p kitest -- examples/kicad/colpitts
```

## Documentation

- The docs site: <https://georgesleen.com/kitest/>
- How a ticket goes from claim to merge: [docs/workflow.md](docs/workflow.md)
