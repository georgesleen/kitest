# Getting started

## The dev shell

kitest builds and runs inside the repository's nix dev shell. The shell
supplies Rust, ngspice, `kicad-cli` from KiCad, and the Python packages.

```sh
git clone https://github.com/georgesleen/kitest
cd kitest
nix develop
```

Every command on this page runs in that shell, from the repository root.

## Run kitest on the Colpitts example

`examples/kicad/colpitts` is a 2N3904 Colpitts oscillator with one probe,
`PRB1`, on its output net `/OUT`. Copy the project, so that you can edit
it, and run the `kitest` command on the copy:

```text
$ cp -r examples/kicad/colpitts /tmp/colpitts
$ cargo run -q -p kitest -- /tmp/colpitts
---- COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: no Expect, nothing checked
0 passed, 0 failed
```

The probe has no `Expect` field yet, so kitest simulates nothing for it.
`VCC=9 V` comes from the project's `kitest.toml`:

```toml
[supplies]
VCC = 9.0
```

## Add an expectation

Open `/tmp/colpitts/colpitts.kicad_sch` in KiCad. Edit the properties of
`PRB1` and set its `Expect` field to:

```text
oscillates(near=10.4MHz, within=5%)
```

Save the schematic and run kitest again:

```text
$ cargo run -q -p kitest -- /tmp/colpitts
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 10.1162 MHz is 283.8175 kHz from 10.4 MHz, within ±5% (520 kHz), at a steady 3.0443 V amplitude
1 passed, 0 failed
```

Now make the check fail. Set `Expect` to `oscillates(near=12MHz, within=2%)`
and run kitest again:

```text
$ cargo run -q -p kitest -- /tmp/colpitts
FAIL COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 10.1239 MHz is 1.8761 MHz from 12 MHz, outside ±2% (240 kHz), at a steady 3.024 V amplitude
    /OUT sits at 3.707 V DC
    Q1 (npn) is active: Vbe 0.658 V, Vce 5.293 V
0 passed, 1 failed
```

The indented lines are a diagnosis from the DC operating point.
[The kitest command](cli.md) tells how to read them.

## Probes in your own project

A probe is the `Probe` symbol from `kicad/kitest.kicad_sym` in this
repository. Add that file to your project's symbol libraries with the
nickname `kitest`: kitest finds probes by the library nickname `kitest` and
the symbol name `Probe`.

Place a probe on each net that you want to check, and give it:

- **Value**: the probe's name in the report. With an empty Value, the probe
  takes the name of its net. Two probes cannot have the same name.
- **Expect**: the check, as [Writing Expect fields](expect.md) describes. A
  probe without `Expect` is reported with `----` and is not checked.

The probe symbol is excluded from simulation, from the BOM, and from the
board, so it does not change the circuit. kitest ignores a probe that is
marked DNP.

## Use kitest from Python

The `kitest` Python package runs the same checks. In the dev shell,
`make pytest` builds the extension and links it into
`crates/kitest-py/python/kitest/`. With that directory on `PYTHONPATH`,
`check_project` returns the report that the command prints:

```text
$ PYTHONPATH=crates/kitest-py/python python3
>>> import kitest
>>> report = kitest.check_project("/tmp/colpitts")
>>> for outcome in report.outcomes():
...     print(outcome.probe(), outcome.corner(), bool(outcome), outcome.check())
...
COLPITTS_OUT [('VCC', 9.0)] False 10.1239 MHz is 1.8761 MHz from 12 MHz, outside ±2% (240 kHz), at a steady 3.024 V amplitude
```

The [Python API](../reference/python.md) lists every class and function.
