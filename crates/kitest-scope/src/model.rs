//! What the scope shows, without a window: the open capture, its views, and
//! which traces are shown, changed only through [`Command`]s.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use kitest_scope::{AcTrace, Capture, Data, FileError, Trace};
use serde_json::{Value as Json, json};

use crate::control::{Command, Edge, Error, Format, ViewName};
use crate::plot;

/// The open capture and how it is shown.
pub struct Model {
    path: PathBuf,
    capture_name: String,
    family: Vec<Capture>,
    names: Vec<String>,
    shown: Vec<bool>,
    view: plot::View,
    derived: Option<(ViewName, plot::View)>,
    modified: Option<SystemTime>,
}

impl Model {
    /// The capture at `path`, shown in the instrument its expectations suit.
    ///
    /// # Errors
    ///
    /// When the capture cannot be read.
    pub fn load(path: &Path) -> Result<Self, FileError> {
        let capture = Capture::load(path)?;
        let (names, view) = capture_view(&capture);
        let mut model = Self {
            path: path.to_path_buf(),
            capture_name: capture.name.clone(),
            shown: vec![true; names.len()],
            names,
            view,
            derived: None,
            modified: modified(path),
            family: vec![capture],
        };
        if initial_spectrum(&model.family[0]) {
            let _ = model.set_view(ViewName::Spectrum);
        }
        Ok(model)
    }

    /// The open capture's file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The open capture's file name.
    pub fn file(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    /// The traces, by name, in capture order.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Which traces are shown, one flag per trace.
    pub fn shown(&self) -> &[bool] {
        &self.shown
    }

    /// The instrument on show.
    pub fn view_name(&self) -> ViewName {
        self.derived
            .as_ref()
            .map_or(ViewName::Primary, |(name, _)| *name)
    }

    /// The capture's own view, which triggers and references change.
    pub fn primary(&self) -> &plot::View {
        &self.view
    }

    /// The view on show, with the flags of the traces it shows.
    pub fn active_mut(&mut self) -> (&mut plot::View, &[bool]) {
        match &mut self.derived {
            Some((_, view)) => (view, &self.shown),
            None => (&mut self.view, &self.shown),
        }
    }

    /// The view on show.
    pub fn active(&self) -> &plot::View {
        self.derived.as_ref().map_or(&self.view, |(_, view)| view)
    }

    /// Whether the open capture's file changed since it was read.
    pub fn changed_on_disk(&self) -> bool {
        modified(&self.path) > self.modified
    }

    /// The instruments the capture can be shown in.
    pub fn views(&self) -> Vec<ViewName> {
        let mut views = vec![ViewName::Primary];
        if self.view.spectrum(&self.shown).is_some() {
            views.push(ViewName::Spectrum);
        }
        if self.view.group_delay(&self.shown).is_some() {
            views.push(ViewName::GroupDelay);
        }
        views
    }

    /// Carries out `command` and returns its result.
    ///
    /// Most commands return the new state. `measure` returns its readings,
    /// and `save` the file it wrote.
    ///
    /// # Errors
    ///
    /// When the command does not fit the capture, or it fails. A PNG needs
    /// a window, so a model alone refuses one.
    pub fn apply(&mut self, command: Command) -> Result<Json, Error> {
        match command {
            Command::Open { path } => self.open(&path)?,
            Command::State => {}
            Command::View { name } => self.set_view(name)?,
            Command::Show { trace, shown } => {
                let index = self.trace(&trace)?;
                self.shown[index] = shown;
                self.rederive();
            }
            Command::Zoom { x, y, pane } => {
                let (view, _) = self.active_mut();
                if let Some(x) = x {
                    view.zoom_x(x).map_err(Error::invalid)?;
                }
                if let Some(y) = y {
                    view.zoom_y(pane, y).map_err(Error::invalid)?;
                }
            }
            Command::Fit => {
                let (view, shown) = self.active_mut();
                let shown = shown.to_vec();
                view.fit(&shown);
            }
            Command::Cursors { a, b } => {
                let (view, _) = self.active_mut();
                view.set_cursors(a, b).map_err(Error::invalid)?;
            }
            Command::Measure {
                trace,
                quantity,
                measurements,
                over,
            } => {
                let readings = self
                    .active()
                    .measure(&trace, quantity.as_deref(), &measurements, over)
                    .map_err(Error::invalid)?;
                return Ok(json!(readings));
            }
            Command::Measurements { pane, measurements } => {
                let (view, _) = self.active_mut();
                view.set_measurements(pane, measurements)
                    .map_err(Error::invalid)?;
            }
            Command::Move {
                trace,
                quantity,
                to,
            } => {
                let (view, shown) = self.active_mut();
                let shown = shown.to_vec();
                view.move_channel(&trace, quantity.as_deref(), to, &shown)
                    .map_err(Error::invalid)?;
            }
            Command::Reference { trace } => {
                if !self.view.can_reference() {
                    return Err(Error::failed(
                        "only a Bode view has a reference trace",
                    ));
                }
                let trace =
                    trace.map(|trace| self.trace(&trace)).transpose()?;
                self.view.set_reference(trace, &self.shown);
                self.rederive();
            }
            Command::Trigger { trace, edge, level } => {
                if !self.view.can_trigger() {
                    return Err(Error::failed(
                        "only a transient view has a trigger",
                    ));
                }
                let rising = edge == Edge::Rising;
                match (trace, level) {
                    (None, _) => self.view.clear_trigger(&self.shown),
                    (Some(trace), Some(level)) => {
                        let trace = self.trace(&trace)?;
                        self.view.set_trigger_level(
                            trace,
                            level,
                            rising,
                            &self.shown,
                        );
                    }
                    (Some(trace), None) => {
                        if !self.view.can_set_trigger() {
                            return Err(Error::invalid(
                                "give a level, or place cursor A at the trigger level",
                            ));
                        }
                        let trace = self.trace(&trace)?;
                        self.view.set_trigger(trace, rising, &self.shown);
                    }
                }
                self.rederive();
            }
            Command::Save { format, path } => {
                return match format {
                    Format::Csv => {
                        std::fs::write(&path, self.active().csv(&self.shown))
                            .map_err(|error| {
                            Error::failed(format!(
                                "{}: {error}",
                                path.display()
                            ))
                        })?;
                        Ok(json!({ "path": path }))
                    }
                    Format::Png => {
                        Err(Error::failed("a PNG needs the scope window"))
                    }
                };
            }
        }
        Ok(self.state())
    }

    /// What the scope shows, as the `state` method reports it.
    pub fn state(&self) -> Json {
        json!({
            "path": self.path,
            "capture": self.capture_name,
            "view": self.view_name(),
            "views": self.views(),
            "traces": self
                .names
                .iter()
                .zip(&self.shown)
                .map(|(name, shown)| json!({ "name": name, "shown": shown }))
                .collect::<Vec<_>>(),
            "plot": self.active().state(&self.shown),
        })
    }

    /// Opens the capture at `path`, keeping layout, cursors, and zoom when it
    /// shares the open capture's name, and overlaying it as another corner.
    ///
    /// # Errors
    ///
    /// When the capture cannot be read.
    pub fn open(&mut self, path: &Path) -> Result<(), Error> {
        let capture = Capture::load(path).map_err(|error| {
            Error::failed(format!("{}: {error}", path.display()))
        })?;
        let same = capture.name == self.capture_name;
        if same {
            match self
                .family
                .iter()
                .position(|old| old.corner == capture.corner)
            {
                Some(index) => self.family[index] = capture,
                None => self.family.push(capture),
            }
        } else {
            self.family = vec![capture];
        }
        let capture = family_capture(&self.family);
        let (names, view) = capture_view(&capture);
        let shown: Vec<bool> = names
            .iter()
            .map(|name| {
                self.names
                    .iter()
                    .position(|old| old == name)
                    .and_then(|index| self.shown.get(index).copied())
                    .unwrap_or(true)
            })
            .collect();
        let name = self.view_name();
        if same {
            self.view.replace(view, &shown, false);
        } else {
            self.view = view;
        }
        self.modified = modified(path);
        self.path = path.to_path_buf();
        self.capture_name = capture.name.clone();
        self.shown = shown;
        self.names = names;
        self.derived = None;
        let name = if same {
            name
        } else if initial_spectrum(&capture) {
            ViewName::Spectrum
        } else {
            ViewName::Primary
        };
        let _ = self.set_view(name);
        Ok(())
    }

    /// Shows the instrument `name`.
    ///
    /// # Errors
    ///
    /// When the capture cannot be shown in it.
    pub fn set_view(&mut self, name: ViewName) -> Result<(), Error> {
        let derived = match name {
            ViewName::Primary => None,
            ViewName::Spectrum => {
                Some(self.view.spectrum(&self.shown).ok_or_else(|| {
                    Error::failed("only a transient capture has a spectrum")
                })?)
            }
            ViewName::GroupDelay => {
                Some(self.view.group_delay(&self.shown).ok_or_else(|| {
                    Error::failed("only an AC sweep has a group delay")
                })?)
            }
        };
        self.derived = derived.map(|view| (name, view));
        Ok(())
    }

    /// Rebuilds the derived view after the primary one or the shown traces changed.
    fn rederive(&mut self) {
        if let Some((name, _)) = self.derived {
            let _ = self
                .set_view(name)
                .or_else(|_| self.set_view(ViewName::Primary));
        }
    }

    /// The index of the trace named `name`.
    fn trace(&self, name: &str) -> Result<usize, Error> {
        self.names
            .iter()
            .position(|known| known == name)
            .ok_or_else(|| {
                Error::invalid(format!(
                    "no trace {name}; the capture has {}",
                    self.names.join(", ")
                ))
            })
    }
}

/// The capture's trace names and plot view.
fn capture_view(capture: &Capture) -> (Vec<String>, plot::View) {
    match &capture.data {
        Data::Transient { time, traces } => (
            traces.iter().map(|trace| trace.name.clone()).collect(),
            plot::time::view(time, traces, &capture.expectations),
        ),
        Data::Ac { frequency, traces } => (
            traces.iter().map(|trace| trace.name.clone()).collect(),
            plot::bode::view(frequency, traces, &capture.expectations),
        ),
    }
}

/// Captures of the same name as one capture, with each corner appended to its
/// trace names.
fn family_capture(captures: &[Capture]) -> Capture {
    let first = captures
        .first()
        .expect("a scope family always has a capture");
    if captures.len() == 1 {
        return first.clone();
    }
    let suffix = |capture: &Capture| {
        capture
            .corner
            .as_deref()
            .map_or_else(String::new, |corner| format!(" [{corner}]"))
    };
    let mut expectations = Vec::new();
    let data = match &first.data {
        Data::Transient { time, .. } => {
            let mut traces = Vec::new();
            for capture in captures {
                let Data::Transient {
                    traces: capture_traces,
                    ..
                } = &capture.data
                else {
                    continue;
                };
                let suffix = suffix(capture);
                traces.extend(capture_traces.iter().map(|trace| Trace {
                    name: format!("{}{suffix}", trace.name),
                    values: trace.values.clone(),
                }));
                expectations.extend(capture.expectations.iter().cloned().map(
                    |mut expectation| {
                        expectation.trace =
                            format!("{}{suffix}", expectation.trace);
                        expectation
                    },
                ));
            }
            Data::Transient {
                time: time.clone(),
                traces,
            }
        }
        Data::Ac { frequency, .. } => {
            let mut traces = Vec::new();
            for capture in captures {
                let Data::Ac {
                    traces: capture_traces,
                    ..
                } = &capture.data
                else {
                    continue;
                };
                let suffix = suffix(capture);
                traces.extend(capture_traces.iter().map(|trace| AcTrace {
                    name: format!("{}{suffix}", trace.name),
                    re: trace.re.clone(),
                    im: trace.im.clone(),
                }));
                expectations.extend(capture.expectations.iter().cloned().map(
                    |mut expectation| {
                        expectation.trace =
                            format!("{}{suffix}", expectation.trace);
                        expectation
                    },
                ));
            }
            Data::Ac {
                frequency: frequency.clone(),
                traces,
            }
        }
    };
    Capture {
        name: first.name.clone(),
        corner: None,
        data,
        expectations,
    }
}

/// Whether a capture opens on its spectrum: when every expectation lies on a
/// frequency axis.
fn initial_spectrum(capture: &Capture) -> bool {
    !capture.expectations.is_empty()
        && capture
            .expectations
            .iter()
            .all(|expectation| expectation.region.is_spectral())
}

/// `path`'s last modification time, if it can be read.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

#[cfg(test)]
mod tests {
    use kitest_scope::{Capture, Data, Expectation, Region, Trace};

    use super::Model;
    use crate::control::{Command, Edge, ViewName};
    use crate::plot::Measurement;

    /// A 1 kHz sine and its inverse, saved as a capture in `directory`.
    fn capture(
        directory: &std::path::Path,
        expectations: Vec<Expectation>,
    ) -> std::path::PathBuf {
        let time: Vec<f64> = (0..5000).map(|i| f64::from(i) * 1e-6).collect();
        let sine: Vec<f64> = time
            .iter()
            .map(|t| (std::f64::consts::TAU * 1e3 * t).sin())
            .collect();
        let mut capture = Capture::new(
            "osc",
            Data::Transient {
                time,
                traces: vec![
                    Trace {
                        name: "out".into(),
                        values: sine.clone(),
                    },
                    Trace {
                        name: "inv".into(),
                        values: sine.iter().map(|v| -v).collect(),
                    },
                ],
            },
        );
        capture.expectations = expectations;
        let path = directory.join(capture.file_name());
        capture.save(&path).unwrap();
        path
    }

    #[test]
    fn measure_reads_the_named_trace_over_the_given_window_in_real_units() {
        let directory = tempfile::tempdir().unwrap();
        let mut model =
            Model::load(&capture(directory.path(), Vec::new())).unwrap();
        let readings = model
            .apply(Command::Measure {
                trace: "out".into(),
                quantity: None,
                measurements: vec![
                    Measurement::PeakToPeak,
                    Measurement::Frequency,
                ],
                over: Some([0.0, 4.999e-3]),
            })
            .unwrap();
        let peak = readings["peak_to_peak"]["value"].as_f64().unwrap();
        let frequency = readings["frequency"]["value"].as_f64().unwrap();
        assert!((peak - 2.0).abs() < 1e-3, "{readings}");
        assert!((frequency - 1e3).abs() < 1.0, "{readings}");
        assert_eq!(readings["frequency"]["unit"], "Hz");
    }

    #[test]
    fn commands_that_do_not_fit_say_why() {
        let directory = tempfile::tempdir().unwrap();
        let mut model =
            Model::load(&capture(directory.path(), Vec::new())).unwrap();
        let mut message = |command| model.apply(command).unwrap_err().message;
        assert!(
            message(Command::Show {
                trace: "nope".into(),
                shown: false
            })
            .contains("out, inv")
        );
        assert!(
            message(Command::View {
                name: ViewName::GroupDelay
            })
            .contains("AC sweep")
        );
        assert!(message(Command::Reference { trace: None }).contains("Bode"));
        assert!(
            message(Command::Trigger {
                trace: Some("out".into()),
                edge: Edge::Rising,
                level: None
            })
            .contains("cursor A")
        );
        assert!(
            message(Command::Measurements {
                pane: 0,
                measurements: vec![Measurement::HalfPower]
            })
            .contains("cannot measure")
        );
    }

    #[test]
    fn state_follows_zoom_cursors_and_hidden_traces() {
        let directory = tempfile::tempdir().unwrap();
        let mut model =
            Model::load(&capture(directory.path(), Vec::new())).unwrap();
        model
            .apply(Command::Zoom {
                x: Some([2e-4, 6e-4]),
                y: None,
                pane: 0,
            })
            .unwrap();
        model
            .apply(Command::Cursors {
                a: Some(3e-4),
                b: None,
            })
            .unwrap();
        let state = model
            .apply(Command::Show {
                trace: "inv".into(),
                shown: false,
            })
            .unwrap();
        let range = state["plot"]["x"]["range"].as_array().unwrap();
        assert!((range[0].as_f64().unwrap() - 2e-4).abs() < 1e-12, "{state}");
        assert_eq!(state["plot"]["cursors"][0], 3e-4);
        assert_eq!(
            state["plot"]["panes"][0]["channels"],
            serde_json::json!(["out"])
        );
        assert_eq!(state["traces"][1]["shown"], false);
    }

    #[test]
    fn a_capture_of_spectral_checks_opens_on_its_spectrum_and_can_go_back() {
        let directory = tempfile::tempdir().unwrap();
        let frequency = Expectation {
            trace: "out".into(),
            passed: true,
            message: "in band".into(),
            region: Region::Frequency {
                low: 900.0,
                high: 1100.0,
            },
        };
        let mut model =
            Model::load(&capture(directory.path(), vec![frequency])).unwrap();
        assert_eq!(model.view_name(), ViewName::Spectrum);
        let state = model
            .apply(Command::View {
                name: ViewName::Primary,
            })
            .unwrap();
        assert_eq!(state["view"], "primary");
        assert_eq!(state["plot"]["x"]["quantity"], "time");
    }
}
