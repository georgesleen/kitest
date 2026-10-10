//! A scope's plots: panes of channels over one shared x axis.

pub mod bode;
mod decimate;
mod measure;
mod pane;
mod si;
pub mod time;
mod view;
mod window;

pub use measure::Measurement;
pub use view::View;

use eframe::egui::Color32;
use egui_plot::{AxisHints, GridInput, GridMark, PlotPoint};
use kitest_measure::Curve;

/// What a plot axis measures: its name, its unit, whether the axis holds
/// the base-10 logarithm of the value, and what a channel of it can be
/// measured for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Quantity {
    name: &'static str,
    unit: Unit,
    log: bool,
    measurements: &'static [Measurement],
}

/// How a value is written.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Unit {
    /// With an SI prefix before this unit symbol.
    Si(&'static str),
    /// Without a prefix, followed by this suffix.
    Plain(&'static str),
}

impl Unit {
    /// `value` in this unit, to the decimals that resolve `resolution`, with
    /// the SI prefix that suits `magnitude`.
    fn format(self, value: f64, resolution: f64, magnitude: f64) -> String {
        match self {
            Unit::Si(symbol) => {
                si::format(value, resolution, magnitude, symbol)
            }
            Unit::Plain(suffix) => si::plain(value, resolution, suffix),
        }
    }

    /// The unit's bare symbol.
    fn symbol(self) -> &'static str {
        match self {
            Unit::Si(symbol) => symbol,
            Unit::Plain(suffix) => suffix.trim(),
        }
    }
}

impl Quantity {
    /// A grid line's label at `value`, in plot coordinates, among lines
    /// `spacing` apart across `range`.
    fn tick(self, value: f64, spacing: f64, range: (f64, f64)) -> String {
        if self.log {
            let value = 10f64.powf(value);
            self.unit.format(value, value, value)
        } else {
            let magnitude = range.0.abs().max(range.1.abs());
            self.unit.format(value, spacing, magnitude)
        }
    }

    /// The readout of `value`, in plot coordinates, to about four significant
    /// digits, on a window whose grid lines are `spacing` apart.
    fn reading(self, value: f64, spacing: f64) -> String {
        let value = self.value(value);
        let floor = if self.log { 0.0 } else { spacing / 1e6 };
        let resolution = (value.abs() / 1000.0).max(floor);
        self.unit.format(
            value,
            resolution,
            value.abs().max(resolution * 1000.0),
        )
    }

    /// The value at `x`, in plot coordinates.
    fn value(self, x: f64) -> f64 {
        if self.log { 10f64.powf(x) } else { x }
    }
}

/// One curve of a view: a trace seen as one quantity.
///
/// Its x values ascend.
struct Channel {
    name: String,
    trace: usize,
    quantity: Quantity,
    source_x: Vec<f64>,
    x: Vec<f64>,
    source_y: Vec<f64>,
    y: Vec<f64>,
    points: Vec<PlotPoint>,
}

impl Channel {
    /// The channel of `trace`, named `name`, with `y` over `x` in plot coordinates.
    fn new(
        name: &str,
        trace: usize,
        quantity: Quantity,
        x: Vec<f64>,
        y: Vec<f64>,
    ) -> Self {
        let points = x
            .iter()
            .zip(&y)
            .map(|(&x, &y)| PlotPoint::new(x, y))
            .collect();
        Self {
            name: name.to_owned(),
            trace,
            quantity,
            source_x: x.clone(),
            x,
            source_y: y.clone(),
            y,
            points,
        }
    }

    /// The channel as a curve to measure.
    fn curve(&self) -> Curve<'_> {
        Curve::new(&self.x, &self.y)
    }

    /// Replaces the displayed y values without changing the source values.
    fn display(&mut self, y: Vec<f64>) {
        self.points = self
            .x
            .iter()
            .zip(&y)
            .map(|(&x, &y)| PlotPoint::new(x, y))
            .collect();
        self.y = y;
    }

    /// Shifts displayed x values by `offset` from their source values.
    fn shift_x(&mut self, offset: f64) {
        self.x = self.source_x.iter().map(|x| x - offset).collect();
        self.points = self
            .x
            .iter()
            .zip(&self.y)
            .map(|(&x, &y)| PlotPoint::new(x, y))
            .collect();
    }
}

/// Grid lines at 1 to 9 times each power of ten, on an axis of base-10 logarithms.
///
/// Decades are the strongest lines, then 2 and 5, then the rest.
fn decade_grid(input: GridInput) -> Vec<GridMark> {
    let (start, end) = input.bounds;
    if !(end - start).is_finite() || end - start > 30.0 {
        return Vec::new();
    }
    let mut marks = Vec::new();
    for decade in start.floor() as i32..=end.ceil() as i32 {
        for multiple in 1..=9 {
            let value = f64::from(decade) + f64::from(multiple).log10();
            if (start..=end).contains(&value) {
                let step_size = match multiple {
                    1 => 1.0,
                    2 | 5 => 0.3,
                    _ => 0.1,
                };
                marks.push(GridMark { value, step_size });
            }
        }
    }
    marks
}

/// Axis hints that name `quantity` and label its grid lines, `spacing` apart
/// on a linear axis.
fn axis(
    hints: AxisHints<'static>,
    quantity: Quantity,
    spacing: f64,
) -> AxisHints<'static> {
    hints.label(quantity.name).formatter(move |mark, range| {
        if !quantity.log && mark.step_size < spacing / 2.0 {
            return String::new();
        }
        quantity.tick(mark.value, spacing, (*range.start(), *range.end()))
    })
}

/// The fixed colour of the trace at `index`.
pub fn color(index: usize) -> Color32 {
    const PALETTE: [Color32; 8] = [
        Color32::from_rgb(86, 180, 233),
        Color32::from_rgb(230, 159, 0),
        Color32::from_rgb(0, 158, 115),
        Color32::from_rgb(240, 228, 66),
        Color32::from_rgb(0, 114, 178),
        Color32::from_rgb(213, 94, 0),
        Color32::from_rgb(204, 121, 167),
        Color32::from_rgb(160, 160, 160),
    ];
    PALETTE[index % PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use egui_plot::GridInput;

    use super::decade_grid;

    #[test]
    fn decade_grid_marks_each_multiple_once_and_decades_strongest() {
        let marks = decade_grid(GridInput {
            bounds: (0.0, 2.0),
            base_step_size: 0.01,
        });
        assert_eq!(marks.len(), 9 + 9 + 1);
        let decades: Vec<f64> = marks
            .iter()
            .filter(|mark| mark.step_size == 1.0)
            .map(|mark| mark.value)
            .collect();
        assert_eq!(decades, [0.0, 1.0, 2.0]);
        let fifty = marks
            .iter()
            .find(|mark| (mark.value - 50f64.log10()).abs() < 1e-12)
            .unwrap();
        assert_eq!(fifty.step_size, 0.3);
    }
}
