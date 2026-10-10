# The scope

`kitest-scope` opens simulation captures as waveforms, spectra, or Bode plots,
with pass/fail expectations drawn over the traces. This page describes the
implemented viewer and capture format. See [Expect fields](expect.md) for probe
checks and [pending scope designs](scope-next.md) for features not implemented,
including filters and MCP integration.

## Opening captures

```sh
kitest-scope FILE
kitest --show PROBE [PROJECT]
kitest --sketch PROBE [PROJECT]
```

The first `kitest-scope` launch opens a window. Later launches hand their file
to that window and exit. `PROBE` may be a probe name or reference such as `PRB1`.
`--show` opens the matching captures; `--sketch` prints a terminal waveform.
They are mutually exclusive. Both run the project's checks, rather than merely
opening an old result. The project defaults to the working directory.

`kitest` writes available waveforms to the project's `.kitest/captures/` even
without `--show`. Files are named from the capture name and corner, replacing
characters other than ASCII letters, digits, and dots with underscores.
Operating-point-only DC checks do not produce waveforms, so they cannot be
shown or sketched. A failed oscillation check can still have a capture.

The window polls the last opened file's modification time, about every 500 ms.
Only that file is watched, not every file in an overlaid corner family. Opening
a capture with the same `name` replaces its matching `corner`, or adds a new
corner to the family. Family trace names include a ` [CORNER]` suffix when a
corner is named; use the names reported by `state` in commands. Each corner
retains its own sampling grid. Opening a different name replaces the family.
Opening the same name with a different analysis kind also replaces the family,
rather than overlaying incompatible transient and AC axes.
Compatible same-name primary views preserve pane layout, cursors, and zoom and
draw the previous run faded behind the new one. This is a previous-run
comparison, not an unlimited history.

A transient capture opens on Spectrum only when it has at least one expectation
and all its regions are spectral (`frequency` or `distortion`). Otherwise it
opens on Primary. Adding `min_swing` produces a time-domain region, so an
oscillation capture with that limit opens on its waveform. The initial spectrum
uses the fitted full time range, including startup; it is not automatically
restricted to the steady-state interval used by the check. Reloading the same
name retains the selected instrument when possible.

## Instruments

- **Primary / transient:** volts against seconds, with a shared time axis and
  separate panes when traces are split.
- **Spectrum / transient:** logarithmic frequency in Hz against amplitude in
  dBV (`20 log10` of zero-to-peak volts, not RMS volts). The FFT linearly
  resamples the primary view's visible time interval, removes its resampled
  mean, and applies a periodic Hann window with amplitude correction. Its
  sample count is the next power of two of the interval's original point count,
  clamped to 256 through 65,536. DC is omitted. Markers show the dominant tone
  and the median non-DC-bin noise floor. Harmonic guides `2f` through `8f` within
  bandwidth are hidden by default; enable **Show harmonics** in the pane's
  right-click menu. This noise floor is a bin-amplitude statistic, not a spectral
  density.
  A trace with no usable data in that time interval is omitted from the derived
  spectrum, without discarding other valid traces; a later rebuild can recover it.
- **Primary / AC:** Bode magnitude in dB and unwrapped phase in degrees against
  logarithmic frequency in Hz. **Reference** divides every response by a
  selected trace (subtracting its dB magnitude and phase); **Absolute response**
  restores the original responses. Margin annotations use the displayed
  responses: gain margin at an odd multiple of -180 degrees, phase margin at
  the first falling 0 dB crossing. These are readings, not stability verdicts.
- **Group delay / AC:** `-d(phase)/d(angular frequency)` in seconds, from the
  displayed unwrapped phase. Differences are central inside the sweep and
  one-sided at the ends. A selected reference therefore also changes delay.
  Faded comparisons are discarded when the reference basis changes or disappears,
  rather than comparing curves measured against different references.

Spectrum and group delay are derived views. Switching to them constructs a new
view; they do not share the primary view's cursor or pane layout. To select a
steady-state spectrum, zoom the waveform's time axis first, then choose
Spectrum. Moving spectral cursors changes spectral measurements, not the FFT's
time interval. Hiding or showing traces retains the derived curves and their
navigation state. Trigger/reference changes and same-capture reloads rebuild
derived curves while preserving compatible pane layout, measurements, cursors,
and zoom. Explicitly switching instruments constructs a fresh derived view.

**Trigger** is post-processing for transient captures, not a new simulation or
a live acquisition trigger. Choose a trace and rising/falling edge. With cursor
A on the primary waveform, the trace's interpolated voltage at A supplies the
level; socket commands may supply an explicit level in volts. Each corner run's
first matching crossing of the selected base trace is aligned to zero; a run
without a matching crossing remains on its source time axis. Time-domain
expectation regions shift with their run. Clear restores absolute time and
regions. Use primary-view cursor A, not a cursor placed on the spectrum, to
set a menu trigger.

## Window controls

| Control | Action |
|---|---|
| Traces sidebar | Show or hide a trace across its channels. |
| View menu | Primary, Spectrum, or Group delay, as the capture permits. |
| Reference menu (AC) | Select a reference trace or restore absolute response. |
| Trigger menu (transient) | Clear, or select a trace and edge at cursor A's level. |
| Hover a plot | Linked x marker and interpolated trace readouts. |
| Scroll over a plot | Zoom the shared x axis about the pointer. |
| Ctrl+scroll / pinch zoom | Zoom that pane's y axis. |
| Middle drag or Ctrl+left drag | Pan x and that pane's y axis. |
| Right drag | Zoom to a rectangle. |
| Double-click or right-click > Fit | Fit all axes to shown traces. |
| A or B while hovering a plot | Place the corresponding x cursor. |
| Escape | Remove both cursors. |
| Ctrl+S | Open the PNG save dialog. |

The command modifier is Command on platforms that use it instead of Ctrl.
Right-click a pane for each channel's **Move to a new pane** (when the pane
holds multiple channels), **Move to pane N** (only panes of the same quantity),
and **Measurements** checkboxes. Spectrum panes also offer **Show harmonics**,
which toggles harmonic guides across the spectrum while leaving the dominant
tone and expectation guides visible. The choice survives zoom, pan, and derived
view rebuilds. Empty panes disappear after moves. Menu pane
numbers start at 1; socket pane indices start at 0. Measurements are printed
for each shown channel below its pane. Cursor readouts include B minus A for
each channel, time separation and reciprocal separation on waveforms, or B/A
frequency ratio on logarithmic axes.

## Measurements and exports

The measurement window is between A and B when both exist and differ;
otherwise it is the visible x range. `measure` can override it with `over`.
Windows are clipped to available data, with linearly interpolated ends.
Measurements use original curves, not the display's pixel-decimated lines.

The scope and engine share `kitest-measure`'s sampled-curve primitives:

- `min`, `max`, and `peak_to_peak` use the clipped curve, including its ends.
- `mean` is the integral of the piecewise-linear signal divided by interval
  duration, not the arithmetic mean of samples. `rms` is the square root of
  the integral of that signal's square divided by duration. Each segment uses
  `(a*a + a*b + b*b)/3`, so uneven sample spacing is handled by integration.
- `frequency` counts rising crossings of the midpoint of the interval's
  minimum and maximum. It returns `(crossings - 1)/(last - first)` and needs
  at least two crossings. This is not the FFT dominant-tone estimator used by
  oscillation checks.
- `rise_time` finds the first rising 90% crossing and the preceding rising 10%
  crossing; `fall_time` uses the first falling 10% crossing and preceding 90%
  crossing. Percentages refer to the measured interval's min/max span, not a
  fixed supply or target voltage. Missing complete edges yield no reading.
- Crossings are linearly interpolated. Samples exactly on a level count once
  when the curve leaves on the opposite side; touching and returning is not a
  crossing.
- `half_power` returns all crossings 3.0103 dB below the interval's peak.
  On logarithmic frequency axes, interpolation is in log-frequency coordinates.

Waveform panes offer all measurements except `half_power`. Spectrum and Bode
magnitude offer `min`, `max`, and `half_power`; phase offers `min` and `max`.
Group-delay panes offer `min`, `max`, and `peak_to_peak`, in seconds.
Waveform-only timing and repetition measurements are not offered on delay's
log-frequency axis.

**File > Save PNG** saves a screenshot of the window, including its UI.
**File > Save CSV** exports the active view's shown channels over the measurement
window, with units in the header. Bode export contains displayed dB magnitude
and phase, not raw complex values; spectrum and delay export their derived
values. For independently sampled traces, rows use the union of shown curves'
sample coordinates within the measurement window. Other columns interpolate
at those coordinates, with empty fields outside their curve's domain.
Interpolation on frequency views is in log10 Hz; exported x values are Hz.
CSV is a sampled export, not a new simulation. PNG needs a running window;
CSV also works through `query`.

## Capture APIs and JSON v2

Rust `Transient::capture(name)` and `Spectra::capture(name)` return a
`kitest_scope::Capture` containing every node's waveform or complex response,
with no attached expectations. Set `corner` explicitly if needed. A check may
contain multiple regions, each with its own verdict: use the plural
`Check::expectations(trace)` and append all of them before saving.

```rust
let mut capture = transient.capture("step-response");
capture.expectations.extend(check.expectations("vout"));
capture.save(std::path::Path::new("step-response.json"))?;
```

Here `transient` and `check` are already computed results. See the runnable
[RC capture example](https://github.com/georgesleen/kitest/blob/main/crates/kitest/examples/rc_capture.rs) for transient and
AC capture generation.

Python uses the same format:

```python
capture = result.capture("step-response")  # Transient or Spectra
capture.expect("vout", check)
capture.save("step-response.json")
```

`expect(trace, check)` appends every drawable region, without re-running the
check. It raises `ValueError` for an unknown trace or a check with no drawable
region. Save errors raise `OSError`. Python's separate
`kitest.scope(result, *nodes, save=None, title=None)` is a matplotlib transient
plotter, not the `kitest-scope` application or its socket API; it requires the
optional plotting dependency and does not attach these expectation overlays.

The file is a flat JSON object, not a nested `data` object: Rust's `data` field
is serialized with its fields at the top level. The v2 fields are:

| Field | Content |
|---|---|
| `version` | `2`; other versions are rejected on load. |
| `name` | Capture identity across re-runs. |
| `corner` | Optional string; omitted when absent. |
| `kind` | `transient` or `ac`. |
| `time` / `frequency` | Transient seconds / AC hertz, in ascending order. |
| `traces` | Transient `{name, values}` or AC `{name, re, im}` arrays. |
| `expectations` | Optional array of `{trace, passed, message, region}`. |

Each trace array must match the axis length, and every expectation must name
an existing trace. Regions are tagged by their own `kind`:

| Region kind | Fields and units |
|---|---|
| `band` | `start`, `end` on the capture axis; `low`, `high` bounds on y. |
| `frequency` | `low`, `high` in Hz, drawn on the spectrum. |
| `swing` | `start`, `end` in seconds; `centre`, `minimum` in volts. Minimum is half peak-to-peak. |
| `distortion` | `fundamental` in Hz, `amplitude` zero-to-peak volts, `maximum` as a fraction, drawn on the spectrum. |

Expectations carry results and annotations, not executable instructions. The
viewer does not recompute a pass/fail result when a cursor or window moves.

## Live control and headless queries

The window accepts line-delimited JSON-RPC 2.0 over a local Unix socket at
`$XDG_RUNTIME_DIR/kitest-scope.sock`. The runtime directory must be absolute,
user-owned, and private. Without `XDG_RUNTIME_DIR`, the scope creates a mode
0700 `kitest-scope-<effective uid>` directory under the system temporary
directory (normally `/tmp`) and uses `kitest-scope.sock` inside it. Invalid
directory ownership or permissions are errors, not a reason to use a public
socket. Client IO has a 30-second deadline. CLI wrappers print the result JSON:

```sh
kitest-scope ctl METHOD [PARAMS]
kitest-scope query FILE METHOD [PARAMS]
kitest-scope ctl measure '{"trace":"/OUT","measurements":["frequency","rms"]}'
kitest-scope query step-response.json state
```

`PARAMS` is a JSON object. `ctl` addresses the live window; `query` loads FILE
into an independent model, applies one command, and exits. A query does not
change the window or persist layout into FILE. Both exit 1 for a refused
command. `ctl` exits 2 when no window is running; other socket errors exit 1.
Malformed command-line JSON, or a query capture that cannot load, exits 2.
Positions and ranges are in real axis units (seconds, Hz, volts, dB, dBV,
degrees), never logarithms.
Log-axis positions must be positive; ranges need distinct ends.

For example, a direct socket request is
`{"jsonrpc":"2.0","id":1,"method":"state","params":{}}` followed by a newline.
An absent
`id` is a notification: it executes without a reply, even for method or
parameter errors. Explicit `id: null` receives a reply. IDs may be strings,
numbers, or null. Nonempty JSON-RPC batches return one array containing only
the calls that need replies; notification-only batches produce no reply.
Invalid batch elements return `-32600` with null ID; an empty batch returns
one invalid-request error. Named parameter objects are supported, not
positional arrays. `state` and `fit` accept absent, null, or empty-object params.

| Method | Parameters | Effect |
|---|---|---|
| `open` | `path` | Open a capture. Relative paths resolve in the receiving process's working directory. |
| `state` | none | Return capture and plot state. |
| `view` | `name`: `primary`, `spectrum`, `group_delay` | Select an instrument. |
| `show` | `trace`, `shown` | Show/hide a trace. |
| `zoom` | optional `x`: `[from,to]`, `y`: `[from,to]`, `pane` (default 0) | Set x or a pane's y range. |
| `fit` | none | Fit all axes. |
| `cursors` | optional `a`, `b` | Place cursors; missing or null removes that cursor. |
| `measure` | `trace`, `measurements`, optional `quantity`, `over`: `[from,to]` | Return measurements over explicit range, cursor range, or visible range. |
| `measurements` | `pane`, `measurements` | Set the pane's measurement strip. |
| `harmonics` | `shown`: boolean | Show/hide `2f` through `8f` guides in Spectrum; refused on other instruments. |
| `move` | `trace`, optional `quantity`, `to` | Move a channel to a zero-based pane; omitted/null `to` creates a new pane. |
| `reference` | optional `trace` | Set Bode reference; omitted/null restores absolute responses. |
| `trigger` | optional `trace`, `edge`: `rising` (default) or `falling`, `level` | Set transient trigger in volts; missing level uses primary cursor A; missing trace clears. |
| `save` | `format`: `png` or `csv`, `path` | Save screenshot or displayed channels. |

`quantity` selects a channel such as `magnitude` or `phase`; omitted means the
trace's first channel. Measurement spellings are `min`, `max`, `peak_to_peak`,
`mean`, `rms`, `frequency`, `rise_time`, `fall_time`, and `half_power`.
Inapplicable measurements, unknown parameter fields, unknown
traces/quantities/panes, and moving between different quantities are refused.

Most methods return the new state. `measure` returns a map of measurement
names to `{value, unit}`, or `null` for an undefined reading; `half_power`'s
value is an array of positions. `save` returns `{path}`. State contains `path`,
`capture`, selected `view`, available `views`, trace names and visibility, and
`plot`. Plot state includes `kind`, x axis, panes (y axes, shown channels,
measurements), cursors, trigger, reference, annotations, and expectations.
Spectrum plot state also includes `harmonics`, the **Show harmonics** checkbox
value; it is false by default and absent on other instruments.
Axis records contain `quantity`, `unit`, `log`, and a real-unit `range`.

JSON-RPC errors use `-32700` for malformed JSON, `-32600` for invalid requests,
`-32601` for unknown methods, `-32602` for invalid parameters, and `-32000` for
commands that cannot be carried out (including headless PNG saves or file IO
failures). The menu reports refused commands in the status bar.
