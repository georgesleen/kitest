//! A waveform drawn as text, for `kitest --show`.

use kitest::Waveform;

/// Columns of plot area.
const WIDTH: usize = 64;
/// Rows of plot area.
const HEIGHT: usize = 9;
/// Expected cycles in the zoomed panel.
const ZOOM_CYCLES: f64 = 5.0;

/// Two panels: the whole run, then its last `ZOOM_CYCLES` expected cycles.
pub(crate) fn sketch(waveform: &Waveform) -> String {
    let end = waveform.time.last().copied().unwrap_or(0.0);
    let start = waveform.time.first().copied().unwrap_or(0.0);
    let zoom = (end - ZOOM_CYCLES / waveform.expected_hertz).max(start);
    let mut text = format!("whole run:\n{}", panel(waveform, start, end));
    text.push_str(&format!(
        "last {ZOOM_CYCLES} expected cycles:\n{}",
        panel(waveform, zoom, end)
    ));
    text
}

/// The waveform from `from` to `to` seconds, each column spanning the
/// lowest to highest voltage in its slice of time.
fn panel(waveform: &Waveform, from: f64, to: f64) -> String {
    let (time, volts) = (&waveform.time, &waveform.volts);
    let slice = (to - from) / WIDTH as f64;
    let columns: Vec<(f64, f64)> = (0..WIDTH)
        .map(|column| {
            let t0 = from + slice * column as f64;
            let first = time.partition_point(|&t| t < t0);
            let last = time.partition_point(|&t| t < t0 + slice);
            if first < last {
                let span = &volts[first..last];
                let low = span.iter().copied().fold(f64::INFINITY, f64::min);
                let high =
                    span.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                (low, high)
            } else {
                let at = interpolate(time, volts, t0 + slice / 2.0);
                (at, at)
            }
        })
        .collect();
    let low = columns
        .iter()
        .map(|&(low, _)| low)
        .fold(f64::INFINITY, f64::min);
    let high = columns
        .iter()
        .map(|&(_, high)| high)
        .fold(f64::NEG_INFINITY, f64::max);
    let range = (high - low).max(1e-9);
    let row =
        |v: f64| (((high - v) / range) * (HEIGHT - 1) as f64).round() as usize;

    let scale = unit(high.abs().max(low.abs()), "V");
    let mut text = String::new();
    for r in 0..HEIGHT {
        let label = match r {
            0 => format!("{:>12} ┤", in_volts(high, &scale)),
            r if r == HEIGHT - 1 => format!("{:>12} ┤", in_volts(low, &scale)),
            _ => format!("{:>12} │", ""),
        };
        text.push_str(&label);
        for &(lo, hi) in &columns {
            text.push(if (row(hi)..=row(lo)).contains(&r) {
                '█'
            } else {
                ' '
            });
        }
        text.push('\n');
    }
    text.push_str(&format!("{:>12} └{}\n", "", "─".repeat(WIDTH)));
    let (start, end) = (seconds(from), seconds(to));
    let gap = WIDTH.saturating_sub(start.chars().count() + end.chars().count());
    text.push_str(&format!("{:>12}  {start}{}{end}\n", "", " ".repeat(gap)));
    text
}

/// `values` at time `t`, linear between the samples either side.
fn interpolate(time: &[f64], values: &[f64], t: f64) -> f64 {
    let after = time.partition_point(|&sample| sample < t);
    match (after.checked_sub(1), time.get(after)) {
        (Some(before), Some(&t1)) => {
            let t0 = time[before];
            let fraction = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
            values[before] + fraction * (values[after] - values[before])
        }
        (Some(before), None) => values[before],
        (None, _) => values.first().copied().unwrap_or(0.0),
    }
}

/// The largest unit of `base`, from nano up, that keeps `magnitude` at or
/// above one: its scale and its name.
fn unit(magnitude: f64, base: &str) -> (f64, String) {
    let (scale, prefix) = [(1.0, ""), (1e-3, "m"), (1e-6, "µ"), (1e-9, "n")]
        .into_iter()
        .find(|&(scale, _)| magnitude >= scale)
        .unwrap_or((1e-9, "n"));
    (scale, format!("{prefix}{base}"))
}

/// `v` volts in the unit `scale` gives, to three decimals.
fn in_volts(v: f64, (scale, name): &(f64, String)) -> String {
    format!("{:.3} {name}", v / scale)
}

/// `t` seconds in the largest unit that keeps it at or above one.
fn seconds(t: f64) -> String {
    if t == 0.0 {
        return "0 s".to_owned();
    }
    let (scale, name) = unit(t.abs(), "s");
    format!("{} {name}", trim(t / scale))
}

/// `value` to four significant decimals without trailing zeros.
fn trim(value: f64) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}
