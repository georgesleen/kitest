//! `kitest [--show PROBE | --sketch PROBE] [PROJECT]`: check every probe in
//! a KiCad project.
//!
//! PROJECT is a project directory or a `.kicad_sch` file, the current
//! directory when omitted. Exits 0 when every check passes, 1 when one
//! fails, and 2 when the probes cannot be checked at all.

mod sketch;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use kitest::{Outcome, Report, check_project};
use kitest_scope::{Capture, Data, Trace};

const USAGE: &str = "usage: kitest [--show PROBE | --sketch PROBE] [PROJECT]\n\
                     Check every probe in the KiCad project at PROJECT, a \
                     directory or a .kicad_sch file (default: .)\n\
                     --show PROBE   open the waveform in kitest-scope\n\
                     --sketch PROBE print the waveform in the terminal";

/// What the command line asks for.
struct Args {
    project: PathBuf,
    show: Option<String>,
    sketch: Option<String>,
}

/// The parsed command line, or `None` for one that does not parse.
fn parse(args: impl Iterator<Item = OsString>) -> Option<Args> {
    let mut project = None;
    let mut show = None;
    let mut sketch = None;
    let mut args = args;
    while let Some(arg) = args.next() {
        if arg == "--show" {
            show = Some(args.next()?.into_string().ok()?);
        } else if arg == "--sketch" {
            sketch = Some(args.next()?.into_string().ok()?);
        } else if project.is_none() && !arg.to_string_lossy().starts_with('-') {
            project = Some(PathBuf::from(arg));
        } else {
            return None;
        }
    }
    if show.is_some() && sketch.is_some() {
        return None;
    }
    Some(Args {
        project: project.unwrap_or_else(|| PathBuf::from(".")),
        show,
        sketch,
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

    let captures = match write_captures(&args.project, &report) {
        Ok(captures) => captures,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    if let Some(probe) = &args.show
        && let Err(message) = show(&report, &captures, probe)
    {
        eprintln!("error: {message}");
        return ExitCode::from(2);
    }
    if let Some(probe) = &args.sketch
        && let Err(message) = sketch(&report, probe)
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

/// One capture written for one outcome.
struct WrittenCapture<'a> {
    outcome: &'a Outcome,
    path: PathBuf,
}

/// Writes every waveform to the project's `.kitest/captures` directory.
fn write_captures<'a>(
    project: &Path,
    report: &'a Report,
) -> Result<Vec<WrittenCapture<'a>>, String> {
    let project = if project.is_dir() {
        project
    } else {
        project.parent().unwrap_or(Path::new("."))
    };
    let directory = project.join(".kitest/captures");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    let mut written = Vec::new();
    for outcome in &report.outcomes {
        let Some(waveform) = &outcome.waveform else {
            continue;
        };
        let mut capture = Capture::new(
            outcome.probe.clone(),
            Data::Transient {
                time: waveform.time.clone(),
                traces: vec![Trace {
                    name: outcome.net.clone(),
                    values: waveform.volts.clone(),
                }],
            },
        );
        capture.corner = corner(outcome);
        if let Some(check) = &outcome.check {
            capture
                .expectations
                .extend(check.expectations(&outcome.net));
        }
        let path = directory.join(capture.file_name());
        capture
            .save(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        written.push(WrittenCapture { outcome, path });
    }
    Ok(written)
}

/// Opens each `probe` capture in the one live scope window.
fn show(
    report: &Report,
    captures: &[WrittenCapture<'_>],
    probe: &str,
) -> Result<(), String> {
    let matched = outcomes(report, probe)?;
    let paths: Vec<&Path> = captures
        .iter()
        .filter(|capture| matched.contains(&capture.outcome))
        .map(|capture| capture.path.as_path())
        .collect();
    if paths.is_empty() {
        return Err(format!("{probe} has no waveform to show"));
    }
    let scope = scope_binary();
    for path in paths {
        Command::new(&scope).arg(path).spawn().map_err(|error| {
            format!("could not launch {}: {error}", scope.to_string_lossy())
        })?;
    }
    Ok(())
}

/// Prints a sketch of each waveform `probe`'s checks ran on.
fn sketch(report: &Report, probe: &str) -> Result<(), String> {
    for outcome in outcomes(report, probe)? {
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

/// Outcomes for the probe named or referenced by `probe`.
fn outcomes<'a>(
    report: &'a Report,
    probe: &str,
) -> Result<Vec<&'a Outcome>, String> {
    let shown: Vec<&Outcome> = report
        .outcomes
        .iter()
        .filter(|outcome| outcome.probe == probe || outcome.reference == probe)
        .collect();
    if !shown.is_empty() {
        return Ok(shown);
    }
    let mut names: Vec<&str> = report
        .outcomes
        .iter()
        .map(|outcome| outcome.probe.as_str())
        .collect();
    names.dedup();
    Err(format!(
        "no probe is named {probe}; the probes are {}",
        names.join(", ")
    ))
}

/// The scope binary beside kitest in development, or its name for PATH lookup.
fn scope_binary() -> OsString {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent().map(|parent| parent.join("kitest-scope"))
        })
        .filter(|path| path.is_file())
        .map_or_else(|| OsString::from("kitest-scope"), PathBuf::into_os_string)
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
    corner(outcome).map_or_else(String::new, |corner| format!(" at {corner}"))
}

/// `VCC=9 V`, the corner `outcome` ran at, or `None` with no corner.
fn corner(outcome: &Outcome) -> Option<String> {
    let corner: Vec<String> = outcome
        .corner
        .iter()
        .map(|(node, volts)| format!("{node}={volts} V"))
        .collect();
    (!corner.is_empty()).then(|| corner.join(", "))
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

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use kitest::{Check, Outcome, Region, Report, Waveform};

    use super::{parse, scope_binary, write_captures};

    #[test]
    fn show_and_sketch_are_distinct_and_exclusive() {
        let args =
            parse(["--show", "OUT", "board"].into_iter().map(Into::into))
                .unwrap();
        assert_eq!(args.show.as_deref(), Some("OUT"));
        assert_eq!(args.sketch, None);
        let args =
            parse(["--sketch", "OUT"].into_iter().map(Into::into)).unwrap();
        assert_eq!(args.show, None);
        assert_eq!(args.sketch.as_deref(), Some("OUT"));
        assert!(
            parse(
                ["--show", "OUT", "--sketch", "OUT"]
                    .into_iter()
                    .map(Into::into)
            )
            .is_none()
        );
    }

    #[test]
    fn the_scope_binary_is_named_kitest_scope() {
        assert_eq!(
            std::path::Path::new(&scope_binary()).file_name(),
            Some(OsStr::new("kitest-scope"))
        );
    }

    #[test]
    fn a_waveform_is_saved_with_its_corner_and_expectation() {
        let report = Report {
            outcomes: vec![Outcome {
                probe: "OUT".into(),
                reference: "PRB1".into(),
                net: "/OUT".into(),
                corner: vec![("VCC".into(), 9.0)],
                check: Some(Check::new(true, "in band".into()).with_region(
                    Region::Frequency {
                        low: 990.0,
                        high: 1010.0,
                    },
                )),
                diagnosis: Vec::new(),
                waveform: Some(Waveform {
                    time: vec![0.0, 1.0],
                    volts: vec![0.0, 1.0],
                    expected_hertz: 1000.0,
                }),
            }],
        };
        let directory = tempfile::tempdir().unwrap();
        let written = write_captures(directory.path(), &report).unwrap();
        assert_eq!(written.len(), 1);
        let capture = kitest::Capture::load(&written[0].path).unwrap();
        assert_eq!(capture.name, "OUT");
        assert_eq!(capture.corner.as_deref(), Some("VCC=9 V"));
        assert_eq!(capture.expectations.len(), 1);
        assert!(capture.expectations[0].passed);
        assert_eq!(capture.expectations[0].trace, "/OUT");
    }
}
