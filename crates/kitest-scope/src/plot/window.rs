//! One axis's visible window, with grid lines at a 1-2-5 spacing.

use std::ops::RangeInclusive;

/// The most x grid lines a window shows.
pub const X_GRID_LINES: f64 = 10.0;

/// The most y grid lines a window shows.
pub const Y_GRID_LINES: f64 = 8.0;

/// The fraction of the data's y span left clear above and below it when a
/// window fits it.
pub const Y_MARGIN: f64 = 0.05;

/// The mantissas of one decade of grid spacings.
const MANTISSAS: [f64; 3] = [1.0, 2.0, 5.0];

/// One axis's window: a centre and a span.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    centre: f64,
    span: f64,
    lines: f64,
}

impl Window {
    /// The window holding `min` to `max`, with `margin` of their span clear
    /// on each side, and at most `lines` grid lines.
    pub fn fit(min: f64, max: f64, margin: f64, lines: f64) -> Self {
        Self {
            centre: (min + max) / 2.0,
            span: nonzero_span(min, max) * (1.0 + 2.0 * margin),
            lines,
        }
    }

    /// The window holding every value of `values`, as [`Window::fit`] does, or
    /// 0 to 1 when there are none.
    pub fn fit_values(
        values: impl Iterator<Item = f64>,
        margin: f64,
        lines: f64,
    ) -> Self {
        let (min, max) = values
            .filter(|value| value.is_finite())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), value| {
                (min.min(value), max.max(value))
            });
        if min > max {
            Self::fit(0.0, 1.0, margin, lines)
        } else {
            Self::fit(min, max, margin, lines)
        }
    }

    /// The spacing of the grid lines.
    pub fn grid_spacing(&self) -> f64 {
        spacing_for(self.span / self.lines)
    }

    /// The visible range.
    pub fn range(&self) -> RangeInclusive<f64> {
        self.centre - self.span / 2.0..=self.centre + self.span / 2.0
    }

    /// Divides the span by `factor`, keeping `around` in place.
    pub fn zoom(&mut self, factor: f64, around: f64) {
        self.span /= factor;
        self.centre = around + (self.centre - around) / factor;
    }

    /// Shows exactly `a` to `b`.
    pub fn zoom_to(&mut self, a: f64, b: f64) {
        if a != b {
            self.centre = (a + b) / 2.0;
            self.span = (b - a).abs();
        }
    }

    /// Moves the window by `delta`.
    pub fn pan(&mut self, delta: f64) {
        self.centre += delta;
    }
}

/// The span from `min` to `max`, or a span that suits their size if it is zero.
fn nonzero_span(min: f64, max: f64) -> f64 {
    let span = max - min;
    let centre = (min + max) / 2.0;
    if span > 0.0 {
        span
    } else if centre != 0.0 {
        centre.abs()
    } else {
        1.0
    }
}

/// The smallest spacing of the 1-2-5 series that is at least `value`.
fn spacing_for(value: f64) -> f64 {
    let decade = 10f64.powf(value.log10().floor());
    MANTISSAS
        .iter()
        .map(|&mantissa| mantissa * decade)
        .find(|&spacing| value <= spacing * (1.0 + 1e-9))
        .unwrap_or(10.0 * decade)
}

#[cfg(test)]
mod tests {
    use super::{Window, spacing_for};

    #[test]
    fn spacing_rounds_up_through_one_two_five() {
        let cases = [
            (0.0041, 0.005),
            (0.5, 0.5),
            (0.5000000001, 0.5),
            (1.01, 2.0),
            (5.5, 10.0),
            (0.1375, 0.2),
        ];
        for (value, expected) in cases {
            let spacing = spacing_for(value);
            assert!(
                (spacing - expected).abs() < 1e-12,
                "{value}: {spacing} != {expected}"
            );
        }
    }

    #[test]
    fn fit_leaves_the_margin_on_each_side() {
        let window =
            Window::fit_values([0.0, 1.0, f64::NAN].into_iter(), 0.05, 8.0);
        let range = window.range();
        assert!(
            (range.start() + 0.05).abs() < 1e-12
                && (range.end() - 1.05).abs() < 1e-12
        );
        assert!((window.grid_spacing() - 0.2).abs() < 1e-12);
    }

    #[test]
    fn zoom_keeps_the_pointer_in_place() {
        let mut window = Window::fit(0.0, 1.0, 0.05, 8.0);
        window.zoom(2.0, 0.8);
        let range = window.range();
        assert!((range.end() - range.start() - 0.55).abs() < 1e-12);
        let before = (0.8 - (-0.05)) / 1.1;
        let after = (0.8 - range.start()) / (range.end() - range.start());
        assert!((before - after).abs() < 1e-12);
    }
}
