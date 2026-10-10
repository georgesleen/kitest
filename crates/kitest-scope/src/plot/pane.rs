//! A pane: one plot of channels sharing a y quantity, its navigation, and
//! its measurement strip.

use eframe::egui::{self, Align2, PointerButton, Stroke};
use egui_plot::{
    AxisHints, Line, LineStyle, Plot, PlotPoint, PlotPoints, PlotUi, Polygon,
    Text, VLine, uniform_grid_spacer,
};

use super::measure::{Measurement, Value};
use super::view::Link;
use super::window::{Window, Y_GRID_LINES, Y_MARGIN};
use super::{Channel, Quantity, Unit, axis, color, decade_grid, decimate};

/// The x zoom exponent per point of scroll.
const ZOOM_PER_POINT: f64 = 1.0 / 200.0;

/// The width, in points, kept for y axis labels, so stacked panes line up.
const Y_AXIS_WIDTH: f32 = 64.0;

/// What every pane of a view reads: the channels, which traces are shown,
/// and each pane's y quantity.
pub(super) struct Shared<'v> {
    pub channels: &'v [Channel],
    pub reference: &'v [Channel],
    pub shown: &'v [bool],
    pub panes: &'v [Quantity],
    pub expectations: &'v [kitest_scope::Expectation],
    pub horizontal_markers: &'v [(usize, f64, String)],
}

/// A change a pane asks its view to make.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Action {
    /// Fit every pane to its shown channels.
    Fit,
    /// Move `channel` into pane `to`, or into a new pane if `to` is `None`.
    Move { channel: usize, to: Option<usize> },
}

/// One plot: the channels in it, its y window, and what it measures.
pub(super) struct Pane {
    pub channels: Vec<usize>,
    pub quantity: Quantity,
    pub measurements: Vec<Measurement>,
    y: Window,
    box_start: Option<PlotPoint>,
}

impl Pane {
    /// A pane of `channels`, indices into `all`, fitted to the shown ones.
    ///
    /// `channels` is not empty, and its channels share a quantity.
    pub fn new(channels: Vec<usize>, all: &[Channel], shown: &[bool]) -> Self {
        let mut pane = Self {
            quantity: all[channels[0]].quantity,
            channels,
            measurements: Vec::new(),
            y: Window::fit(0.0, 1.0, Y_MARGIN, Y_GRID_LINES),
            box_start: None,
        };
        pane.fit(all, shown);
        pane
    }

    /// Fits the y window to the shown channels, or to all of them if none is shown.
    pub fn fit(&mut self, all: &[Channel], shown: &[bool]) {
        let mut channels: Vec<&Channel> =
            self.shown_channels(all, shown).collect();
        if channels.is_empty() {
            channels = self.channels.iter().map(|&index| &all[index]).collect();
        }
        let values = channels
            .into_iter()
            .flat_map(|channel| channel.y.iter().copied());
        self.y = Window::fit_values(values, Y_MARGIN, Y_GRID_LINES);
    }

    /// Keeps `old`'s measurement choices and, unless `refit`, its y window.
    pub fn preserve(&mut self, old: &Self, refit: bool) {
        self.measurements.clone_from(&old.measurements);
        if !refit {
            self.y = old.y;
        }
    }

    /// The pane's channels whose traces are shown.
    pub fn shown_channels<'c>(
        &self,
        all: &'c [Channel],
        shown: &'c [bool],
    ) -> impl Iterator<Item = &'c Channel> {
        self.channels.iter().map(move |&index| &all[index]).filter(
            move |channel| shown.get(channel.trace).copied().unwrap_or(false),
        )
    }

    /// The rows outside the plot: expectations and, when selected, measurements.
    pub fn strip_rows(&self, link: &Link, shared: &Shared<'_>) -> usize {
        let expectations = shared
            .expectations
            .iter()
            .filter(|expectation| {
                self.channels.iter().any(|&index| {
                    let channel = &shared.channels[index];
                    channel.name == expectation.trace
                        && shared
                            .shown
                            .get(channel.trace)
                            .copied()
                            .unwrap_or(false)
                })
            })
            .count();
        let cursors = link.a.is_some() && link.b.is_some();
        let measurements = if self.measurements.is_empty() && !cursors {
            0
        } else {
            1 + self.shown_channels(shared.channels, shared.shown).count()
        };
        expectations + measurements
    }

    /// Draws the pane `height` points tall, then its measurement strip, and
    /// returns the hovered x and the changes asked of the view.
    ///
    /// `index` is the pane's place in its view.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        height: f32,
        link: &mut Link,
        shared: &Shared<'_>,
    ) -> (Option<f64>, Vec<Action>) {
        let (x, y) = (link.quantity, self.quantity);
        let (x_spacing, y_spacing) =
            (link.x.grid_spacing(), self.y.grid_spacing());
        let columns = ui.available_width().round() as usize;
        let marker = ui.visuals().weak_text_color();
        ui.horizontal_wrapped(|ui| {
            ui.strong(self.quantity.name);
            ui.weak(format!(
                "{} / div",
                self.quantity.unit.format(y_spacing, y_spacing, y_spacing)
            ));
            for channel in self.shown_channels(shared.channels, shared.shown) {
                ui.colored_label(color(channel.trace), channel.name.as_str());
            }
        });
        for expectation in shared.expectations.iter().filter(|expectation| {
            self.channels.iter().any(|&index| {
                let channel = &shared.channels[index];
                channel.name == expectation.trace
                    && shared.shown.get(channel.trace).copied().unwrap_or(false)
            })
        }) {
            let (text, tint) = if expectation.passed {
                ("PASS", egui::Color32::GREEN)
            } else {
                ("FAIL", egui::Color32::RED)
            };
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(tint, egui::RichText::new(text).strong());
                ui.colored_label(tint, expectation.message.as_str());
            });
        }
        let mut plot = Plot::new(("pane", index))
            .grid_fade(0.8)
            .height(height)
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .allow_boxed_zoom(false)
            .allow_double_click_reset(false)
            .allow_axis_zoom_drag(false)
            .show_x(false)
            .show_y(false)
            .custom_x_axes(vec![
                axis(AxisHints::new_x(), x, x_spacing)
                    .label_spacing(40.0..=50.0),
            ])
            .custom_y_axes(vec![
                axis(AxisHints::new_y(), y, y_spacing)
                    .min_thickness(Y_AXIS_WIDTH),
            ])
            .y_grid_spacer(uniform_grid_spacer(move |_| {
                [y_spacing / 5.0, y_spacing, y_spacing]
            }));
        plot = if x.log {
            plot.x_grid_spacer(decade_grid)
        } else {
            plot.x_grid_spacer(uniform_grid_spacer(move |_| {
                [x_spacing / 5.0, x_spacing, x_spacing]
            }))
        };

        let mut actions = Vec::new();
        let response = plot.show(ui, |plot_ui| {
            if self.navigate(plot_ui, link) {
                actions.push(Action::Fit);
            }
            plot_ui.set_plot_bounds_x(link.x.range());
            plot_ui.set_plot_bounds_y(self.y.range());
            for reference in shared.reference.iter().filter(|reference| {
                self.channels.iter().any(|&index| {
                    let current = &shared.channels[index];
                    current.name == reference.name
                        && current.quantity == reference.quantity
                }) && shared
                    .shown
                    .get(reference.trace)
                    .copied()
                    .unwrap_or(false)
            }) {
                let points = decimate::visible(
                    &reference.points,
                    link.x.range(),
                    columns,
                );
                plot_ui.line(
                    Line::new("", points)
                        .color(color(reference.trace).gamma_multiply(0.28))
                        .width(1.0),
                );
            }
            let top = *self.y.range().end();
            for channel in self.shown_channels(shared.channels, shared.shown) {
                let points =
                    decimate::visible(&channel.points, link.x.range(), columns);
                plot_ui.line(
                    Line::new(channel.name.as_str(), points)
                        .color(color(channel.trace))
                        .width(1.5),
                );
            }
            for expectation in
                shared.expectations.iter().filter(|expectation| {
                    self.channels.iter().any(|&index| {
                        let channel = &shared.channels[index];
                        channel.name == expectation.trace
                            && shared
                                .shown
                                .get(channel.trace)
                                .copied()
                                .unwrap_or(false)
                    })
                })
            {
                if expectation.region.is_spectral() != x.log {
                    continue;
                }
                let tint = if expectation.passed {
                    egui::Color32::GREEN
                } else {
                    egui::Color32::RED
                };
                let limit = |plot_ui: &mut PlotUi<'_>,
                             y: f64,
                             from: f64,
                             to: f64,
                             label: &str| {
                    plot_ui.line(
                        Line::new(
                            "",
                            PlotPoints::new(vec![[from, y], [to, y]]),
                        )
                        .color(tint)
                        .width(2.0),
                    );
                    plot_ui.text(
                        Text::new("", PlotPoint::new(from, y), label)
                            .color(tint)
                            .anchor(Align2::LEFT_BOTTOM),
                    );
                };
                let (start, end, low, high) = match expectation.region {
                    kitest_scope::Region::Band {
                        start,
                        end,
                        low,
                        high,
                    } => (start, end, low, high),
                    kitest_scope::Region::Frequency { low, high }
                        if low > 0.0 && high > low =>
                    {
                        (
                            low.log10(),
                            high.log10(),
                            *self.y.range().start(),
                            *self.y.range().end(),
                        )
                    }
                    kitest_scope::Region::Swing {
                        start,
                        end,
                        centre,
                        minimum,
                    } => {
                        limit(
                            plot_ui,
                            centre + minimum,
                            start,
                            end,
                            "min swing",
                        );
                        limit(
                            plot_ui,
                            centre - minimum,
                            start,
                            end,
                            "min swing",
                        );
                        continue;
                    }
                    kitest_scope::Region::Distortion {
                        fundamental,
                        amplitude,
                        maximum,
                    } if self.quantity.name == "spectrum"
                        && fundamental > 0.0
                        && amplitude * maximum > 0.0 =>
                    {
                        let level = 20.0 * (amplitude * maximum).log10();
                        let from = (1.5 * fundamental).log10();
                        limit(
                            plot_ui,
                            level,
                            from,
                            *link.x.range().end(),
                            "THD limit",
                        );
                        continue;
                    }
                    _ => continue,
                };
                plot_ui.polygon(
                    Polygon::new(
                        "",
                        PlotPoints::new(vec![
                            [start, low],
                            [end, low],
                            [end, high],
                            [start, high],
                        ]),
                    )
                    .stroke(Stroke::new(1.0, tint))
                    .style(LineStyle::dashed_dense())
                    .fill_color(tint.gamma_multiply(0.12)),
                );
            }
            for (trace, at, label) in &link.markers {
                if shared.shown.get(*trace).copied().unwrap_or(false) {
                    plot_ui.vline(
                        VLine::new("", *at)
                            .color(color(*trace))
                            .style(LineStyle::dashed_dense()),
                    );
                    plot_ui.text(
                        Text::new("", PlotPoint::new(*at, top), label.as_str())
                            .color(color(*trace))
                            .anchor(Align2::LEFT_TOP),
                    );
                }
            }
            for (trace, at, label) in shared.horizontal_markers {
                if !shared.shown.get(*trace).copied().unwrap_or(false) {
                    continue;
                }
                plot_ui.hline(
                    egui_plot::HLine::new("", *at)
                        .color(color(*trace).gamma_multiply(0.65))
                        .style(LineStyle::dotted_dense()),
                );
                plot_ui.text(
                    Text::new(
                        "",
                        PlotPoint::new(*link.x.range().start(), *at),
                        label.as_str(),
                    )
                    .color(color(*trace))
                    .anchor(Align2::LEFT_BOTTOM),
                );
            }
            for (name, at) in [("A", link.a), ("B", link.b)] {
                if let Some(at) = at {
                    plot_ui.vline(
                        VLine::new("", at)
                            .color(marker)
                            .style(LineStyle::dashed_loose()),
                    );
                    plot_ui.text(
                        Text::new("", PlotPoint::new(at, top), name)
                            .color(marker)
                            .anchor(Align2::RIGHT_TOP),
                    );
                }
            }
            let hovered = plot_ui
                .pointer_coordinate()
                .filter(|_| plot_ui.response().hovered());
            if let (Some(start), Some(end)) =
                (self.box_start, plot_ui.pointer_coordinate())
            {
                let corners = vec![
                    [start.x, start.y],
                    [end.x, start.y],
                    [end.x, end.y],
                    [start.x, end.y],
                ];
                plot_ui.polygon(
                    Polygon::new("", PlotPoints::new(corners))
                        .stroke(Stroke::new(1.0, marker))
                        .fill_color(marker.gamma_multiply(0.1)),
                );
            } else if let Some(at) = hovered.map(|point| point.x).or(link.hover)
            {
                plot_ui.vline(VLine::new("", at).color(marker));
            }
            hovered
        });

        let hovered = response.inner.map(|point| point.x);
        if let Some(at) = hovered.or(link.hover) {
            let top = PlotPoint::new(at, *self.y.range().end());
            let anchor = match hovered {
                Some(_) => ui.ctx().pointer_latest_pos(),
                None => Some(response.transform.position_from_point(&top)),
            }
            .unwrap_or_else(|| response.response.rect.left_top());
            self.readout(ui, index, anchor, at, link, shared);
        }
        response
            .response
            .context_menu(|ui| self.menu(ui, index, shared, &mut actions));
        self.strip(ui, index, link, shared);
        (hovered, actions)
    }

    /// Applies the pointer's scroll, drag, keys, and double-click to `link`
    /// and the pane, and returns whether a double-click asks for a fit.
    ///
    /// Scroll zooms x and Ctrl+scroll zooms y. A middle drag or a Ctrl+left
    /// drag pans, a right drag zooms to a box, and A or B places a cursor.
    fn navigate(&mut self, plot_ui: &mut PlotUi<'_>, link: &mut Link) -> bool {
        let response = plot_ui.response().clone();
        let pointer = plot_ui.pointer_coordinate();
        let (scroll, zoom, command, press, key_a, key_b) =
            plot_ui.ctx().input(|input| {
                (
                    input.smooth_scroll_delta.y,
                    input.zoom_delta(),
                    input.modifiers.command,
                    input.pointer.press_origin(),
                    input.key_pressed(egui::Key::A),
                    input.key_pressed(egui::Key::B),
                )
            });
        if let Some(pointer) = pointer
            && response.hovered()
        {
            link.x
                .zoom((f64::from(scroll) * ZOOM_PER_POINT).exp(), pointer.x);
            self.y.zoom(f64::from(zoom), pointer.y);
            if key_a {
                link.a = Some(pointer.x);
            }
            if key_b {
                link.b = Some(pointer.x);
            }
        }
        if response.dragged_by(PointerButton::Middle)
            || (response.dragged_by(PointerButton::Primary) && command)
        {
            let delta = plot_ui.pointer_coordinate_drag_delta();
            link.x.pan(-f64::from(delta.x));
            self.y.pan(-f64::from(delta.y));
        }
        if response.drag_started_by(PointerButton::Secondary)
            && let Some(press) = press
        {
            self.box_start = Some(plot_ui.plot_from_screen(press));
        }
        if response.drag_stopped_by(PointerButton::Secondary)
            && let (Some(start), Some(end)) = (self.box_start.take(), pointer)
        {
            link.x.zoom_to(start.x, end.x);
            self.y.zoom_to(start.y, end.y);
        }
        response.double_clicked()
    }

    /// Shows each shown channel's value at `at` in a box beside `anchor`.
    fn readout(
        &self,
        ui: &egui::Ui,
        index: usize,
        anchor: egui::Pos2,
        at: f64,
        link: &Link,
        shared: &Shared<'_>,
    ) {
        let (x, y) = (link.quantity, self.quantity);
        let (x_spacing, y_spacing) =
            (link.x.grid_spacing(), self.y.grid_spacing());
        egui::Area::new(egui::Id::new(("readout", index)))
            .order(egui::Order::Foreground)
            .interactable(false)
            .fixed_pos(anchor + egui::vec2(16.0, 16.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.label(x.reading(at, x_spacing));
                    egui::Grid::new(("readout values", index))
                        .num_columns(2)
                        .show(ui, |ui| {
                            for channel in self
                                .shown_channels(shared.channels, shared.shown)
                            {
                                if let Some(value) = channel.curve().at(at) {
                                    ui.colored_label(
                                        color(channel.trace),
                                        channel.name.as_str(),
                                    );
                                    ui.colored_label(
                                        color(channel.trace),
                                        y.reading(value, y_spacing),
                                    );
                                    ui.end_row();
                                }
                            }
                        });
                });
            });
    }

    /// The right-click menu: move a channel to another pane, and pick measurements.
    fn menu(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        shared: &Shared<'_>,
        actions: &mut Vec<Action>,
    ) {
        for &channel in &self.channels {
            ui.menu_button(shared.channels[channel].name.as_str(), |ui| {
                if self.channels.len() > 1
                    && ui.button("Move to a new pane").clicked()
                {
                    actions.push(Action::Move { channel, to: None });
                    ui.close();
                }
                for (other, &quantity) in shared.panes.iter().enumerate() {
                    if other != index
                        && quantity == self.quantity
                        && ui
                            .button(format!("Move to pane {}", other + 1))
                            .clicked()
                    {
                        actions.push(Action::Move {
                            channel,
                            to: Some(other),
                        });
                        ui.close();
                    }
                }
            });
        }
        ui.separator();
        ui.menu_button("Measurements", |ui| {
            for &measurement in self.quantity.measurements {
                let mut on = self.measurements.contains(&measurement);
                if ui.checkbox(&mut on, measurement.label()).changed() {
                    if on {
                        self.measurements.push(measurement);
                    } else {
                        self.measurements.retain(|&other| other != measurement);
                    }
                }
            }
        });
        if ui.button("Fit").clicked() {
            actions.push(Action::Fit);
            ui.close();
        }
    }

    /// The table under the plot: each shown channel's picked measurements
    /// over the measurement window, and its change from cursor A to B.
    fn strip(
        &self,
        ui: &mut egui::Ui,
        index: usize,
        link: &Link,
        shared: &Shared<'_>,
    ) {
        let cursors = link.a.zip(link.b);
        if self.measurements.is_empty() && cursors.is_none() {
            return;
        }
        let over = link.over();
        egui::Grid::new(("strip", index))
            .num_columns(self.measurements.len() + 2)
            .show(ui, |ui| {
                ui.label("");
                for measurement in &self.measurements {
                    ui.strong(measurement.label());
                }
                if cursors.is_some() {
                    ui.strong("B - A");
                }
                ui.end_row();
                for channel in
                    self.shown_channels(shared.channels, shared.shown)
                {
                    let tint = color(channel.trace);
                    ui.colored_label(tint, channel.name.as_str());
                    for measurement in &self.measurements {
                        let text = measurement
                            .of(channel.curve(), over.clone())
                            .map_or_else(
                                || "-".to_owned(),
                                |value| self.write(&value, link),
                            );
                        ui.colored_label(tint, text);
                    }
                    if let Some((a, b)) = cursors {
                        let curve = channel.curve();
                        let text = curve.at(b).zip(curve.at(a)).map_or_else(
                            || "-".to_owned(),
                            |(b, a)| self.write(&Value::Level(b - a), link),
                        );
                        ui.colored_label(tint, text);
                    }
                    ui.end_row();
                }
            });
    }

    /// A measurement's value written in its unit.
    fn write(&self, value: &Value, link: &Link) -> String {
        match value {
            Value::Level(level) => {
                self.quantity.reading(*level, self.y.grid_spacing())
            }
            Value::Duration(duration) => link.quantity.unit.format(
                *duration,
                duration.abs() / 1000.0,
                *duration,
            ),
            Value::Rate(rate) => {
                Unit::Si("Hz").format(*rate, rate.abs() / 1000.0, *rate)
            }
            Value::Positions(positions) => positions
                .iter()
                .map(|&at| link.quantity.reading(at, link.x.grid_spacing()))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}
