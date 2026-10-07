# kitest

kitest is a simulation-based test framework for KiCad designs. You place a
probe on a net in the schematic and write what the net must do in the
probe's `Expect` field. The `kitest` command exports the design with
`kicad-cli`, simulates it with ngspice, and reports a pass or a fail for
each probe.

This is the Colpitts example, with `oscillates(near=10.4MHz, within=5%)`
in the `Expect` field of its probe `PRB1`:

```text
$ kitest
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 10.1162 MHz is 283.8175 kHz from 10.4 MHz, within ±5% (520 kHz), at a steady 3.0443 V amplitude
1 passed, 0 failed
```

Nothing SPICE-specific goes into the schematic. kitest adds the supplies,
the stimulus and the analysis itself, and it takes part models from KiCad's
own Simulation Model fields or from a model library.

The engine is a Rust crate, `kitest`. A thin PyO3 crate, `kitest-py`,
exposes the same engine to Python as the `kitest` package.

## Where to go next

- [Getting started](guide/getting-started.md) runs `kitest` on the Colpitts
  example.
- [Writing Expect fields](guide/expect.md) gives the syntax of a probe's
  check.
- [kitest.toml](guide/config.md) declares supplies and model libraries.
- [Models](guide/models.md) tells how kitest finds a SPICE model for each
  part.
- [The kitest command](guide/cli.md) covers the output, the exit codes,
  `--show`, and how to read a failure.
- The [Python API](reference/python.md) and the [Rust API](reference/rust.md)
  give the full programmatic surface.
