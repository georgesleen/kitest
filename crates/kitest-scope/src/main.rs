//! `kitest-scope FILE`: open a scope file in a window.

use std::path::PathBuf;
use std::process::ExitCode;

use eframe::egui;
use kitest_scope::Capture;

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

    let title = format!("kitest scope: {}", path.display());
    let app = Scope::new(capture);
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

/// The window: the capture, and which of its traces are shown.
struct Scope {
    capture: Capture,
    shown: Vec<bool>,
}

impl Scope {
    fn new(capture: Capture) -> Self {
        let traces = match &capture {
            Capture::Transient { traces, .. } => traces.len(),
            Capture::Ac { traces, .. } => traces.len(),
        };
        Self {
            capture,
            shown: vec![true; traces],
        }
    }

    fn names(&self) -> Vec<&str> {
        match &self.capture {
            Capture::Transient { traces, .. } => {
                traces.iter().map(|trace| trace.name.as_str()).collect()
            }
            Capture::Ac { traces, .. } => {
                traces.iter().map(|trace| trace.name.as_str()).collect()
            }
        }
    }
}

impl eframe::App for Scope {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("traces").show(ui, |ui| {
            ui.heading("Traces");
            let names: Vec<String> =
                self.names().into_iter().map(str::to_owned).collect();
            for (name, shown) in names.iter().zip(&mut self.shown) {
                ui.checkbox(shown, name);
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let (kind, points) = match &self.capture {
                Capture::Transient { time, .. } => ("transient", time.len()),
                Capture::Ac { frequency, .. } => ("AC sweep", frequency.len()),
            };
            ui.label(format!("{kind}, {points} points"));
        });
    }
}
