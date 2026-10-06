//! `kitest [PROJECT]`: check every probe in a KiCad project.
//!
//! PROJECT is a project directory or a `.kicad_sch` file, the current
//! directory when omitted. Exits 0 when every check passes, 1 when one
//! fails, and 2 when the probes cannot be checked at all.

use std::path::PathBuf;
use std::process::ExitCode;

use kitest::{Outcome, check_project};

const USAGE: &str = "usage: kitest [PROJECT]\n\
                     Check every probe in the KiCad project at PROJECT, a \
                     directory or a .kicad_sch file (default: .)";

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let project = match (args.next(), args.next()) {
        (None, _) => PathBuf::from("."),
        (Some(arg), None) if arg == "-h" || arg == "--help" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        (Some(arg), None) => PathBuf::from(arg),
        (Some(_), Some(_)) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    let report = match check_project(&project) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("error: {}", chain(&error));
            return ExitCode::from(2);
        }
    };

    for outcome in &report.outcomes {
        println!("{}", line(outcome));
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
    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// One outcome as `PASS COLPITTS_OUT (PRB1, /OUT) at VCC=9 V: ...`.
fn line(outcome: &Outcome) -> String {
    let status = match &outcome.check {
        None => "----",
        Some(check) if check.passed() => "PASS",
        Some(_) => "FAIL",
    };
    let corner: Vec<String> = outcome
        .corner
        .iter()
        .map(|(node, volts)| format!("{node}={volts} V"))
        .collect();
    let at = if corner.is_empty() {
        String::new()
    } else {
        format!(" at {}", corner.join(", "))
    };
    let detail = match &outcome.check {
        None => "no Expect, nothing checked".to_owned(),
        Some(check) => check.to_string(),
    };
    format!(
        "{status} {} ({}, {}){at}: {detail}",
        outcome.probe, outcome.reference, outcome.net
    )
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
