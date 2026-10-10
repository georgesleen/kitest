# Probe Expect fields

A KiCad kitest probe's `Expect` field states a check on its connected net.
The field is parsed, never evaluated as Python. An empty field leaves the
probe unchecked. `kitest [PROJECT]` runs the checks at the configured supply
corners and reports each outcome. See [the scope](scope.md) for viewing captured
waveforms and attaching check results through Rust or Python.

## Syntax

```text
dc(near=3.3V, within=5%)
dc(near=0V, within=100mV)
oscillates(near=10.4MHz, within=2%)
oscillates(near=10.4MHz, within=2%, min_swing=1V, max_thd=5%)
```

These are syntax examples, not measured results. Only `dc` and `oscillates`
are supported. Both require keyword arguments `near` and `within`; positional
arguments and unknown keywords are errors. Values accept KiCad-style SI
multipliers and optional units. `near` is volts for `dc`, hertz for
`oscillates`. `within` may be a percentage, an absolute amount in that unit,
`percent(5)`, or `abs(100mV)` (use a frequency amount for oscillation).
A percentage is relative to the absolute target: `near=0V` therefore needs an
absolute tolerance for a nonzero band.

`oscillates` also accepts two optional, positive limits:

| Keyword | Meaning |
|---|---|
| `min_swing=1V` | At least 1 V either side of the midpoint of the measured min/max, meaning at least 2 V peak-to-peak. Not RMS, and not minimum voltage relative to ground. |
| `max_thd=5%` | At most 0.05 total harmonic distortion as a fraction of the fundamental amplitude. The field requires percent syntax, not `0.05` or `percent(5)`. |

Omitting either limit means it is not checked. The parser exposes
`Expectation::Dc { near, within }` and
`Expectation::Oscillates { near, within, min_swing, max_thd }` in Rust through
`kitest::parse_expectation`; the optional limits are `Option<f64>`.
The stored THD limit is a fraction, even though the field spelling is a percent.
This probe syntax is not an arbitrary expression language or a general list
of every programmable `Check` operation.

## What is measured

**DC** runs an operating-point analysis and compares the net voltage against
`near +/- within`. It does not capture a transient or check ripple or settling.

**Oscillation** injects a brief current kick to start the net independently of
numerical noise, then tests its amplitude envelope. The nominal kick is 1 mA;
if it causes a step above 0.5 V, the kick is scaled toward a 0.2 V step.
Transient output requests 100 samples per expected cycle. Run lengths are
200, 800, then 3,200 expected cycles, extending while the envelope is unsettled.
The final three envelope windows each cover 25 expected cycles; amplitude
changes of at most 1% per window count as steady. Oscillations below 1 mV
zero-to-peak fail. A run still growing or decaying after the last attempt fails.

A steady run's final approximately 50 expected cycles supply the frequency,
swing, and distortion checks. Frequency is the strongest sinusoid's refined
FFT frequency, not the scope's midpoint-crossing `frequency` measurement.
`min_swing` compares half the peak-to-peak span on that interval. `max_thd`
uses the dominant tone as the fundamental and calculates
`sqrt(A2^2 + ... + A10^2) / A1`: ten harmonics counting the fundamental as
harmonic 1. Harmonics beyond the available spectrum are omitted. This is
harmonic distortion, not THD+N; unrelated noise is not summed as distortion.
The spectrum uses linear resampling, mean removal, a periodic Hann window,
and corrected zero-to-peak tone amplitudes.

Each drawable check contributes its own region, message, and pass/fail verdict.
The combined check passes only when all required checks pass. Frequency regions
appear on the scope's spectrum; swing regions on its waveform; distortion
regions identify the fundamental and allowed harmonic amplitude. A check that
cannot find a tone can fail without a distortion region. Rust's
`Check::expectations(trace)` returns all drawable regions, not one merged
verdict; Python `capture.expect(trace, check)` appends them all.

## Limits

The envelope criterion can mistake a sufficiently slowly decaying, high-Q
resonance (roughly Q above 8,000) for a sustained oscillator. Designs containing
a quartz crystal, identified by a `Y` reference or KiCad crystal symbol, are
rejected for oscillation checks rather than claiming to model crystal startup.
The expected frequency controls duration and sampling, so a badly wrong
`near` can produce an inappropriate simulation interval or bandwidth.

Saved captures contain the full transient, including the kick and startup.
The viewer's initial FFT does not automatically select the check's final
steady-state interval; select that time window on the waveform before opening
Spectrum. Changing viewer cursors never changes the recorded check verdict.
The [pending scope designs](scope-next.md) describe proposed filter and agent
interfaces; they are not supported Expect syntax or implemented scope features.
