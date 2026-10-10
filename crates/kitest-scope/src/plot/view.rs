//! A view: a stack of panes sharing one x axis, a hover line, and cursors.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::ops::RangeInclusive;

use eframe::egui;
use serde::Serialize;

use super::measure::{Measurement, Value};
use super::pane::{Action, Pane, Shared};
use super::window::{Window, X_GRID_LINES};
use super::{Channel, Quantity, Unit};

/// A trace, level, and edge that align each run to time zero.
#[derive(Debug, Clone, Copy)]
struct Trigger {
    trace: usize,
    level: f64,
    rising: bool,
}

/// What the panes of a view share: the x axis and its window, the hover
/// line, the cursors, and the markers measurements place.
pub(super) struct Link {
    pub quantity: Quantity,
    pub x: Window,
    pub hover: Option<f64>,
    pub a: Option<f64>,
    pub b: Option<f64>,
    pub markers: Vec<(usize, f64, String)>,
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
    reference: Vec<Channel>,
    panes: Vec<Pane>,
    expectations: Vec<kitest_scope::Expectation>,
    fixed_markers: Vec<(usize, f64, String)>,
    horizontal_markers: Vec<(usize, f64, String)>,
    reference_trace: Option<usize>,
    annotations: Vec<String>,
    trigger: Option<Trigger>,
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
        expectations: &[kitest_scope::Expectation],
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
            reference: Vec::new(),
            panes,
            expectations: expectations.to_vec(),
            fixed_markers: Vec::new(),
            horizontal_markers: Vec::new(),
            reference_trace: None,
            annotations: Vec::new(),
            trigger: None,
        }
    }

    /// Replaces the channels with `next`, keeping layout, cursors, and zoom
    /// when both views have the same axes, and keeping this run as reference.
    pub fn replace(&mut self, mut next: Self, shown: &[bool], refit: bool) {
        if self.kind != next.kind || self.link.quantity != next.link.quantity {
            *self = next;
            return;
        }
        next.reference = std::mem::take(&mut self.channels);
        next.trigger = self.trigger;
        next.apply_trigger(refit);
        next.link.a = self.link.a;
        next.link.b = self.link.b;
        next.link.hover = self.link.hover;
        if !refit {
            next.link.x = self.link.x;
        }

        let mut panes = Vec::new();
        for old in &self.panes {
            let indices: Vec<usize> = old
                .channels
                .iter()
                .filter_map(|&index| {
                    let channel = &next.reference[index];
                    next.channels.iter().position(|candidate| {
                        candidate.name == channel.name
                            && candidate.quantity == channel.quantity
                    })
                })
                .collect();
            if !indices.is_empty() {
                let mut pane = Pane::new(indices, &next.channels, shown);
                pane.preserve(old, refit);
                panes.push(pane);
            }
        }
        if !panes.is_empty() {
            next.panes = panes;
        }
        *self = next;
    }

    /// A spectrum view of this transient view's shown channels over the
    /// visible time window, or `None` for another kind of view.
    pub fn spectrum(&self, shown: &[bool]) -> Option<Self> {
        if self.kind != "transient" {
            return None;
        }
        let frequency = Quantity {
            name: "frequency",
            unit: Unit::Si("Hz"),
            log: true,
            measurements: &[],
        };
        let magnitude = Quantity {
            name: "spectrum",
            unit: Unit::Plain(" dBV"),
            log: false,
            measurements: super::measure::MAGNITUDE,
        };
        let over = self.link.x.range();
        let mut channels = Vec::new();
        let mut markers = Vec::new();
        let mut horizontal_markers = Vec::new();
        for channel in self.channels.iter().filter(|channel| {
            shown.get(channel.trace).copied().unwrap_or(false)
        }) {
            let samples = channel
                .x
                .iter()
                .filter(|at| over.contains(at))
                .count()
                .next_power_of_two()
                .clamp(256, 65_536);
            let spectrum = channel.curve().spectrum(over.clone(), samples)?;
            let bin_width = spectrum.bin_width();
            let x: Vec<f64> = (1..spectrum.amplitudes().len())
                .map(|bin| (bin as f64 * bin_width).log10())
                .collect();
            let y: Vec<f64> = spectrum
                .amplitudes()
                .iter()
                .skip(1)
                .map(|amplitude| {
                    20.0 * amplitude.max(f64::MIN_POSITIVE).log10()
                })
                .collect();
            let noise =
                20.0 * spectrum.noise_floor().max(f64::MIN_POSITIVE).log10();
            horizontal_markers.push((
                channel.trace,
                noise,
                format!("noise floor {:.1} dBV", noise),
            ));
            if let Some(tone) = spectrum.dominant() {
                for harmonic in 1..=8 {
                    let hertz = tone.hertz * harmonic as f64;
                    if hertz <= 10f64.powf(*x.last()?) {
                        let label = if harmonic == 1 {
                            frequency.reading(hertz.log10(), 0.0)
                        } else {
                            format!("{harmonic}f")
                        };
                        markers.push((channel.trace, hertz.log10(), label));
                    }
                }
            }
            channels.push(Channel::new(
                &channel.name,
                channel.trace,
                magnitude,
                x,
                y,
            ));
        }
        if channels.is_empty() {
            return None;
        }
        let all = (0..channels.len()).collect();
        let mut view = Self::new(
            "spectrum",
            frequency,
            channels,
            vec![all],
            &self.expectations,
        );
        view.fixed_markers = markers;
        view.horizontal_markers = horizontal_markers;
        Some(view)
    }

    /// Whether this transient view can align runs to a trigger.
    pub fn can_trigger(&self) -> bool {
        self.kind == "transient"
    }

    /// Whether cursor A supplies a level for a trigger.
    pub fn can_set_trigger(&self) -> bool {
        self.can_trigger() && self.link.a.is_some()
    }

    /// Aligns the first `trace` crossing of cursor A's level to time zero.
    pub fn set_trigger(&mut self, trace: usize, rising: bool, shown: &[bool]) {
        let Some(at) = self.link.a else { return };
        let Some(level) = self
            .channels
            .iter()
            .find(|channel| channel.trace == trace)
            .and_then(|channel| channel.curve().at(at))
        else {
            return;
        };
        self.set_trigger_level(trace, level, rising, shown);
    }

    /// Aligns the first `trace` crossing of `level`, on the given edge, to
    /// time zero.
    pub fn set_trigger_level(
        &mut self,
        trace: usize,
        level: f64,
        rising: bool,
        shown: &[bool],
    ) {
        self.trigger = Some(Trigger {
            trace,
            level,
            rising,
        });
        self.apply_trigger(true);
        for pane in &mut self.panes {
            pane.fit(&self.channels, shown);
        }
    }

    /// Clears the trigger and restores the capture's absolute time axis.
    pub fn clear_trigger(&mut self, shown: &[bool]) {
        self.trigger = None;
        for channel in &mut self.channels {
            channel.shift_x(0.0);
        }
        self.link.x = fit_x(&self.channels, shown);
        self.link.a = None;
    }

    /// The trigger as a short status line.
    pub fn trigger_line(&self) -> Option<String> {
        let trigger = self.trigger?;
        let channel = self
            .channels
            .iter()
            .find(|channel| channel.trace == trigger.trace)?;
        Some(format!(
            "trigger {} {} through {}",
            channel.name,
            if trigger.rising { "rising" } else { "falling" },
            channel
                .quantity
                .reading(trigger.level, trigger.level.abs().max(1e-12),)
        ))
    }

    /// Applies the trigger to the current channels.
    fn apply_trigger(&mut self, refit: bool) {
        let Some(trigger) = self.trigger else { return };
        let Some(channel) = self
            .channels
            .iter()
            .find(|channel| channel.trace == trigger.trace)
        else {
            return;
        };
        let Some((&first, &last)) =
            channel.source_x.first().zip(channel.source_x.last())
        else {
            return;
        };
        let curve =
            kitest_measure::Curve::new(&channel.source_x, &channel.source_y);
        let Some(crossing) = curve
            .crossings(trigger.level, first..=last)
            .into_iter()
            .find(|crossing| crossing.rising == trigger.rising)
        else {
            return;
        };
        for channel in &mut self.channels {
            channel.shift_x(crossing.x);
        }
        if refit {
            let shown = vec![
                true;
                self.channels
                    .iter()
                    .map(|channel| channel.trace + 1)
                    .max()
                    .unwrap_or(0)
            ];
            self.link.x = fit_x(&self.channels, &shown);
        }
        self.link.a = Some(0.0);
    }

    /// Whether this view can divide its responses by a reference trace.
    pub fn can_reference(&self) -> bool {
        self.kind == "AC sweep"
    }

    /// The trace the responses are divided by, or `None` for absolute responses.
    pub fn reference_trace(&self) -> Option<usize> {
        self.reference_trace
    }

    /// Divides every magnitude and phase response by `trace`, or restores the
    /// absolute responses for `None`.
    pub fn set_reference(&mut self, trace: Option<usize>, shown: &[bool]) {
        if !self.can_reference() {
            return;
        }
        for quantity in ["magnitude", "phase"] {
            let reference = trace.and_then(|trace| {
                self.channels
                    .iter()
                    .find(|channel| {
                        channel.trace == trace
                            && channel.quantity.name == quantity
                    })
                    .map(|channel| channel.source_y.clone())
            });
            for channel in self
                .channels
                .iter_mut()
                .filter(|channel| channel.quantity.name == quantity)
            {
                let y = match &reference {
                    Some(reference) => channel
                        .source_y
                        .iter()
                        .zip(reference)
                        .map(|(value, reference)| value - reference)
                        .collect(),
                    None => channel.source_y.clone(),
                };
                channel.display(y);
            }
        }
        self.reference_trace = trace;
        self.update_margins(shown);
        for pane in &mut self.panes {
            pane.fit(&self.channels, shown);
        }
    }

    /// A group-delay view of every shown phase response, or `None` for a
    /// non-Bode view.
    pub fn group_delay(&self, shown: &[bool]) -> Option<Self> {
        if self.kind != "AC sweep" {
            return None;
        }
        let delay = Quantity {
            name: "group delay",
            unit: Unit::Si("s"),
            log: false,
            measurements: super::measure::WAVEFORM,
        };
        let mut channels = Vec::new();
        for phase in self.channels.iter().filter(|channel| {
            channel.quantity.name == "phase"
                && shown.get(channel.trace).copied().unwrap_or(false)
        }) {
            let frequency: Vec<f64> =
                phase.x.iter().map(|x| 10f64.powf(*x)).collect();
            let values = kitest_measure::group_delay(&frequency, &phase.y);
            channels.push(Channel::new(
                &phase.name,
                phase.trace,
                delay,
                phase.x.clone(),
                values,
            ));
        }
        if channels.is_empty() {
            return None;
        }
        let all = (0..channels.len()).collect();
        Some(Self::new(
            "group delay",
            self.link.quantity,
            channels,
            vec![all],
            &[],
        ))
    }

    /// Recomputes the selected transfer functions' gain and phase margins.
    fn update_margins(&mut self, shown: &[bool]) {
        self.annotations.clear();
        self.fixed_markers.clear();
        for (trace, &shown) in shown.iter().enumerate() {
            if !shown || Some(trace) == self.reference_trace {
                continue;
            }
            let magnitude = self.channels.iter().find(|channel| {
                channel.trace == trace && channel.quantity.name == "magnitude"
            });
            let phase = self.channels.iter().find(|channel| {
                channel.trace == trace && channel.quantity.name == "phase"
            });
            let (Some(magnitude), Some(phase)) = (magnitude, phase) else {
                continue;
            };
            if let Some((at, margin)) =
                kitest_measure::gain_margin(magnitude.curve(), phase.curve())
            {
                self.fixed_markers.push((trace, at, "GM".to_owned()));
                self.annotations.push(format!(
                    "{} gain margin {:.2} dB at {}",
                    magnitude.name,
                    margin,
                    self.link.quantity.reading(at, self.link.x.grid_spacing())
                ));
            }
            if let Some((at, margin)) =
                kitest_measure::phase_margin(magnitude.curve(), phase.curve())
            {
                self.fixed_markers.push((trace, at, "PM".to_owned()));
                self.annotations.push(format!(
                    "{} phase margin {:.2} deg at {}",
                    magnitude.name,
                    margin,
                    self.link.quantity.reading(at, self.link.x.grid_spacing())
                ));
            }
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
        for annotation in &self.annotations {
            ui.weak(annotation);
        }
        self.link.markers = self.markers(shown);

        let quantities: Vec<Quantity> =
            self.panes.iter().map(|pane| pane.quantity).collect();
        let shared = Shared {
            channels: &self.channels,
            reference: &self.reference,
            shown,
            panes: &quantities,
            expectations: &self.expectations,
            horizontal_markers: &self.horizontal_markers,
        };
        let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        let strips: f32 = self
            .panes
            .iter()
            .map(|pane| {
                (pane.strip_rows(&self.link, &shared) as f32 + 1.0) * row
            })
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
    fn markers(&self, shown: &[bool]) -> Vec<(usize, f64, String)> {
        let over = self.link.over();
        let mut markers = self.fixed_markers.clone();
        for pane in self
            .panes
            .iter()
            .filter(|pane| pane.measurements.contains(&Measurement::HalfPower))
        {
            for channel in pane.shown_channels(&self.channels, shown) {
                if let Some(Value::Positions(points)) =
                    Measurement::HalfPower.of(channel.curve(), over.clone())
                {
                    markers.extend(points.into_iter().map(|x| {
                        let label = self
                            .link
                            .quantity
                            .reading(x, self.link.x.grid_spacing());
                        (channel.trace, x, label)
                    }));
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

/// What a view shows, in real units, for a client of the command protocol.
#[derive(Debug, Serialize)]
pub struct State {
    /// The kind of view, such as `transient`, `AC sweep`, or `spectrum`.
    pub kind: &'static str,
    pub x: Axis,
    pub panes: Vec<PaneState>,
    /// The cursors on the x axis, in its unit.
    pub cursors: [Option<f64>; 2],
    pub trigger: Option<String>,
    /// The trace the responses are divided by, by name.
    pub reference: Option<String>,
    /// Margins and similar readings the view prints.
    pub annotations: Vec<String>,
    pub expectations: Vec<kitest_scope::Expectation>,
}

/// One axis: what it measures and the window it shows, in its unit.
#[derive(Debug, Serialize)]
pub struct Axis {
    pub quantity: &'static str,
    pub unit: &'static str,
    pub log: bool,
    pub range: [f64; 2],
}

/// One pane: its y axis, the channels in it, and what it measures.
#[derive(Debug, Serialize)]
pub struct PaneState {
    pub y: Axis,
    pub channels: Vec<String>,
    pub measurements: Vec<Measurement>,
}

/// One measurement's result, or `null` when the window does not define it.
#[derive(Debug, Serialize)]
pub struct Reading {
    pub value: ReadingValue,
    pub unit: &'static str,
}

/// A reading's number, or its places for a measurement that finds several.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ReadingValue {
    One(f64),
    Many(Vec<f64>),
}

impl View {
    /// What the view shows, in real units.
    ///
    /// `shown` holds one flag per trace.
    pub fn state(&self, shown: &[bool]) -> State {
        let x = self.link.quantity;
        let real = |at: Option<f64>| at.map(|at| x.value(at));
        State {
            kind: self.kind,
            x: Axis {
                quantity: x.name,
                unit: x.unit.symbol(),
                log: x.log,
                range: self.real_range(self.link.x.range()),
            },
            panes: self
                .panes
                .iter()
                .map(|pane| PaneState {
                    y: Axis {
                        quantity: pane.quantity.name,
                        unit: pane.quantity.unit.symbol(),
                        log: false,
                        range: [*pane.y.range().start(), *pane.y.range().end()],
                    },
                    channels: pane
                        .shown_channels(&self.channels, shown)
                        .map(|channel| channel.name.clone())
                        .collect(),
                    measurements: pane.measurements.clone(),
                })
                .collect(),
            cursors: [real(self.link.a), real(self.link.b)],
            trigger: self.trigger_line(),
            reference: self.reference_trace.and_then(|trace| {
                self.channels
                    .iter()
                    .find(|channel| channel.trace == trace)
                    .map(|channel| channel.name.clone())
            }),
            annotations: self.annotations.clone(),
            expectations: self.expectations.clone(),
        }
    }

    /// Shows `range` on the x axis, in its unit.
    ///
    /// # Errors
    ///
    /// When an end lies off a log axis or the ends are equal.
    pub fn zoom_x(&mut self, range: [f64; 2]) -> Result<(), String> {
        let [a, b] = [self.plot(range[0])?, self.plot(range[1])?];
        if a == b {
            return Err("an x range needs two different ends".to_owned());
        }
        self.link.x.zoom_to(a, b);
        Ok(())
    }

    /// Shows `range` on pane `pane`'s y axis, in its unit.
    ///
    /// # Errors
    ///
    /// When the view has no such pane or the ends are equal.
    pub fn zoom_y(
        &mut self,
        pane: usize,
        range: [f64; 2],
    ) -> Result<(), String> {
        let count = self.panes.len();
        let pane = self.panes.get_mut(pane).ok_or_else(|| {
            format!("there is no pane {pane}; the view has {count}")
        })?;
        if range[0] == range[1] {
            return Err("a y range needs two different ends".to_owned());
        }
        pane.y.zoom_to(range[0], range[1]);
        Ok(())
    }

    /// Fits the x axis and every pane to the shown traces.
    pub fn fit(&mut self, shown: &[bool]) {
        self.apply(Action::Fit, shown);
    }

    /// Places cursors A and B in the x axis's unit, removing any that is `None`.
    ///
    /// # Errors
    ///
    /// When a position lies off a log axis.
    pub fn set_cursors(
        &mut self,
        a: Option<f64>,
        b: Option<f64>,
    ) -> Result<(), String> {
        let a = a.map(|a| self.plot(a)).transpose()?;
        let b = b.map(|b| self.plot(b)).transpose()?;
        (self.link.a, self.link.b) = (a, b);
        Ok(())
    }

    /// Each of `measurements` of the channel of `trace`, over `over` in the x
    /// axis's unit, or else between the cursors or across the visible window.
    ///
    /// `quantity` picks among channels of one trace, such as `phase`; the
    /// trace's first channel is measured without it.
    ///
    /// # Errors
    ///
    /// When no channel matches, or a measurement does not apply to its quantity.
    pub fn measure(
        &self,
        trace: &str,
        quantity: Option<&str>,
        measurements: &[Measurement],
        over: Option<[f64; 2]>,
    ) -> Result<BTreeMap<Measurement, Option<Reading>>, String> {
        let channel = self.channel(trace, quantity)?;
        let over = match over {
            Some([a, b]) => {
                let (a, b) = (self.plot(a)?, self.plot(b)?);
                a.min(b)..=a.max(b)
            }
            None => self.link.over(),
        };
        let x = self.link.quantity;
        let mut readings = BTreeMap::new();
        for &measurement in measurements {
            if !channel.quantity.measurements.contains(&measurement) {
                return Err(format!(
                    "{} cannot be measured for {}; it takes {}",
                    measurement.label(),
                    channel.quantity.name,
                    names(channel.quantity.measurements)
                ));
            }
            let reading =
                measurement.of(channel.curve(), over.clone()).map(|value| {
                    match value {
                        Value::Level(level) => Reading {
                            value: ReadingValue::One(level),
                            unit: channel.quantity.unit.symbol(),
                        },
                        Value::Duration(duration) => Reading {
                            value: ReadingValue::One(duration),
                            unit: x.unit.symbol(),
                        },
                        Value::Rate(rate) => Reading {
                            value: ReadingValue::One(rate),
                            unit: "Hz",
                        },
                        Value::Positions(positions) => Reading {
                            value: ReadingValue::Many(
                                positions
                                    .into_iter()
                                    .map(|at| x.value(at))
                                    .collect(),
                            ),
                            unit: x.unit.symbol(),
                        },
                    }
                });
            readings.insert(measurement, reading);
        }
        Ok(readings)
    }

    /// Sets what pane `pane` measures and shows in its strip.
    ///
    /// # Errors
    ///
    /// When the view has no such pane, or a measurement does not apply to it.
    pub fn set_measurements(
        &mut self,
        pane: usize,
        measurements: Vec<Measurement>,
    ) -> Result<(), String> {
        let count = self.panes.len();
        let pane = self.panes.get_mut(pane).ok_or_else(|| {
            format!("there is no pane {pane}; the view has {count}")
        })?;
        if let Some(other) = measurements.iter().find(|measurement| {
            !pane.quantity.measurements.contains(measurement)
        }) {
            return Err(format!(
                "a {} pane cannot measure {}; it takes {}",
                pane.quantity.name,
                other.label(),
                names(pane.quantity.measurements)
            ));
        }
        pane.measurements = measurements;
        Ok(())
    }

    /// Moves the channel of `trace` into pane `to`, or into a new pane for `None`.
    ///
    /// # Errors
    ///
    /// When no channel matches, the pane does not exist, or it holds another quantity.
    pub fn move_channel(
        &mut self,
        trace: &str,
        quantity: Option<&str>,
        to: Option<usize>,
        shown: &[bool],
    ) -> Result<(), String> {
        let channel = self.channel(trace, quantity)?;
        let index = self
            .channels
            .iter()
            .position(|other| std::ptr::eq(other, channel))
            .expect("the channel is the view's own");
        if let Some(to) = to {
            let pane = self.panes.get(to).ok_or_else(|| {
                format!(
                    "there is no pane {to}; the view has {}",
                    self.panes.len()
                )
            })?;
            if pane.quantity != channel.quantity {
                return Err(format!(
                    "pane {to} holds {}, not {}",
                    pane.quantity.name, channel.quantity.name
                ));
            }
        }
        self.apply(Action::Move { channel: index, to }, shown);
        Ok(())
    }

    /// The channel of `trace`, the one of `quantity` when given.
    fn channel(
        &self,
        trace: &str,
        quantity: Option<&str>,
    ) -> Result<&Channel, String> {
        self.channels
            .iter()
            .find(|channel| {
                channel.name == trace
                    && quantity.is_none_or(|quantity| {
                        channel.quantity.name == quantity
                    })
            })
            .ok_or_else(|| {
                let mut known: Vec<String> = self
                    .channels
                    .iter()
                    .map(|channel| {
                        format!("{} ({})", channel.name, channel.quantity.name)
                    })
                    .collect();
                known.dedup();
                format!(
                    "no trace {trace} here; the view has {}",
                    known.join(", ")
                )
            })
    }

    /// `value`, in the x axis's unit, in plot coordinates.
    fn plot(&self, value: f64) -> Result<f64, String> {
        if !value.is_finite() {
            return Err(format!("{value} is not a finite position"));
        }
        if self.link.quantity.log {
            if value <= 0.0 {
                return Err(format!(
                    "{value} lies off the log {} axis",
                    self.link.quantity.name
                ));
            }
            Ok(value.log10())
        } else {
            Ok(value)
        }
    }

    /// `range`, in plot coordinates, in the x axis's unit.
    fn real_range(&self, range: RangeInclusive<f64>) -> [f64; 2] {
        let x = self.link.quantity;
        [x.value(*range.start()), x.value(*range.end())]
    }
}

/// The protocol names of `measurements`, comma separated.
fn names(measurements: &[Measurement]) -> String {
    measurements
        .iter()
        .map(|measurement| {
            serde_json::to_value(measurement)
                .ok()
                .and_then(|name| name.as_str().map(str::to_owned))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(", ")
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
        View::new("transient", TIME, channels, vec![vec![0, 1, 2]], &[])
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

    #[test]
    fn trigger_aligns_the_selected_crossing_to_zero() {
        let channel = Channel::new(
            "clk",
            0,
            VOLTAGE,
            vec![0.0, 1.0, 2.0, 3.0],
            vec![0.0, 1.0, 0.0, 1.0],
        );
        let mut view =
            View::new("transient", TIME, vec![channel], vec![vec![0]], &[]);
        view.link.a = Some(0.5);
        view.set_trigger(0, true, &[true]);
        assert_eq!(view.channels[0].x, [-0.5, 0.5, 1.5, 2.5]);
        assert_eq!(view.link.a, Some(0.0));
        assert!(view.trigger_line().unwrap().contains("clk rising"));
        view.clear_trigger(&[true]);
        assert_eq!(view.channels[0].x, [0.0, 1.0, 2.0, 3.0]);
    }
}
