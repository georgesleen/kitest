//! Running netlists through the `ngspice` binary.

use std::str::FromStr;
use std::{collections::BTreeMap, path::Path};

use num_complex::Complex64;

use crate::{Analysis, Backend, Results};

pub struct Ngspice {
    binary: String,
}

impl Default for Ngspice {
    fn default() -> Self {
        Self {
            binary: "ngspice".into(),
        }
    }
}

impl Backend for Ngspice {
    type Error = NgspiceError;

    fn run(&self, netlist: &str, analysis: Analysis) -> Result<Results, NgspiceError> {
        let dir = tempfile::tempdir().map_err(NgspiceError::Io)?;
        let deck_path = dir.path().join("deck.cir");
        let raw_path = dir.path().join("out.raw");

        std::fs::write(&deck_path, build_deck(netlist, &analysis, &raw_path))
            .map_err(NgspiceError::Io)?;

        let output = std::process::Command::new(&self.binary)
            .arg("-b")
            .arg(&deck_path)
            .output()
            .map_err(NgspiceError::Spawn)?;

        if !output.status.success() {
            return Err(NgspiceError::Exec {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        let raw = std::fs::read_to_string(&raw_path).map_err(NgspiceError::Io)?;
        parse_rawfile(&raw)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NgspiceError {
    #[error("could not launch ngspice")]
    Spawn(#[source] std::io::Error),

    #[error("io error while running ngspice")]
    Io(#[source] std::io::Error),

    #[error("ngspice exited with status {code:?}:\n{stderr}")]
    Exec { code: Option<i32>, stderr: String },

    #[error("could not parse ngspice rawfile: {0}")]
    Parse(String),
}

fn build_deck(netlist: &str, analysis: &Analysis, raw_path: &Path) -> String {
    format!(
        "{netlist}\n.control\n{cmd}\nset filetype=ascii\nwrite {raw}\n.endc\n.end\n",
        cmd = directive(analysis),
        raw = raw_path.display(),
    )
}

/// The ngspice `.control` command for an analysis
fn directive(analysis: &Analysis) -> String {
    match analysis {
        Analysis::Op => "op".to_string(),
        Analysis::Tran { step, stop } => format!("tran {step} {stop}"),
    }
}

/// Parse an ASCII ngspice rawfile into [Results].
fn parse_rawfile(raw: &str) -> Result<Results, NgspiceError> {
    let mut lines = raw.lines();
    let mut n_vars: Option<usize> = None;
    let mut n_points: Option<usize> = None;
    let mut complex = false;

    for line in lines.by_ref() {
        if line.starts_with("Variables:") {
            break;
        }
        let Some((key, value)) = line.split_once(":") else {
            continue;
        };
        match key.trim() {
            "No. Variables" => n_vars = Some(parse_num(value, "count")?),
            "No. Points" => n_points = Some(parse_num(value, "count")?),
            "Flags" => complex = value.contains("complex"),
            _ => {}
        }
    }

    let n_vars = n_vars.ok_or_else(|| NgspiceError::Parse("missing No. Variables".into()))?;
    let n_points = n_points.ok_or_else(|| NgspiceError::Parse("missing No. Points".into()))?;

    // Collect variable names
    let mut names = Vec::with_capacity(n_vars);
    for line in lines.by_ref().take(n_vars) {
        let name = line
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| NgspiceError::Parse(format!("malformed variable line: {line:?}")))?;
        names.push(name.to_owned());
    }

    // Collect variable values
    lines
        .find(|line| line.starts_with("Values:"))
        .ok_or_else(|| NgspiceError::Parse("missing Values section".into()))?;

    let count = n_vars * n_points;
    if complex {
        let values = collect_values(lines, count, parse_complex)?;
        Ok(Results::complex(reshape(names, &values, n_vars, n_points)))
    } else {
        let values = collect_values(lines, count, |s| parse_num(s, "value"))?;
        Ok(Results::real(reshape(names, &values, n_vars, n_points)))
    }
}

/// Parse `s` into a number, tagging a failure with `what` for the error message.
fn parse_num<T: FromStr>(s: &str, what: &str) -> Result<T, NgspiceError> {
    s.trim()
        .parse()
        .map_err(|_| NgspiceError::Parse(format!("bad {what}: {s:?}")))
}

/// Parse one complex value written as `re,im`.
fn parse_complex(s: &str) -> Result<Complex64, NgspiceError> {
    let (re, im) = s
        .split_once(',')
        .ok_or_else(|| NgspiceError::Parse(format!("bad complex: {s:?}")))?;
    Ok(Complex64::new(
        parse_num(re, "value")?,
        parse_num(im, "value")?,
    ))
}

/// Read one trailing token per line from the `Values:` body into a flat list.
fn collect_values<T>(
    lines: std::str::Lines,
    expected: usize,
    parse: impl Fn(&str) -> Result<T, NgspiceError>,
) -> Result<Vec<T>, NgspiceError> {
    let mut values: Vec<T> = vec![];
    for line in lines {
        let Some(token) = line.split_whitespace().last() else {
            continue;
        };
        values.push(parse(token)?);
    }
    if values.len() != expected {
        return Err(NgspiceError::Parse(format!(
            "expected {expected} values, found {}",
            values.len()
        )));
    }
    Ok(values)
}

/// Split a row-major value list into per-name columns.
fn reshape<T: Copy>(
    names: Vec<String>,
    values: &[T],
    n_vars: usize,
    n_points: usize,
) -> BTreeMap<String, Vec<T>> {
    let mut signals = BTreeMap::new();
    for (v, name) in names.into_iter().enumerate() {
        let signal = (0..n_points).map(|p| values[p * n_vars + v]).collect();
        signals.insert(name, signal);
    }
    signals
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARSE_EPS: f64 = 1e-12;

    const TRANSIENT: &str = "\
Title: synthetic
Plotname: Transient Analysis
Flags: real
No. Variables: 2
No. Points: 2
Variables:
 0 time time
 1 v(out) voltage
Values:
 0 0.0
 1.0
 1 1.0e-3
 2.0
";

    const AC: &str = "\
Title: synthetic
Plotname: AC Analysis
Flags: complex
No. Variables: 2
No. Points: 2
Variables:
 0 frequency frequency
 1 v(out) voltage
Values:
 0 1.0,0.0
 0.6,-0.8
 1 10.0,0.0
 0.3,-0.4
";

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(path).unwrap()
    }

    fn assert_signal(r: &Results, name: &str, point: usize, expected: f64) {
        let got = r.signal(name).expect("signal present")[point];
        assert!(
            (got - expected).abs() < PARSE_EPS,
            "{name}[{point}] = {got}, expected {expected}"
        );
    }

    fn assert_parse_err(raw: &str, needle: &str) {
        let err = parse_rawfile(raw).expect_err("expected parse failure");
        let msg = err.to_string();
        assert!(
            msg.contains(needle),
            "error {msg:?} did not mention {needle:?}"
        );
    }

    #[test]
    fn parses_op_point() {
        let raw = fixture("voltage-divider/divider.raw");
        let r = parse_rawfile(&raw).unwrap();
        assert_signal(&r, "v(vin)", 0, 5.0);
        assert_signal(&r, "i(v1)", 0, -2.5e-4);
        assert_signal(&r, "v(vout)", 0, 2.5);
        assert!(r.signal("nope").is_none());
    }

    #[test]
    fn reshapes_multiple_points() {
        let r = parse_rawfile(TRANSIENT).unwrap();
        assert_signal(&r, "time", 0, 0.0);
        assert_signal(&r, "time", 1, 1.0e-3);
        assert_signal(&r, "v(out)", 0, 1.0);
        assert_signal(&r, "v(out)", 1, 2.0);
    }

    #[test]
    fn parses_complex_ac() {
        let r = parse_rawfile(AC).unwrap();
        let out = r.spectrum("v(out)").expect("spectrum present");
        assert_eq!(out[0], Complex64::new(0.6, -0.8));
        assert!(r.signal("v(out)").is_none());
    }

    #[test]
    fn rejects_missing_variable_count() {
        assert_parse_err(
            "Plotname: Operating Point\nVariables:\nValues:\n",
            "No. Variables",
        );
    }

    #[test]
    fn rejects_missing_values_section() {
        assert_parse_err(
            "No. Variables: 1\nNo. Points: 1\nVariables:\n 0 v(out) voltage\n",
            "Values",
        );
    }

    #[test]
    fn rejects_point_count_mismatch() {
        let raw =
            "No. Variables: 1\nNo. Points: 2\nVariables:\n 0 v(out) voltage\nValues:\n 0 1.0\n";
        assert_parse_err(raw, "expected");
    }

    #[test]
    fn rejects_non_numeric_value() {
        let raw =
            "No. Variables: 1\nNo. Points: 1\nVariables:\n 0 v(out) voltage\nValues:\n 0 oops\n";
        assert_parse_err(raw, "bad value");
    }

    #[test]
    fn tran_directive_renders() {
        let d = directive(&Analysis::Tran {
            step: 1e-3,
            stop: 5e-3,
        });
        assert_eq!(d, "tran 0.001 0.005");
    }

    #[test]
    fn build_deck_wraps_netlist() {
        let deck = build_deck(
            "* t\nv1 a 0 dc 1\n",
            &Analysis::Op,
            Path::new("/tmp/out.raw"),
        );
        assert!(deck.contains("* t\nv1 a 0 dc 1\n"));
        assert!(deck.contains("\nop\n"));
        assert!(deck.contains("\nset filetype=ascii\n"));
        assert!(deck.contains("\nwrite /tmp/out.raw\n"));
        assert!(deck.contains("\n.endc\n"));
        assert!(deck.ends_with(".end\n"));
    }
}
