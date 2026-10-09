//! `kitest-scope FILE`: open a scope file in a window.

mod live;
mod plot;
mod save;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use eframe::egui;
use kitest_scope::Capture;

use save::{Choice, Dialog, Format};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (Some(path), None) = (args.next(), args.next()) else {
        eprintln!("usage: kitest-scope FILE");
        return ExitCode::from(2);
    };
    let path = PathBuf::from(path);
    let (paths, socket) = match live::connect_or_listen(&path) {
        Ok(live::Instance::Forwarded) => return ExitCode::SUCCESS,
        Ok(live::Instance::Owner { paths, _socket }) => (paths, _socket),
        Err(error) => {
            eprintln!("error: single scope window: {error}");
            return ExitCode::from(1);
        }
    };
    let capture = match Capture::load(&path) {
        Ok(capture) => capture,
        Err(error) => {
            eprintln!("error: {}: {error}", path.display());
            return ExitCode::from(2);
        }
    };

    let file = file_name(&path);
    let app = Scope::new(path, file, capture, paths, socket);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_app_id("kitest-scope"),
        ..Default::default()
    };
    match eframe::run_native(
        "kitest scope",
        options,
        Box::new(|_| Ok(Box::new(app))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// A view derived from the capture's primary waveform or Bode response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Derived {
    Spectrum,
    GroupDelay,
}

/// The window: the capture's view, which traces are shown, and the state
/// of a save.
struct Scope {
    path: PathBuf,
    file: String,
    capture_name: String,
    family: Vec<Capture>,
    names: Vec<String>,
    view: plot::View,
    alternate: Option<plot::View>,
    derived: Option<Derived>,
    shown: Vec<bool>,
    dialog: Option<Dialog>,
    screenshot_to: Option<PathBuf>,
    message: Option<String>,
    modified: Option<std::time::SystemTime>,
    paths: std::sync::mpsc::Receiver<PathBuf>,
    _socket: live::Socket,
}

impl Scope {
    fn new(
        path: PathBuf,
        file: String,
        capture: Capture,
        paths: std::sync::mpsc::Receiver<PathBuf>,
        socket: live::Socket,
    ) -> Self {
        let (names, view) = capture_view(&capture);
        let shown = vec![true; names.len()];
        let derived = initial_derived(&capture);
        let alternate = match derived {
            Some(Derived::Spectrum) => view.spectrum(&shown),
            Some(Derived::GroupDelay) => view.group_delay(&shown),
            None => None,
        };
        Self {
            modified: modified(&path),
            path,
            file,
            capture_name: capture.name.clone(),
            family: vec![capture],
            shown,
            names,
            view,
            alternate,
            derived,
            dialog: None,
            screenshot_to: None,
            message: None,
            paths,
            _socket: socket,
        }
    }

    /// Loads every capture later launches handed to this window.
    fn receive_paths(&mut self, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = self.paths.try_iter().collect();
        for path in paths {
            self.open(path, ctx);
        }
    }

    /// Reloads the current capture when its file changed.
    fn reload(&mut self, ctx: &egui::Context) {
        let modified = modified(&self.path);
        if modified > self.modified {
            self.open(self.path.clone(), ctx);
        }
    }

    /// Opens `path` in this window, preserving state for the same capture.
    fn open(&mut self, path: PathBuf, ctx: &egui::Context) {
        match Capture::load(&path) {
            Ok(capture) => {
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
                if same {
                    self.view.replace(view, &shown, false);
                } else {
                    self.view = view;
                }
                if !same {
                    self.derived = initial_derived(&capture);
                }
                self.alternate = match self.derived {
                    Some(Derived::Spectrum) => self.view.spectrum(&shown),
                    Some(Derived::GroupDelay) => self.view.group_delay(&shown),
                    None => None,
                };
                if self.alternate.is_none() {
                    self.derived = None;
                }
                self.modified = modified(&path);
                self.path = path;
                self.file = file_name(&self.path);
                self.capture_name = capture.name;
                self.shown = shown;
                self.names = names;
                self.message = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                    "kitest scope: {}",
                    self.file
                )));
            }
            Err(error) => {
                self.message =
                    Some(format!("could not open {}: {error}", path.display()));
            }
        }
    }

    /// Carries out the save the dialog asked for.
    fn save(&mut self, ctx: &egui::Context, path: PathBuf, format: Format) {
        match format {
            Format::Csv => {
                let view = self.alternate.as_ref().unwrap_or(&self.view);
                let result = std::fs::write(&path, view.csv(&self.shown));
                self.report(&path, result.map_err(save::SaveError::from));
            }
            Format::Png => {
                self.screenshot_to = Some(path);
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                    egui::UserData::default(),
                ));
            }
        }
    }

    /// Writes a screenshot that arrived this frame to the file waiting for it.
    fn receive_screenshot(&mut self, ctx: &egui::Context) {
        let image = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let (Some(image), Some(path)) = (image, self.screenshot_to.take()) {
            let result = save::write_png(&path, &image);
            self.report(&path, result);
        }
    }

    /// Records the outcome of a save for the status bar.
    fn report(&mut self, path: &Path, result: Result<(), save::SaveError>) {
        self.message = Some(match result {
            Ok(()) => format!("saved {}", path.display()),
            Err(error) => format!("could not save {}: {error}", path.display()),
        });
    }
}

/// The capture's trace names and plot view.
fn capture_view(capture: &Capture) -> (Vec<String>, plot::View) {
    match &capture.data {
        kitest_scope::Data::Transient { time, traces } => (
            traces.iter().map(|trace| trace.name.clone()).collect(),
            plot::time::view(time, traces, &capture.expectations),
        ),
        kitest_scope::Data::Ac { frequency, traces } => (
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
        kitest_scope::Data::Transient { time, .. } => {
            let mut traces = Vec::new();
            for capture in captures {
                let kitest_scope::Data::Transient {
                    traces: capture_traces,
                    ..
                } = &capture.data
                else {
                    continue;
                };
                let suffix = suffix(capture);
                traces.extend(capture_traces.iter().map(|trace| {
                    kitest_scope::Trace {
                        name: format!("{}{suffix}", trace.name),
                        values: trace.values.clone(),
                    }
                }));
                expectations.extend(capture.expectations.iter().cloned().map(
                    |mut expectation| {
                        expectation.trace =
                            format!("{}{suffix}", expectation.trace);
                        expectation
                    },
                ));
            }
            kitest_scope::Data::Transient {
                time: time.clone(),
                traces,
            }
        }
        kitest_scope::Data::Ac { frequency, .. } => {
            let mut traces = Vec::new();
            for capture in captures {
                let kitest_scope::Data::Ac {
                    traces: capture_traces,
                    ..
                } = &capture.data
                else {
                    continue;
                };
                let suffix = suffix(capture);
                traces.extend(capture_traces.iter().map(|trace| {
                    kitest_scope::AcTrace {
                        name: format!("{}{suffix}", trace.name),
                        re: trace.re.clone(),
                        im: trace.im.clone(),
                    }
                }));
            }
            kitest_scope::Data::Ac {
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

/// The instrument an expectation selects when a capture first opens.
fn initial_derived(capture: &Capture) -> Option<Derived> {
    let frequency = capture.expectations.iter().any(|expectation| {
        matches!(expectation.region, kitest_scope::Region::Frequency { .. })
    });
    let band = capture.expectations.iter().any(|expectation| {
        matches!(expectation.region, kitest_scope::Region::Band { .. })
    });
    (frequency && !band).then_some(Derived::Spectrum)
}

/// `path`'s file name, or its display when it has none.
fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// `path`'s last modification time, if it can be read.
fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

impl eframe::App for Scope {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.receive_paths(&ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        self.reload(&ctx);
        self.receive_screenshot(&ctx);
        if ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::S)
        }) {
            self.dialog = Some(Dialog::new(&self.path, Format::Png));
        }
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    for (format, label) in
                        [(Format::Png, "Save PNG…"), (Format::Csv, "Save CSV…")]
                    {
                        if ui.button(label).clicked() {
                            self.dialog = Some(Dialog::new(&self.path, format));
                            ui.close();
                        }
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui
                        .selectable_label(self.derived.is_none(), "Primary")
                        .clicked()
                    {
                        self.derived = None;
                        self.alternate = None;
                        ui.close();
                    }
                    let spectrum = self.view.spectrum(&self.shown);
                    if ui
                        .add_enabled(
                            spectrum.is_some(),
                            egui::Button::selectable(
                                self.derived == Some(Derived::Spectrum),
                                "Spectrum",
                            ),
                        )
                        .clicked()
                    {
                        self.alternate = spectrum;
                        self.derived = Some(Derived::Spectrum);
                        ui.close();
                    }
                    let delay = self.view.group_delay(&self.shown);
                    if ui
                        .add_enabled(
                            delay.is_some(),
                            egui::Button::selectable(
                                self.derived == Some(Derived::GroupDelay),
                                "Group delay",
                            ),
                        )
                        .clicked()
                    {
                        self.alternate = delay;
                        self.derived = Some(Derived::GroupDelay);
                        ui.close();
                    }
                });
                if self.view.can_reference() {
                    ui.menu_button("Reference", |ui| {
                        if ui
                            .selectable_label(
                                self.view.reference_trace().is_none(),
                                "Absolute response",
                            )
                            .clicked()
                        {
                            self.view.set_reference(None, &self.shown);
                            ui.close();
                        }
                        for (trace, name) in self.names.iter().enumerate() {
                            if ui
                                .selectable_label(
                                    self.view.reference_trace() == Some(trace),
                                    name,
                                )
                                .clicked()
                            {
                                self.view
                                    .set_reference(Some(trace), &self.shown);
                                ui.close();
                            }
                        }
                    });
                }
                if self.view.can_trigger() {
                    ui.menu_button("Trigger", |ui| {
                        if ui.button("Clear").clicked() {
                            self.view.clear_trigger(&self.shown);
                            ui.close();
                        }
                        if !self.view.can_set_trigger() {
                            ui.weak(
                                "Place cursor A at the trigger level first",
                            );
                        }
                        for (trace, name) in self.names.iter().enumerate() {
                            ui.menu_button(name, |ui| {
                                for (rising, label) in [
                                    (true, "Rising edge"),
                                    (false, "Falling edge"),
                                ] {
                                    if ui
                                        .add_enabled(
                                            self.view.can_set_trigger(),
                                            egui::Button::new(label),
                                        )
                                        .clicked()
                                    {
                                        self.view.set_trigger(
                                            trace,
                                            rising,
                                            &self.shown,
                                        );
                                        ui.close();
                                    }
                                }
                            });
                        }
                    });
                }
            });
        });
        egui::Panel::left("traces").show(ui, |ui| {
            ui.heading("Traces");
            for (index, (name, shown)) in
                self.names.iter().zip(&mut self.shown).enumerate()
            {
                ui.checkbox(
                    shown,
                    egui::RichText::new(name).color(plot::color(index)),
                );
            }
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            let view = self.alternate.as_ref().unwrap_or(&self.view);
            let mut status = format!("{}: {}", self.file, view.status());
            if let Some(message) = &self.message {
                status = format!("{status}; {message}");
            }
            if let Some(trigger) = self.view.trigger_line() {
                status = format!("{status}; {trigger}");
            }
            ui.label(status);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            match &mut self.alternate {
                Some(view) => view.show(ui, &self.shown),
                None => self.view.show(ui, &self.shown),
            }
        });
        if let Some(dialog) = &mut self.dialog
            && let Some(choice) = dialog.show(&ctx)
        {
            self.dialog = None;
            if let Choice::Save { path, format } = choice {
                self.save(&ctx, path, format);
            }
        }
    }
}
