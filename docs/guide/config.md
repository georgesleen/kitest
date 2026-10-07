# kitest.toml

`kitest.toml` declares the test conditions of a project. kitest reads it
from the directory that holds the schematic. The file is optional; without
it, kitest uses no declared supplies and only the bundled model library.

The file has two tables, `[supplies]` and `[models]`. Any other table or
key is an error.

```toml
[supplies]
VCC = [5.0, 9.0, 12.0]

[models]
libraries = ["models/house.toml"]
```

## [supplies]

Each key is the full name of a net, and each value is a voltage in volts.
Quote a name that starts with `+` or `-`, such as `"+3V3" = 3.3`. A net
that a label names has its sheet path in its full name, such as `/OUT` in
the Colpitts example.

kitest adds an ideal voltage source from each declared net to ground.

### Corners

A list of voltages, such as `VCC = [5.0, 9.0, 12.0]`, makes each voltage a
separate corner. kitest runs every check once at each corner. With more
than one list, kitest runs every combination.

The Colpitts example with `VCC = [5.0, 9.0, 12.0]`:

```text
$ kitest
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=5 V: 10.1484 MHz is 251.6406 kHz from 10.4 MHz, within ±5% (520 kHz), at a steady 1.8191 V amplitude
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 10.1162 MHz is 283.8175 kHz from 10.4 MHz, within ±5% (520 kHz), at a steady 3.0443 V amplitude
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=12 V: 10.1041 MHz is 295.8739 kHz from 10.4 MHz, within ±5% (520 kHz), at a steady 3.9572 V amplitude
3 passed, 0 failed
```

### How kitest powers each rail

A rail is a net that a power symbol makes, or a net that `[supplies]`
declares. kitest resolves each rail in this order:

1. **Driven**: a power-output pin drives the rail, such as the output of a
   regulator. The circuit powers the rail, and kitest adds no source. A
   driven rail must not be in `[supplies]`.
2. **Declared**: `[supplies]` gives the voltage.
3. **Ground**: the last part of the net name is a ground name, such as
   `GND`, `AGND`, `DGND`, `GNDA`, `PGND`, `0V` or `EARTH`, in any case.
4. **Inferred**: the last part of the net name states one voltage, such as
   `+5V`, `-15V`, `+3V3`, `+1V8`, `12V` or `+5VA`. kitest uses that voltage.

A rail that matches none of these is an error, and kitest names the pins
that the rail feeds. The Colpitts example with an empty `kitest.toml`:

```text
$ kitest
error: cannot resolve supplies: 1 problem:
  - rail VCC has no voltage; it feeds Q1 (2N3904) pin C, R1 (47k) pin 1; add "VCC" = <volts> under [supplies] in kitest.toml
```

A declared name that is not a net is also an error:

```text
$ kitest
error: cannot resolve supplies: 1 problem:
  - supply "VDD" names no net
```

## [models]

`libraries` lists model library files for the project. A relative path is
taken from the directory that holds `kitest.toml`. kitest searches these
files in order, then the library that it bundles.
[Models](models.md) gives the file format.

## Errors

An unknown table or key stops kitest with exit status 2. This is
`[supply]` in place of `[supplies]`:

```text
$ kitest
error: ./kitest.toml is not a valid kitest config
caused by: TOML parse error at line 1, column 2
  |
1 | [supply]
  |  ^^^^^^
unknown field `supply`, expected `supplies` or `models`
```

A supply with an empty list, or with a value that is not a finite number,
is also an error.
