//! `kitest-scope FILE`: open a scope file in a window.

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
    let capture = match Capture::load(&path) {
        Ok(capture) => capture,
        Err(error) => {
            eprintln!("error: {}: {error}", path.display());
            return ExitCode::from(2);
        }
    };

    let file = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let title = format!("kitest scope: {file}");
    let app = Scope::new(path, file, capture);
    match eframe::run_native(
        &title,
        eframe::NativeOptions::default(),
        Box::new(|_| Ok(Box::new(app))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// The window: the capture's view, which traces are shown, and the state
/// of a save.
struct Scope {
    path: PathBuf,
    file: String,
    names: Vec<String>,
    view: plot::View,
    shown: Vec<bool>,
    dialog: Option<Dialog>,
    screenshot_to: Option<PathBuf>,
    message: Option<String>,
}

impl Scope {
    fn new(path: PathBuf, file: String, capture: Capture) -> Self {
        let (names, view): (Vec<String>, _) = match &capture {
            Capture::Transient { time, traces } => (
                traces.iter().map(|trace| trace.name.clone()).collect(),
                plot::time::view(time, traces),
            ),
            Capture::Ac { frequency, traces } => (
                traces.iter().map(|trace| trace.name.clone()).collect(),
                plot::bode::view(frequency, traces),
            ),
        };
        Self {
            path,
            file,
            shown: vec![true; names.len()],
            names,
            view,
            dialog: None,
            screenshot_to: None,
            message: None,
        }
    }

    /// Carries out the save the dialog asked for.
    fn save(&mut self, ctx: &egui::Context, path: PathBuf, format: Format) {
        match format {
            Format::Csv => {
                let result = std::fs::write(&path, self.view.csv(&self.shown));
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

impl eframe::App for Scope {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
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
            let mut status = format!("{}: {}", self.file, self.view.status());
            if let Some(message) = &self.message {
                status = format!("{status}; {message}");
            }
            ui.label(status);
        });
        egui::CentralPanel::default()
            .show(ui, |ui| self.view.show(ui, &self.shown));
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
