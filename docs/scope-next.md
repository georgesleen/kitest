# Scope work still to design or implement

This page records discussions that are not implemented. The [scope guide](scope.md) documents the implemented viewer, capture format and control API; the Scope section of [the roadmap](roadmap.md#scope) summarises both implemented and proposed instruments.

## Filter analysis

Proposed probe syntax, not accepted by the parser:

```text
lowpass(cutoff=1.59kHz, within=5%, input=/IN)
highpass(cutoff=1.59kHz, within=5%, input=/IN)
```

The output probe names the input net. kitest injects a small-signal AC voltage there and measures output/input, with supply rails AC-grounded at their DC corner biases. The input bias and source impedance still need an explicit design for active filters; an ideal grounded source is not safe for every input.

Measure cutoff relative to passband gain, not the sweep's peak. Peaking must not redefine passband gain. A finite sweep endpoint only approximates the passband, so the measurement needs a convergence or plateau criterion and an ambiguity result.

An optional stopband check was discussed as `min_rejection=40dB, at=16kHz`. A single-frequency check must draw a point threshold, not imply a ceiling over the whole stopband. A separate frequency-range mask can assert the latter.

Start with low-pass and high-pass, then band-pass centre/bandwidth and ripple. Sweep limits derive from expectations, but must report when the range misses a crossing. Add a KiCad RC example and closed-form integration tests.

AC filter-check results need to travel through `Outcome`, Python reports, capture writing, and `--show`; the existing waveform field alone is transient-specific. Their captures should use the existing Bode viewer, not try to FFT an AC sweep.

## Oscillator requirements

`min_swing` and `max_thd` are implemented. A maximum swing, an explicit DC bias expectation, and startup deadline were discussed but are not implemented. Do not infer these requirements in the viewer.

THD sums harmonics 2 through 10 relative to the dominant tone. Broader purity measures need separate names: THD+N includes noise, SFDR finds the largest spur, and phase noise measures sidebands around the carrier. A purity assertion must state bandwidth and harmonic count rather than equate all purity with THD.

Startup time needs a defined steady-state envelope and an assertion such as `starts_within=20us`. Its graphical result belongs on the time axis.

## Live window and layout

Add a capture-directory browser, watches for every open family member, and a setting choosing hold-zoom or refit on reload. The implemented viewer already preserves layouts and cursors on reload and retains each family curve's own sample grid.

Stacked panes exist. Tabs, side-by-side splits, resizing, and drag/drop were discussed with `egui_tiles`. Saving layout/zoom/cursors is not requested; PNG and CSV are the requested exports.

## Agent control

The JSON-RPC socket, `ctl`, and windowless `query` exist. An MCP stdio adapter over the same commands is not implemented. Notifications for capture reloads and state changes, discoverable command schemas, and persistent sessions need a protocol design. MCP should be an adapter, not a second execution path.

SCPI compatibility was discussed as a possible lab-instrument adapter, not the primary protocol.

## Further instruments

Smith and polar charts need impedance or port data, not voltage-only AC traces. Measurement strips can add overshoot against an explicit target. Noise and power measurements need an explicit impedance and bandwidth rather than treating peak-voltage dBV as power.
