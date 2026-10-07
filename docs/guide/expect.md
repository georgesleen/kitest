# Writing Expect fields

A probe's `Expect` field holds one check. The check is written as a call,
with the same names as the Python API, such as:

```text
oscillates(near=10.4MHz, within=5%)
```

kitest parses the field. It never evaluates it as Python.

## The checks

| Check | Passes when | `near=` is |
| --- | --- | --- |
| `dc(near=..., within=...)` | The net's DC operating-point voltage is within `within` of `near`. | a voltage |
| `oscillates(near=..., within=...)` | The net sustains an oscillation, and its frequency is within `within` of `near`. | a frequency |

Both checks take the two keyword arguments `near=` and `within=`, and
nothing else. Both arguments are necessary, in either order. A trailing
comma and spaces around `=` and `,` are permitted.

## Values

A value reads like a KiCad part value: a number, an optional SI
multiplier, and an optional unit.

- **Multipliers**: `f`, `p`, `n`, `u` (also `µ`), `m`, `k` (also `K`),
  `M`, `meg` (also `Meg` and `MEG`), `G`, `T`. A capital `M` is mega and a
  lowercase `m` is milli.
- **Units**: `V` for a voltage; `Hz` (also `hz` and `HZ`) for a frequency.
  A unit that does not match the check is an error.
- **Other forms**: an exponent, as in `10.4e6`; one space before the
  multiplier, as in `10.1 MHz`; a multiplier as the decimal point, as in
  `3V3` for 3.3 V; a sign, as in `-2.5V`.

## Tolerances

`within=` takes one of these forms:

| Form | Meaning |
| --- | --- |
| `5%` | 5 percent of `near` |
| `100mV`, `200kHz` | an absolute amount, in the unit of the check |
| `percent(5)` | the same as `5%` |
| `abs(100mV)` | the same as `100mV` |

A bare number, such as `within=5`, is an error, because it can be a
percentage or an amount. The tolerance must be more than zero.

## Examples

Each of these is the `Expect` field of `PRB1` in a copy of
`examples/kicad/colpitts`, with the output of `kitest`.

A DC check with an absolute tolerance:

```text
dc(near=3V7, within=abs(200mV))
```

```text
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 3.7065 V is 6.5246 mV from 3.7 V, within ±200 mV
1 passed, 0 failed
```

A DC check that fails:

```text
dc(near=3.3V, within=5%)
```

```text
FAIL COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 3.7065 V is 406.5246 mV from 3.3 V, outside ±5% (165 mV)
    Q1 (npn) is active: Vbe 0.658 V, Vce 5.293 V
0 passed, 1 failed
```

An oscillation check with a spaced multiplier and an absolute tolerance:

```text
oscillates(near=10.1 MHz, within=200kHz)
```

```text
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 10.12 MHz is 19.9771 kHz from 10.1 MHz, within ±200 kHz, at a steady 3.0254 V amplitude
1 passed, 0 failed
```

The same check, with an exponent and `percent(...)`:

```text
oscillates(near=10.4e6, within=percent(5))
```

```text
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 10.1162 MHz is 283.8175 kHz from 10.4 MHz, within ±5% (520 kHz), at a steady 3.0443 V amplitude
1 passed, 0 failed
```

## How oscillates decides

`oscillates` runs a transient analysis at 100 samples per expected cycle.
At the start, a current impulse kicks the probe's net, so that startup
does not wait for numerical noise. The run lasts 200 expected cycles, then
800, then 3200, until the amplitude is steady. The check then fails for
one of these results:

- The amplitude stays below 1 mV: there is no oscillation. If the
  amplitude falls, the kick rang a resonance.
- The amplitude still grows after 3200 cycles: the oscillator is still
  starting.
- The amplitude falls after 3200 cycles: the circuit rings but does not
  sustain an oscillation.

When the amplitude is steady, the check measures the dominant frequency
over the last 50 cycles and compares it with `near`.

`oscillates` cannot check a crystal oscillator yet. If the design holds a
crystal, every `oscillates` check fails and says so. A crystal is a part
with a `Y` reference, such as `Y1`, or a symbol from the `Crystal` library,
or a symbol whose name starts with `Crystal`.

## Errors

kitest reads every `Expect` field before it simulates anything. If a field
is wrong, kitest stops with exit status 2 and names the probe:

```text
$ kitest
error: cannot read probe expectations:
  - PRB1 Expect: oscillates's within=5 could be a percentage or an amount; write 5% for a percentage, or give a frequency, such as 10.4MHz or 32.768k
```

```text
$ kitest
error: cannot read probe expectations:
  - PRB1 Expect: unknown check "osc"; a probe can state dc(near=3.3V, within=5%) or oscillates(near=10.4MHz, within=2%)
```

```text
$ kitest
error: cannot read probe expectations:
  - PRB1 Expect: "10MHz" is not a voltage, such as 3.3V or 250mV
```

```text
$ kitest
error: cannot read probe expectations:
  - PRB1 Expect: expected ')' at column 11, found 'w', in dc(near=1 within=abs(1))
```
