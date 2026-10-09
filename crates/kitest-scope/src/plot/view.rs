//! A view: a stack of panes sharing one x axis, a hover line, and cursors.

use std::fmt::Write as _;
use std::ops::RangeInclusive;

use eframe::egui;

use super::measure::{Measurement, Value};
use super::pane::{Action, Pane, Shared};
use super::window::{Window, X_GRID_LINES};
use super::{Channel, Quantity, Unit};

/// What the panes of a view share: the x axis and its window, the hover
/// line, the cursors, and the markers measurements place.
pub(super) struct Link {
    pub quantity: Quantity,
    pub x: Window,
    pub hover: Option<f64>,
    pub a: Option<f64>,
    pub b: Option<f64>,
    pub markers: Vec<(usize, f64)>,
}

impl Link {
    /// The window measurements cover: between the cursors when both are
    /// placed, else the visible x range.
    pub fn over(&self) -> RangeInclusive<f64> {
        match (self.a, self.b) {
            (Some(a), Some(b)) if a != b => a.min(b)..=a.max(b),
            _ => self.x.range(),
        }
    }
}

/// A capture's channels, laid out in panes over one shared x axis.
pub struct View {
    kind: &'static str,
    link: Link,
    channels: Vec<Channel>,
    panes: Vec<Pane>,
}

impl View {
    /// A view of `channels` over `x`, one pane per group of channel indices.
    ///
    /// The channels share their x samples, and each group's channels share a quantity.
    pub(super) fn new(
        kind: &'static str,
        x: Quantity,
        channels: Vec<Channel>,
        groups: Vec<Vec<usize>>,
    ) -> Self {
        let shown = vec![
            true;
            channels
                .iter()
                .map(|channel| channel.trace + 1)
                .max()
                .unwrap_or(0)
        ];
        let panes = groups
            .into_iter()
            .filter(|group| !group.is_empty())
            .map(|group| Pane::new(group, &channels, &shown))
            .collect();
        Self {
            kind,
            link: Link {
                quantity: x,
                x: fit_x(&channels, &shown),
                hover: None,
                a: None,
                b: None,
                markers: Vec::new(),
            },
            channels,
            panes,
        }
    }

    /// Draws the cursor line and the panes, and applies their interactions.
    ///
    /// `shown` holds one flag per trace.
    pub fn show(&mut self, ui: &mut egui::Ui, shown: &[bool]) {
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            (self.link.a, self.link.b) = (None, None);
        }
        if let Some(line) = self.cursor_line() {
            ui.label(line);
        } else {
            ui.weak("Press A or B over a plot to place a cursor; right-click a plot for panes and measurements.");
        }
        self.link.markers = self.markers(shown);

        let quantities: Vec<Quantity> =
            self.panes.iter().map(|pane| pane.quantity).collect();
        let shared = Shared {
            channels: &self.channels,
            shown,
            panes: &quantities,
        };
        let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        let strips: f32 = self
            .panes
            .iter()
            .map(|pane| pane.strip_rows(&self.link, &shared) as f32 * row)
            .sum();
        let gaps =
            ui.spacing().item_spacing.y * (self.panes.len() as f32 + 1.0);
        let height = ((ui.available_height() - strips - gaps)
            / self.panes.len().max(1) as f32)
            .max(60.0);

        let mut hover = None;
        let mut actions = Vec::new();
        for (index, pane) in self.panes.iter_mut().enumerate() {
            let (hovered, pane_actions) =
                pane.show(ui, index, height, &mut self.link, &shared);
            hover = hover.or(hovered);
            actions.extend(pane_actions);
        }
        self.link.hover = hover;
        for action in actions {
            self.apply(action, shown);
        }
    }

    /// Applies one pane's request.
    fn apply(&mut self, action: Action, shown: &[bool]) {
        match action {
            Action::Fit => {
                self.link.x = fit_x(&self.channels, shown);
                for pane in &mut self.panes {
                    pane.fit(&self.channels, shown);
                }
            }
            Action::Move { channel, to } => {
                for pane in &mut self.panes {
                    pane.channels.retain(|&other| other != channel);
                }
                match to.and_then(|to| self.panes.get_mut(to)) {
                    Some(pane) => pane.channels.push(channel),
                    None => self.panes.push(Pane::new(
                        vec![channel],
                        &self.channels,
                        shown,
                    )),
                }
                self.panes.retain(|pane| !pane.channels.is_empty());
            }
        }
    }

    /// Where the -3 dB measurement of each shown channel places a marker.
    fn markers(&self, shown: &[bool]) -> Vec<(usize, f64)> {
        let over = self.link.over();
        let mut markers = Vec::new();
        for pane in self
            .panes
            .iter()
            .filter(|pane| pane.measurements.contains(&Measurement::HalfPower))
        {
            for channel in pane.shown_channels(&self.channels, shown) {
                if let Some(Value::Positions(points)) =
                    Measurement::HalfPower.of(channel.curve(), over.clone())
                {
                    markers
                        .extend(points.into_iter().map(|x| (channel.trace, x)));
                }
            }
        }
        markers
    }

    /// The cursors' positions and the span between them, if one is placed.
    fn cursor_line(&self) -> Option<String> {
        let x = self.link.quantity;
        let spacing = self.link.x.grid_spacing();
        let mut line = String::new();
        for (name, at) in [("A", self.link.a), ("B", self.link.b)] {
            if let Some(at) = at {
                let _ = write!(line, "{name} {}    ", x.reading(at, spacing));
            }
        }
        if let (Some(a), Some(b)) = (self.link.a, self.link.b) {
            if x.log {
                let ratio = x.value(b) / x.value(a);
                let _ = write!(
                    line,
                    "B / A {}",
                    Unit::Plain("").format(ratio, ratio.abs() / 1000.0, ratio)
                );
            } else {
                let span = b - a;
                let _ = write!(
                    line,
                    "B - A {}",
                    x.unit.format(span, span.abs() / 1000.0, span)
                );
                if span != 0.0 {
                    let rate = 1.0 / span.abs();
                    let _ = write!(
                        line,
                        ",  1/|B - A| {}",
                        Unit::Si("Hz").format(rate, rate / 1000.0, rate)
                    );
                }
            }
        }
        (!line.is_empty()).then(|| line.trim_end().to_owned())
    }

    /// A one-line summary of the capture and of the x grid spacing.
    pub fn status(&self) -> String {
        let x = self.link.quantity;
        let samples =
            self.channels.first().map_or(&[][..], |channel| &channel.x);
        let mut status = format!("{}, {} points", self.kind, samples.len());
        if let (Some(&first), Some(&last)) = (samples.first(), samples.last()) {
            let (first, last) = (x.value(first), x.value(last));
            let magnitude = first.abs().max(last.abs());
            let end = |value: f64| {
                if x.log {
                    x.unit.format(value, value, value)
                } else {
                    x.unit.format(value, (last - first) / 10.0, magnitude)
                }
            };
            let _ = write!(status, ", {} to {}", end(first), end(last));
        }
        if !x.log {
            let spacing = self.link.x.grid_spacing();
            let _ = write!(
                status,
                "; {}/div",
                x.unit.format(spacing, spacing, spacing)
            );
        }
        status
    }

    /// The shown channels as comma-separated values over the measurement
    /// window, one row per x sample.
    pub fn csv(&self, shown: &[bool]) -> String {
        let x = self.link.quantity;
        let columns: Vec<&Channel> = self
            .panes
            .iter()
            .flat_map(|pane| pane.shown_channels(&self.channels, shown))
            .collect();
        let mut csv = format!("{} ({})", x.name, x.unit.symbol());
        for channel in &columns {
            let _ = write!(
                csv,
                ",{} {} ({})",
                channel.name,
                channel.quantity.name,
                channel.quantity.unit.symbol()
            );
        }
        csv.push('\n');
        let over = self.link.over();
        let samples =
            self.channels.first().map_or(&[][..], |channel| &channel.x);
        for (index, &at) in samples
            .iter()
            .enumerate()
            .filter(|(_, at)| over.contains(at))
        {
            let _ = write!(csv, "{}", x.value(at));
            for channel in &columns {
                let _ = write!(csv, ",{}", channel.y[index]);
            }
            csv.push('\n');
        }
        csv
    }
}

/// The x window holding every shown channel, or every channel if none is shown.
fn fit_x(channels: &[Channel], shown: &[bool]) -> Window {
    let any_shown = channels
        .iter()
        .any(|channel| shown.get(channel.trace).copied().unwrap_or(false));
    let ends = channels
        .iter()
        .filter(|channel| {
            !any_shown || shown.get(channel.trace).copied().unwrap_or(false)
        })
        .flat_map(|channel| {
            channel
                .x
                .first()
                .into_iter()
                .chain(channel.x.last())
                .copied()
        });
    Window::fit_values(ends, 0.0, X_GRID_LINES)
}

#[cfg(test)]
mod tests {
    use super::super::measure::WAVEFORM;
    use super::super::{Channel, Quantity, Unit};
    use super::{Action, View};

    const TIME: Quantity = Quantity {
        name: "time",
        unit: Unit::Si("s"),
        log: false,
        measurements: &[],
    };
    const VOLTAGE: Quantity = Quantity {
        name: "voltage",
        unit: Unit::Si("V"),
        log: false,
        measurements: WAVEFORM,
    };

    fn view() -> View {
        let x = vec![0.0, 1.0, 2.0, 3.0];
        let channels = (0..3)
            .map(|trace| {
                Channel::new(
                    &format!("v{trace}"),
                    trace,
                    VOLTAGE,
                    x.clone(),
                    vec![trace as f64; 4],
                )
            })
            .collect();
        View::new("transient", TIME, channels, vec![vec![0, 1, 2]])
    }

    #[test]
    fn moving_a_channel_to_a_new_pane_and_back_leaves_no_empty_pane() {
        let mut view = view();
        let shown = [true; 3];
        view.apply(
            Action::Move {
                channel: 1,
                to: None,
            },
            &shown,
        );
        assert_eq!(view.panes.len(), 2);
        assert_eq!(view.panes[0].channels, [0, 2]);
        assert_eq!(view.panes[1].channels, [1]);
        view.apply(
            Action::Move {
                channel: 1,
                to: Some(0),
            },
            &shown,
        );
        assert_eq!(view.panes.len(), 1);
        assert_eq!(view.panes[0].channels, [0, 2, 1]);
    }

    #[test]
    fn measurements_cover_the_span_between_the_cursors() {
        let mut view = view();
        view.link.a = Some(2.5);
        view.link.b = Some(0.5);
        assert_eq!(view.link.over(), 0.5..=2.5);
        view.link.b = None;
        assert_eq!(view.link.over(), view.link.x.range());
    }

    #[test]
    fn csv_holds_the_shown_channels_inside_the_window() {
        let mut view = view();
        view.link.a = Some(0.5);
        view.link.b = Some(2.0);
        let csv = view.csv(&[true, false, true]);
        assert_eq!(
            csv,
            "time (s),v0 voltage (V),v2 voltage (V)\n1,0,2\n2,0,2\n"
        );
    }
}
