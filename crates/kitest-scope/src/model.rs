//! What the scope shows, without a window: the open capture, its views, and
//! which traces are shown, changed only through [`Command`]s.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use kitest_scope::{Capture, Data, FileError};
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
        let (names, view) = family_view(std::slice::from_ref(&capture));
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
        if self.view.can_trigger() {
            views.push(ViewName::Spectrum);
        }
        if self.view.can_reference() {
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
            Command::Harmonics { shown } => {
                let (view, _) = self.active_mut();
                view.set_harmonics(shown).map_err(Error::invalid)?;
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
        let same = capture.name == self.capture_name
            && std::mem::discriminant(&capture.data)
                == std::mem::discriminant(&self.family[0].data);
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
        let (names, view) = family_view(&self.family);
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
        self.capture_name = self.family[0].name.clone();
        self.shown = shown;
        self.names = names;
        if !same {
            self.derived = None;
        }
        let name = if same {
            name
        } else if initial_spectrum(&self.family[0]) {
            ViewName::Spectrum
        } else {
            ViewName::Primary
        };
        if same && self.derived.is_some() {
            self.rederive();
        } else {
            let _ = self.set_view(name);
        }
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
                    Error::failed("a spectrum needs a transient capture with a shown trace")
                })?)
            }
            ViewName::GroupDelay => {
                Some(self.view.group_delay(&self.shown).ok_or_else(|| {
                    Error::failed("a group delay needs an AC sweep with a shown trace")
                })?)
            }
        };
        self.derived = derived.map(|view| (name, view));
        Ok(())
    }

    /// Rebuilds the derived view after the primary one or the shown traces changed.
    fn rederive(&mut self) {
        let Some((name, old)) = &mut self.derived else {
            return;
        };
        let next = match name {
            ViewName::Spectrum => self.view.spectrum(&self.shown),
            ViewName::GroupDelay => self.view.group_delay(&self.shown),
            ViewName::Primary => None,
        };
        if let Some(next) = next {
            old.replace(next, &self.shown, false);
        } else {
            self.derived = None;
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

/// Builds each run independently, preserving its sampling grid.
fn family_view(captures: &[Capture]) -> (Vec<String>, plot::View) {
    let mut names = Vec::new();
    let mut combined: Option<plot::View> = None;
    for (run, capture) in captures.iter().enumerate() {
        let (run_names, mut view) = capture_view(capture);
        let suffix = capture
            .corner
            .as_deref()
            .map_or_else(String::new, |corner| format!(" [{corner}]"));
        view.name_run(run, names.len(), &suffix);
        names.extend(
            run_names.into_iter().map(|name| format!("{name}{suffix}")),
        );
        if let Some(combined) = &mut combined {
            combined.append(view);
        } else {
            combined = Some(view);
        }
    }
    (
        names,
        combined.expect("a scope family always has a capture"),
    )
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

    #[test]
    fn regression_family_preserves_each_grid_and_trigger() {
        let directory = tempfile::tempdir().unwrap();
        let save = |corner: &str, time: Vec<f64>| {
            let mut capture = Capture::new(
                "family",
                Data::Transient {
                    traces: vec![Trace {
                        name: "out".into(),
                        values: vec![0.0, 1.0, 0.0, 1.0][..time.len()].to_vec(),
                    }],
                    time,
                },
            );
            capture.corner = Some(corner.into());
            capture.expectations = vec![Expectation {
                trace: "out".into(),
                passed: true,
                message: "band".into(),
                region: Region::Band {
                    start: 0.0,
                    end: 1.0,
                    low: 0.0,
                    high: 1.0,
                },
            }];
            let path = directory.path().join(capture.file_name());
            capture.save(&path).unwrap();
            path
        };
        let a = save("a", vec![0.0, 1.0, 2.0, 3.0]);
        let b = save("b", vec![0.0, 2.0, 4.0]);
        let mut model = Model::load(&a).unwrap();
        model.open(&b).unwrap();
        let readings = model
            .apply(Command::Measure {
                trace: "out [b]".into(),
                quantity: None,
                measurements: vec![Measurement::Max],
                over: Some([0.0, 4.0]),
            })
            .unwrap();
        assert_eq!(readings["max"]["value"], 1.0);
        model
            .apply(Command::Trigger {
                trace: Some("out [a]".into()),
                edge: Edge::Rising,
                level: Some(0.5),
            })
            .unwrap();
        let expectations = model.state()["plot"]["expectations"].clone();
        assert_eq!(expectations[0]["region"]["start"], -0.5);
        assert_eq!(expectations[1]["region"]["start"], -1.0);
        let csv = model.primary().csv(model.shown());
        assert!(csv.contains("0.5,1,0.75\n"), "{csv}");
        model
            .apply(Command::Trigger {
                trace: None,
                edge: Edge::Rising,
                level: None,
            })
            .unwrap();
        assert_eq!(
            model.state()["plot"]["expectations"][1]["region"]["start"],
            0.0
        );
    }

    #[test]
    fn harmonics_commands_toggle_spectrum_and_preserve_rebuild_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = capture(directory.path(), Vec::new());
        let mut model = Model::load(&path).unwrap();
        assert!(model.apply(Command::Harmonics { shown: true }).is_err());
        assert!(model.state()["plot"].get("harmonics").is_none());
        model.set_view(ViewName::Spectrum).unwrap();
        assert_eq!(model.state()["plot"]["harmonics"], false);
        let expectations = model.state()["plot"]["expectations"].clone();
        let state = model.apply(Command::Harmonics { shown: true }).unwrap();
        assert_eq!(state["plot"]["harmonics"], true);
        assert_eq!(state["plot"]["expectations"], expectations);
        model.apply(Command::Fit).unwrap();
        assert_eq!(model.state()["plot"]["harmonics"], true);
        model.open(&path).unwrap();
        assert_eq!(model.state()["plot"]["harmonics"], true);
        let state = model.apply(Command::Harmonics { shown: false }).unwrap();
        assert_eq!(state["plot"]["harmonics"], false);
        model.set_view(ViewName::Primary).unwrap();
        assert!(model.state()["plot"].get("harmonics").is_none());
        assert!(model.apply(Command::Harmonics { shown: false }).is_err());
    }

    #[test]
    fn regression_derived_hide_and_reload_preserve_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = capture(directory.path(), Vec::new());
        let mut model = Model::load(&path).unwrap();
        model.set_view(ViewName::Spectrum).unwrap();
        model
            .apply(Command::Move {
                trace: "inv".into(),
                quantity: None,
                to: None,
            })
            .unwrap();
        model
            .apply(Command::Zoom {
                x: Some([500.0, 2000.0]),
                y: Some([-80.0, 10.0]),
                pane: 0,
            })
            .unwrap();
        model
            .apply(Command::Cursors {
                a: Some(1000.0),
                b: Some(1500.0),
            })
            .unwrap();
        model
            .apply(Command::Measurements {
                pane: 0,
                measurements: vec![Measurement::Max],
            })
            .unwrap();
        let before = model.state()["plot"].clone();
        model
            .apply(Command::Show {
                trace: "inv".into(),
                shown: false,
            })
            .unwrap();
        assert_eq!(model.state()["plot"]["cursors"], before["cursors"]);
        model.open(&path).unwrap();
        model
            .apply(Command::Show {
                trace: "inv".into(),
                shown: true,
            })
            .unwrap();
        let after = model.state()["plot"].clone();
        assert_eq!(after["x"], before["x"]);
        assert_eq!(after["panes"], before["panes"]);
        assert_eq!(after["cursors"], before["cursors"]);
    }

    #[test]
    fn regression_ac_family_preserves_independent_grids() {
        let make = |corner: &str, frequency: Vec<f64>| {
            let mut capture = Capture::new(
                "ac-family",
                Data::Ac {
                    traces: vec![kitest_scope::AcTrace {
                        name: "out".into(),
                        re: vec![2.0; frequency.len()],
                        im: vec![0.0; frequency.len()],
                    }],
                    frequency,
                },
            );
            capture.corner = Some(corner.into());
            capture
        };
        let captures = [
            make("a", vec![10.0, 100.0, 1000.0]),
            make("b", vec![1.0, 10.0, 100.0, 1000.0, 10000.0]),
        ];
        let (names, view) = super::family_view(&captures);
        assert_eq!(names, ["out [a]", "out [b]"]);
        let csv = view.csv(&[true; 2]);
        assert_eq!(csv.lines().count(), 6);
        let first: Vec<_> = csv.lines().nth(1).unwrap().split(',').collect();
        assert_eq!(first[0], "1");
        assert_eq!(first[1], "");
        assert_eq!(first[3], "");
        let delay = view.group_delay(&[true; 2]).unwrap();
        assert!(
            delay
                .measure(
                    "out [b]",
                    None,
                    &[Measurement::Min],
                    Some([1.0, 10000.0])
                )
                .is_ok()
        );
    }

    #[test]
    fn regression_spectrum_command_ignores_unavailable_hidden_corner() {
        let directory = tempfile::tempdir().unwrap();
        let save = |corner: &str, time: Vec<f64>, values: Vec<f64>| {
            let mut capture = Capture::new(
                "family",
                Data::Transient {
                    time,
                    traces: vec![Trace {
                        name: "out".into(),
                        values,
                    }],
                },
            );
            capture.corner = Some(corner.into());
            let path = directory.path().join(capture.file_name());
            capture.save(&path).unwrap();
            path
        };
        let long =
            save("long", vec![0.0, 1.0, 2.0, 3.0], vec![0.0, 1.0, 0.0, 1.0]);
        let short = save("short", vec![0.0, 1.0], vec![0.0, 1.0]);
        let mut model = Model::load(&long).unwrap();
        model.open(&short).unwrap();
        model
            .apply(Command::Show {
                trace: "out [short]".into(),
                shown: false,
            })
            .unwrap();
        model
            .apply(Command::Zoom {
                x: Some([2.0, 3.0]),
                y: None,
                pane: 0,
            })
            .unwrap();
        model
            .apply(Command::View {
                name: ViewName::Spectrum,
            })
            .unwrap();
        assert_eq!(
            model.state()["plot"]["panes"][0]["channels"],
            serde_json::json!(["out [long]"])
        );
        model
            .apply(Command::Cursors {
                a: Some(10.0),
                b: Some(20.0),
            })
            .unwrap();
        let before = model.state()["plot"]["cursors"].clone();
        model
            .apply(Command::Trigger {
                trace: Some("out [long]".into()),
                edge: Edge::Rising,
                level: Some(0.5),
            })
            .unwrap();
        assert_eq!(model.state()["plot"]["cursors"], before);
        model
            .apply(Command::Show {
                trace: "out [short]".into(),
                shown: true,
            })
            .unwrap();
        assert_eq!(
            model.state()["plot"]["panes"][0]["channels"],
            serde_json::json!(["out [long]", "out [short]"])
        );
    }
}
