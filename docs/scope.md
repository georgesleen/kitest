# The scope

`kitest-scope` shows simulation captures: waveforms, spectra, and Bode plots,
with each test expectation drawn on the plot. One window serves the whole
session. A script or an agent drives it with the same commands as its menus.

## Opening captures

```sh
kitest-scope FILE
```

The first launch opens the window. Each later launch hands its file to that
window and exits. The window reloads its file when the file changes. A
capture with the same name keeps the layout, the cursors, and the zoom, and
the previous run stays behind it as a faded trace. Captures of one name at
different corners overlay as a family.

`kitest --show PROBE` writes the probe's captures to the project's
`.kitest/captures/` and opens them this way.

## Commands

The scope speaks JSON-RPC 2.0, one message per line, on the Unix socket
`$XDG_RUNTIME_DIR/kitest-scope.sock`. Two commands wrap it:

```sh
kitest-scope ctl METHOD [PARAMS]          # the live window
kitest-scope query FILE METHOD [PARAMS]   # FILE alone, without a window
```

Both print the result as JSON and exit 1 on a refused command. `ctl` exits 2
when no window runs. Positions and ranges are in each axis's own unit:
seconds, hertz, volts, decibels, or degrees, never logarithms.

| Method | Parameters | Effect |
|---|---|---|
| `open` | `path` | Opens a capture. A relative path resolves in the window's directory. |
| `state` | | Reports what the window shows. |
| `view` | `name`: `primary`, `spectrum`, or `group_delay` | Shows one instrument. |
| `show` | `trace`, `shown` | Shows or hides a trace. |
| `zoom` | `x`: `[from, to]`, `y`: `[from, to]`, `pane` (default 0) | Shows a window on the x axis, or on a pane's y axis. |
| `fit` | | Fits every axis to the shown traces. |
| `cursors` | `a`, `b` | Places cursors A and B; a missing one is removed. |
| `measure` | `trace`, `measurements`, `quantity`, `over`: `[from, to]` | Measures one trace, over `over`, else between the cursors, else across the visible window. |
| `measurements` | `pane`, `measurements` | Sets what a pane prints in its strip. |
| `move` | `trace`, `quantity`, `to` | Moves a trace into pane `to`, or into a new pane. |
| `reference` | `trace` | Divides every Bode response by a trace's; no trace shows absolute responses. |
| `trigger` | `trace`, `edge`: `rising` or `falling`, `level` | Aligns each run's first crossing of `level` to time zero. Without `level`, cursor A sets it; without `trace`, the trigger clears. |
| `save` | `format`: `png` or `csv`, `path` | Writes the window as a PNG, or the shown traces as CSV. A PNG needs the window. |

`quantity` picks one channel of a trace that has several, such as `magnitude`
or `phase` on a Bode plot. The trace's first channel is used without it.

The measurements are `min`, `max`, `peak_to_peak`, `mean`, `rms`,
`frequency`, `rise_time`, `fall_time`, and `half_power`. A pane accepts only
those that apply to its quantity.

Most methods return the new state. `measure` returns one reading per
measurement, each a `value` and a `unit`, or `null` when the window does not
define it. `save` returns the path it wrote.

```sh
$ kitest-scope ctl measure '{"trace":"/OUT","measurements":["frequency","rms"]}'
{
  "frequency": { "unit": "Hz", "value": 10118853.9 },
  "rms": { "unit": "V", "value": 6.665 }
}
```

A refused command returns a JSON-RPC error: `-32601` for an unknown method,
`-32602` for parameters that do not fit the capture, and `-32000` for a
command the scope could not carry out.
