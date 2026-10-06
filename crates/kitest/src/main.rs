//! `kitest [--show PROBE] [PROJECT]`: check every probe in a KiCad project.
//!
//! PROJECT is a project directory or a `.kicad_sch` file, the current
//! directory when omitted. Exits 0 when every check passes, 1 when one
//! fails, and 2 when the probes cannot be checked at all.

mod sketch;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use kitest::{Outcome, Report, check_project};

const USAGE: &str = "usage: kitest [--show PROBE] [PROJECT]\n\
                     Check every probe in the KiCad project at PROJECT, a \
                     directory or a .kicad_sch file (default: .)\n\
                     --show PROBE  sketch the waveform an oscillation check \
                     ran on, for the probe named or referenced PROBE";

/// What the command line asks for.
struct Args {
    project: PathBuf,
    show: Option<String>,
}

/// The parsed command line, or `None` for one that does not parse.
fn parse(args: impl Iterator<Item = OsString>) -> Option<Args> {
    let mut project = None;
    let mut show = None;
    let mut args = args;
    while let Some(arg) = args.next() {
        if arg == "--show" {
            show = Some(args.next()?.into_string().ok()?);
        } else if project.is_none() && !arg.to_string_lossy().starts_with('-') {
            project = Some(PathBuf::from(arg));
        } else {
            return None;
        }
    }
    Some(Args {
        project: project.unwrap_or_else(|| PathBuf::from(".")),
        show,
    })
}

fn main() -> ExitCode {
    let args = std::env::args_os().skip(1);
    if std::env::args_os()
        .skip(1)
        .any(|arg| arg == "-h" || arg == "--help")
    {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let Some(args) = parse(args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };

    let report = match check_project(&args.project) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("error: {}", chain(&error));
            return ExitCode::from(2);
        }
    };

    for outcome in &report.outcomes {
        println!("{}", line(outcome));
        for finding in &outcome.diagnosis {
            println!("    {finding}");
        }
    }
    let checked = report
        .outcomes
        .iter()
        .filter(|outcome| outcome.check.is_some())
        .count();
    let failed = report
        .outcomes
        .iter()
        .filter(|outcome| !outcome.passed())
        .count();
    println!("{} passed, {failed} failed", checked - failed);
    if let Some(probe) = &args.show
        && let Err(message) = show(&report, probe)
    {
        eprintln!("error: {message}");
        return ExitCode::from(2);
    }
    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Print a sketch of each waveform `probe`'s checks ran on.
fn show(report: &Report, probe: &str) -> Result<(), String> {
    let shown: Vec<&Outcome> = report
        .outcomes
        .iter()
        .filter(|outcome| outcome.probe == probe || outcome.reference == probe)
        .collect();
    if shown.is_empty() {
        let mut names: Vec<&str> = report
            .outcomes
            .iter()
            .map(|outcome| outcome.probe.as_str())
            .collect();
        names.dedup();
        return Err(format!(
            "no probe is named {probe}; the probes are {}",
            names.join(", ")
        ));
    }
    for outcome in shown {
        println!();
        println!(
            "{} ({}, {}){}",
            outcome.probe,
            outcome.reference,
            outcome.net,
            at(outcome)
        );
        match &outcome.waveform {
            Some(waveform) => print!("{}", sketch::sketch(waveform)),
            None => println!("no waveform: only an oscillation check runs one"),
        }
    }
    Ok(())
}

/// One outcome as `PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: ...`.
fn line(outcome: &Outcome) -> String {
    let status = match &outcome.check {
        None => "----",
        Some(check) if check.passed() => "PASS",
        Some(_) => "FAIL",
    };
    let at = at(outcome);
    let detail = match &outcome.check {
        None => "no Expect, nothing checked".to_owned(),
        Some(check) => check.to_string(),
    };
    format!(
        "{status} {} ({}, {}){at}: {detail}",
        outcome.probe, outcome.reference, outcome.net
    )
}

/// ` at VCC=9 V`, the corner `outcome` ran at, or empty with no corner.
fn at(outcome: &Outcome) -> String {
    let corner: Vec<String> = outcome
        .corner
        .iter()
        .map(|(node, volts)| format!("{node}={volts} V"))
        .collect();
    if corner.is_empty() {
        String::new()
    } else {
        format!(" at {}", corner.join(", "))
    }
}

/// `error` followed by each underlying cause.
fn chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(source) = cause {
        message.push_str("\ncaused by: ");
        message.push_str(&source.to_string());
        cause = source.source();
    }
    message
}
