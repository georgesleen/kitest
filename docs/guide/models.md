# Models

kitest needs a SPICE model for each part that it simulates. It looks for
the model of each part in this order, and uses the first that applies:

1. The model file that the part's `Sim.Library` field names.
2. The part's Value, for a resistor, a capacitor or an inductor.
3. A model library entry that covers the part's Value.
4. A default model for the part's `Sim.Device`.

kitest leaves out a part that is excluded from simulation or marked DNP.

## Sim.Library and Sim.Name

KiCad's Simulation Model dialog writes these fields on a symbol. kitest
reads them as KiCad's own simulator does:

- `Sim.Library` is the path of a SPICE model file. kitest expands
  `${KIPRJMOD}` to the project directory, and expands any other `${VAR}`
  from the environment. A relative path is taken from the project
  directory. A path through a variable that only KiCad sets, such as
  `${KICAD9_SYMBOL_DIR}`, is resolved from KiCad's own SPICE export.
- `Sim.Name` is the `.model` or `.subckt` in that file to use, in any case.
  If the file defines only one, `Sim.Name` is optional.
- `Sim.Device`, such as `NPN` or `PMOS`, must agree with the type of a
  `.model` card.
- `Sim.Pins` maps symbol pins to model pins, as in `1=E 2=B 3=C`. Without
  it, a `.subckt` takes the symbol's pins in pin-number order.

kitest reads the fields from the placed symbol first, then from its
library symbol.

### Example

The Colpitts example binds its transistor `Q1` through the bundled model
library. To bind it to a model file instead, put this file at
`models/q.lib` in a copy of the project:

```text
* vendor model
.model Q2N3904_VENDOR npn(is=6.734f xti=3 eg=1.11 vaf=74.03 bf=416.4
+ ne=1.259 ise=6.734f ikf=66.78m xtb=1.5 br=.7371 nc=2 isc=0 ikr=0 rc=1
+ cjc=3.638p mjc=.3085 vjc=.75 fc=.5 cje=4.493p mje=.2593 vje=.75
+ tr=239.5n tf=301.2p itf=.4 vtf=4 xtf=2 rb=10)
```

Then set these fields on `Q1`:

| Field | Value |
| --- | --- |
| `Sim.Library` | `${KIPRJMOD}/models/q.lib` |
| `Sim.Name` | `Q2N3904_VENDOR` |

With `oscillates(near=10.4MHz, within=5%)` on `PRB1`:

```text
$ kitest
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 10.1162 MHz is 283.8175 kHz from 10.4 MHz, within ±5% (520 kHz), at a steady 3.0443 V amplitude
1 passed, 0 failed
```

`models/q.lib` without `${KIPRJMOD}/` gives the same result. A wrong
`Sim.Name` stops kitest with exit status 2, and kitest lists what the file
defines:

```text
$ kitest
error: cannot simulate 1 part:
  - Q1's Sim.Name "Q2N3904" is not defined in ./models/q.lib, which defines Q2N3904_VENDOR
```

A file that defines more than one model needs `Sim.Name`:

```text
$ kitest
error: cannot simulate 1 part:
  - Q1 sets Sim.Library ./models/q.lib but no Sim.Name, and the file defines Q2N3904_VENDOR, Q2N3904_OTHER; set Sim.Name to the one to use
```

## Resistors, capacitors and inductors

A part is a resistor, a capacitor or an inductor if its `Sim.Device` is
`R`, `C` or `L`. A part without `Sim.Device` is one if its reference
starts with `R`, `C` or `L`. kitest reads the part's Value as KiCad writes
it, such as `4k7`, `470pF` or `1uH`. Such a part must have two pins, and
must not set `Sim.Params`.

## Model libraries

A model library is a TOML file that maps part Values to model cards.
kitest bundles a library with common parts, such as the 2N3904. The
`libraries` list under `[models]` in `kitest.toml` adds project libraries,
which kitest searches before the bundled one. A Value matches in any case.

Each `[[model]]` entry has these keys:

| Key | Meaning |
| --- | --- |
| `values` | The part Values that the entry covers. Two entries cannot cover the same Value. |
| `source` | Optional: where the card came from, such as a manufacturer and a date. |
| `card` | One `.model` or `.subckt` card. |
| `ports` | For a `.subckt` only: the pin role of each port, in the card's port order. |

A `.model` card connects pins by device role: `C B E` for a BJT, `A K`
for a diode, `D G S` for a JFET, and `D G S B` for a MOSFET.

### Example

In a copy of the Colpitts example, set the Value of `Q1` to `MYNPN`. Put
this library at `models/house.toml`:

```toml
[[model]]
values = ["MYNPN"]
source = "vendor 2N3904 under a house part number"
card = """
.model MYNPN npn(is=6.734f xti=3 eg=1.11 vaf=74.03 bf=416.4
+ ne=1.259 ise=6.734f ikf=66.78m xtb=1.5 br=.7371 nc=2 isc=0 ikr=0 rc=1
+ cjc=3.638p mjc=.3085 vjc=.75 fc=.5 cje=4.493p mje=.2593 vje=.75
+ tr=239.5n tf=301.2p itf=.4 vtf=4 xtf=2 rb=10)
"""
```

and name it in `kitest.toml`:

```toml
[supplies]
VCC = 9.0

[models]
libraries = ["models/house.toml"]
```

```text
$ kitest
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 10.1162 MHz is 283.8175 kHz from 10.4 MHz, within ±5% (520 kHz), at a steady 3.0443 V amplitude
1 passed, 0 failed
```

## Default models

A part that nothing else covers, but that has a `Sim.Device` such as
`NPN`, `PNP`, `D`, `NJFET`, `PJFET`, `NMOS` or `PMOS`, is simulated on
ngspice's default model for that device, with the part's `Sim.Params`.
The `kitest` command does not report a default model. In Python,
`Netlist.defaulted()` lists the parts that use one. This is the Colpitts
example with the Value of `Q1` set to `MYNPN` and no project library:

```text
>>> import kitest
>>> design = kitest.export_design("/tmp/colpitts/colpitts.kicad_sch")
>>> design.netlist().defaulted()
['Q1']
```

A part with no `Sim.Library`, no `Sim.Device`, and no library entry for
its Value stops kitest:

```text
$ kitest
error: cannot simulate 1 part:
  - Q1 has no simulation model: it has no Sim.Library or Sim.Device, and no model library covers its value "MYNPN"; assign it a model file in KiCad's Simulation Model dialog, add the model to a [models] library in kitest.toml, or exclude Q1 from simulation
```
