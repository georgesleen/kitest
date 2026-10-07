# The kitest command

```text
$ kitest --help
usage: kitest [--show PROBE] [PROJECT]
Check every probe in the KiCad project at PROJECT, a directory or a .kicad_sch file (default: .)
--show PROBE  sketch the waveform an oscillation check ran on, for the probe named or referenced PROBE
```

In the dev shell, `cargo run -q -p kitest -- [ARGS]` runs the command from
the repository.

`PROJECT` is a directory that holds one `.kicad_pro` file, or a
`.kicad_sch` file. Without `PROJECT`, kitest uses the current directory.
kitest reads `kitest.toml` from the directory of the schematic.

## Output

kitest prints one line for each probe at each corner, in the order of the
probes in the schematic:

```text
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 10.1162 MHz is 283.8175 kHz from 10.4 MHz, within ±5% (520 kHz), at a steady 3.0443 V amplitude
1 passed, 0 failed
```

The line gives, in sequence:

- The status: `PASS`, `FAIL`, or `----` for a probe without `Expect`.
- The probe's name, then its reference and its net in parentheses.
- The corner: the voltage of each rail that kitest supplied.
- The measured value, its distance from `near`, and the tolerance.

The last line counts the checks that passed and failed.

## Exit status

| Status | Meaning |
| --- | --- |
| 0 | Every check passed. Probes without `Expect` do not count. |
| 1 | One or more checks failed. |
| 2 | kitest could not check the probes, or the command line is wrong. |

Status 2 comes with a line that starts with `error:`. For example, a path
that does not exist:

```text
$ kitest /tmp/nothere
error: /tmp/nothere is not a KiCad project: cannot read it: No such file or directory (os error 2)
```

## Read a failure

For a failed check, kitest runs a DC operating point at the same corner and
prints what it finds, indented under the check:

- The DC voltage of the probe's net, unless the check is a `dc` check.
- The bias of each transistor. A BJT is `off` when its Vbe is below
  0.5 V, `saturated` when its Vce is below 0.2 V, and `active` otherwise.
  A FET shows its Vgs and Vds.
- A labelled net that sits at 0 V and feeds a collector or a drain. This is
  usually a supply that nothing powers.
- A note when every net sits at 0 V, which means that nothing powers the
  circuit.

An oscillation check against the wrong frequency. The transistor is
active and the circuit oscillates, so the expectation is wrong, not the
circuit:

```text
$ kitest
FAIL COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 10.1239 MHz is 1.8761 MHz from 12 MHz, outside ±2% (240 kHz), at a steady 3.024 V amplitude
    /OUT sits at 3.707 V DC
    Q1 (npn) is active: Vbe 0.658 V, Vce 5.293 V
0 passed, 1 failed
```

The same Colpitts with `VCC = 0` in `kitest.toml`. The transistor is off
and no net has a voltage:

```text
$ kitest
FAIL COLPITTS_OUT (PRB1, /OUT) at VCC=0 V: no oscillation: 283.2496 µV amplitude after 200 cycles, below the 1 mV floor; the kick rang it, falling 39% every 25 cycles, so it is a resonance, not an oscillator
    /OUT sits at -0.000 V DC
    Q1 (npn) is off: Vbe 0.000 V, Vce 0.000 V
    every net sits at 0 V DC: nothing powers the circuit
0 passed, 1 failed
```

## --show

`--show PROBE` draws, after the report, the waveform that an oscillation
check ran on. `PROBE` is the probe's name or its reference. The sketch
shows the whole run, then the last five expected cycles:

```text
$ kitest --show PRB1
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 10.1162 MHz is 283.8175 kHz from 10.4 MHz, within ±5% (520 kHz), at a steady 3.0443 V amplitude
1 passed, 0 failed

COLPITTS_OUT (PRB1, /OUT) at VCC=9 V
whole run:
     9.090 V ┤  ██████████████████████████████████████████████████████████████
             │  ██████████████████████████████████████████████████████████████
             │ ███████████████████████████████████████████████████████████████
             │ ███████████████████████████████████████████████████████████████
             │ ███████████████████████████████████████████████████████████████
             │ ███████████████████████████████████████████████████████████████
             │ ███████████████████████████████████████████████████████████████
             │████████████████████████████████████████████████████████████████
     3.206 V ┤ ███████████████████████████████████████████████████████████████
             └────────────────────────────────────────────────────────────────
              0 s                                                   19.2308 µs
last 5 expected cycles:
     9.088 V ┤    ████         ████          ████         ████         ████
             │    █  ██        █   █        █   █        ██  █        ██  ██
             │   █    █       ██   ██      ██   ██      ██    █       █    █
             │   █     █      █     █      █     █      █     █      ██    ██
             │  █      █     ██     ██    ██     ██     █      █     █      █
             │  █      ██    █       █    █       █    █       █    ██      █
             │ █        ██  ██       ██  ██       ██  ██       ██   █        █
             │██         █ ██         █ ██         █  █         ██ █         █
     3.208 V ┤█          ███           ██           ███          ███
             └────────────────────────────────────────────────────────────────
              18.75 µs                                              19.2308 µs
```

A `dc` check runs no transient analysis, so it has no waveform:

```text
$ kitest --show COLPITTS_OUT
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 3.7065 V is 6.5246 mV from 3.7 V, within ±200 mV
1 passed, 0 failed

COLPITTS_OUT (PRB1, /OUT) at VCC=9 V
no waveform: only an oscillation check runs one
```

A name that no probe has is an error, with exit status 2:

```text
$ kitest --show NOPE
PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: 10.1162 MHz is 283.8175 kHz from 10.4 MHz, within ±5% (520 kHz), at a steady 3.0443 V amplitude
1 passed, 0 failed
error: no probe is named NOPE; the probes are COLPITTS_OUT
```
