//! `kitest-scope`: open a scope file in the one live window, or drive and
//! query the scope with JSON-RPC commands.

mod control;
mod live;
mod model;
mod plot;
mod save;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;
use serde_json::Value as Json;

use control::{Command, Format, ViewName};
use model::Model;
use save::{Choice, Dialog};

const USAGE: &str = "usage: kitest-scope FILE\n\
                     \x20      kitest-scope ctl METHOD [PARAMS]\n\
                     \x20      kitest-scope query FILE METHOD [PARAMS]\n\
                     FILE opens in the one live window, which a later launch reuses.\n\
                     ctl sends one JSON-RPC command to that window and prints its result.\n\
                     query applies one command to FILE without a window.\n\
                     PARAMS is a JSON object, such as '{\"trace\":\"/OUT\",\"measurements\":[\"rms\"]}'.\n\
                     Methods: ";

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let strings: Vec<&str> =
        args.iter().filter_map(|arg| arg.to_str()).collect();
    match strings.as_slice() {
        ["-h" | "--help"] => {
            println!("{USAGE}{}", control::METHODS.join(", "));
            ExitCode::SUCCESS
        }
        ["ctl", method, params @ ..] if params.len() <= 1 => {
            ctl(method, params.first().copied())
        }
        ["query", file, method, params @ ..] if params.len() <= 1 => {
            query(&PathBuf::from(file), method, params.first().copied())
        }
        _ if args.len() == 1 => window(PathBuf::from(&args[0])),
        _ => {
            eprintln!("{USAGE}{}", control::METHODS.join(", "));
            ExitCode::from(2)
        }
    }
}

/// Sends one command to the live window and prints its result.
fn ctl(method: &str, params: Option<&str>) -> ExitCode {
    let params = match params.map(serde_json::from_str::<Json>).transpose() {
        Ok(params) => params,
        Err(error) => {
            eprintln!("error: PARAMS is not JSON: {error}");
            return ExitCode::from(2);
        }
    };
    match control::call(&live::socket_path(), method, params) {
        Ok(result) => print_json(&result),
        Err(control::ClientError::NoWindow) => {
            eprintln!(
                "error: no scope window is running; start one with kitest-scope FILE"
            );
            ExitCode::from(2)
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// Applies one command to `file` without a window and prints its result.
fn query(
    file: &std::path::Path,
    method: &str,
    params: Option<&str>,
) -> ExitCode {
    let mut model = match Model::load(file) {
        Ok(model) => model,
        Err(error) => {
            eprintln!("error: {}: {error}", file.display());
            return ExitCode::from(2);
        }
    };
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": match params.map(serde_json::from_str::<Json>).transpose() {
            Ok(params) => params,
            Err(error) => {
                eprintln!("error: PARAMS is not JSON: {error}");
                return ExitCode::from(2);
            }
        },
    });
    match control::parse(&request.to_string())
        .command
        .and_then(|command| model.apply(command))
    {
        Ok(result) => print_json(&result),
        Err(error) => {
            eprintln!("error: {} (code {})", error.message, error.code);
            ExitCode::from(1)
        }
    }
}

/// Prints `json` indented, and succeeds.
fn print_json(json: &Json) -> ExitCode {
    match serde_json::to_string_pretty(json) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// Opens `path` in the live window, starting it when none runs.
fn window(path: PathBuf) -> ExitCode {
    let (requests, context, socket) = match live::connect_or_listen(&path) {
        Ok(live::Instance::Forwarded) => return ExitCode::SUCCESS,
        Ok(live::Instance::Owner {
            requests,
            context,
            socket,
        }) => (requests, context, socket),
        Err(error) => {
            eprintln!("error: {}: {error}", path.display());
            return ExitCode::from(1);
        }
    };
    let model = match Model::load(&path) {
        Ok(model) => model,
        Err(error) => {
            eprintln!("error: {}: {error}", path.display());
            return ExitCode::from(2);
        }
    };
    let app = Scope {
        title: String::new(),
        model,
        dialog: None,
        screenshot: None,
        message: None,
        requests,
        _socket: socket,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_app_id("kitest-scope"),
        ..Default::default()
    };
    match eframe::run_native(
        "kitest scope",
        options,
        Box::new(move |creation| {
            let _ = context.set(creation.egui_ctx.clone());
            Ok(Box::new(app))
        }),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// A PNG on its way: the file it goes to, and the request waiting on it.
struct Screenshot {
    path: PathBuf,
    waiting: Option<(Json, Sender<String>)>,
}

/// The window: the model it shows, and the state of saves and requests.
struct Scope {
    title: String,
    model: Model,
    dialog: Option<Dialog>,
    screenshot: Option<Screenshot>,
    message: Option<String>,
    requests: Receiver<live::Envelope>,
    _socket: live::Socket,
}

impl Scope {
    /// Carries out a command from a menu, reporting a refusal in the status bar.
    fn run(&mut self, ctx: &egui::Context, command: Command) {
        if let Err(error) = self.execute(ctx, command, None) {
            self.message = Some(error.message);
        }
    }

    /// Carries out `command`, which `waiting` asked for over the socket.
    ///
    /// A PNG replies once the screenshot arrives, so it returns `None`.
    fn execute(
        &mut self,
        ctx: &egui::Context,
        command: Command,
        waiting: Option<(Json, Sender<String>)>,
    ) -> Result<Option<Json>, control::Error> {
        match command {
            Command::Save {
                format: Format::Png,
                path,
            } => {
                if self.screenshot.is_some() {
                    return Err(control::Error::failed(
                        "a PNG is already being saved",
                    ));
                }
                self.screenshot = Some(Screenshot { path, waiting });
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                    egui::UserData::default(),
                ));
                Ok(None)
            }
            command => {
                let result = self.model.apply(command)?;
                if let Some((id, reply)) = waiting {
                    let _ = reply.send(control::reply(id, Ok(result.clone())));
                }
                Ok(Some(result))
            }
        }
    }

    /// Answers every request that arrived over the socket.
    fn receive_requests(&mut self, ctx: &egui::Context) {
        let envelopes: Vec<live::Envelope> = self.requests.try_iter().collect();
        for envelope in envelopes {
            let id = envelope.call.id;
            let outcome = envelope.call.command.and_then(|command| {
                self.execute(
                    ctx,
                    command,
                    Some((id.clone(), envelope.reply.clone())),
                )
            });
            if let Err(error) = outcome {
                let _ = envelope.reply.send(control::reply(id, Err(error)));
            }
        }
    }

    /// Writes a screenshot that arrived this frame, and answers its request.
    fn receive_screenshot(&mut self, ctx: &egui::Context) {
        let image = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else { return };
        let Some(screenshot) = self.screenshot.take() else {
            return;
        };
        let path = screenshot.path;
        let outcome = save::write_png(&path, &image)
            .map(|()| serde_json::json!({ "path": path }))
            .map_err(|error| {
                control::Error::failed(format!("{}: {error}", path.display()))
            });
        self.message = Some(match &outcome {
            Ok(_) => format!("saved {}", path.display()),
            Err(error) => error.message.clone(),
        });
        if let Some((id, reply)) = screenshot.waiting {
            let _ = reply.send(control::reply(id, outcome));
        }
    }
}

impl eframe::App for Scope {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.receive_requests(&ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        if self.model.changed_on_disk() {
            let path = self.model.path().to_path_buf();
            self.run(&ctx, Command::Open { path });
        }
        self.receive_screenshot(&ctx);
        if self.title != self.model.file() {
            self.title = self.model.file();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                "kitest scope: {}",
                self.title
            )));
        }
        if ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::S)
        }) {
            self.dialog = Some(Dialog::new(self.model.path(), Format::Png));
        }
        let mut commands = Vec::new();
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    for (format, label) in
                        [(Format::Png, "Save PNG…"), (Format::Csv, "Save CSV…")]
                    {
                        if ui.button(label).clicked() {
                            self.dialog =
                                Some(Dialog::new(self.model.path(), format));
                            ui.close();
                        }
                    }
                });
                ui.menu_button("View", |ui| {
                    let views = self.model.views();
                    for (name, label) in [
                        (ViewName::Primary, "Primary"),
                        (ViewName::Spectrum, "Spectrum"),
                        (ViewName::GroupDelay, "Group delay"),
                    ] {
                        let selected = self.model.view_name() == name;
                        if ui
                            .add_enabled(
                                views.contains(&name),
                                egui::Button::selectable(selected, label),
                            )
                            .clicked()
                        {
                            commands.push(Command::View { name });
                            ui.close();
                        }
                    }
                });
                let primary = self.model.primary();
                if primary.can_reference() {
                    ui.menu_button("Reference", |ui| {
                        let current = primary.reference_trace();
                        if ui
                            .selectable_label(
                                current.is_none(),
                                "Absolute response",
                            )
                            .clicked()
                        {
                            commands.push(Command::Reference { trace: None });
                            ui.close();
                        }
                        for (index, name) in
                            self.model.names().iter().enumerate()
                        {
                            if ui
                                .selectable_label(current == Some(index), name)
                                .clicked()
                            {
                                commands.push(Command::Reference {
                                    trace: Some(name.clone()),
                                });
                                ui.close();
                            }
                        }
                    });
                }
                if primary.can_trigger() {
                    ui.menu_button("Trigger", |ui| {
                        if ui.button("Clear").clicked() {
                            commands.push(Command::Trigger {
                                trace: None,
                                edge: control::Edge::Rising,
                                level: None,
                            });
                            ui.close();
                        }
                        let ready = primary.can_set_trigger();
                        if !ready {
                            ui.weak(
                                "Place cursor A at the trigger level first",
                            );
                        }
                        for name in self.model.names() {
                            ui.menu_button(name, |ui| {
                                for (edge, label) in [
                                    (control::Edge::Rising, "Rising edge"),
                                    (control::Edge::Falling, "Falling edge"),
                                ] {
                                    if ui
                                        .add_enabled(
                                            ready,
                                            egui::Button::new(label),
                                        )
                                        .clicked()
                                    {
                                        commands.push(Command::Trigger {
                                            trace: Some(name.clone()),
                                            edge,
                                            level: None,
                                        });
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
            for (index, (name, &shown)) in self
                .model
                .names()
                .iter()
                .zip(self.model.shown())
                .enumerate()
            {
                let mut checked = shown;
                if ui
                    .checkbox(
                        &mut checked,
                        egui::RichText::new(name).color(plot::color(index)),
                    )
                    .changed()
                {
                    commands.push(Command::Show {
                        trace: name.clone(),
                        shown: checked,
                    });
                }
            }
        });
        for command in commands {
            self.run(&ctx, command);
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            let mut status = format!(
                "{}: {}",
                self.model.file(),
                self.model.active().status()
            );
            if let Some(trigger) = self.model.primary().trigger_line() {
                status = format!("{status}; {trigger}");
            }
            if let Some(message) = &self.message {
                status = format!("{status}; {message}");
            }
            ui.label(status);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let (view, shown) = self.model.active_mut();
            view.show(ui, shown);
        });
        if let Some(dialog) = &mut self.dialog
            && let Some(choice) = dialog.show(&ctx)
        {
            self.dialog = None;
            if let Choice::Save { path, format } = choice {
                self.run(&ctx, Command::Save { format, path });
            }
        }
    }
}
