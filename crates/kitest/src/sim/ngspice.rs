//! Running netlists through the `ngspice` binary.
//!
//! This module is the only place that knows ngspice's raw file format. It parses
//! the raw text into a neutral [`RawTable`], then each analysis validates the
//! table it expects and maps it to a domain result type.

use std::str::FromStr;
use std::{collections::BTreeMap, path::Path};

use num_complex::Complex64;

use crate::stimulus::{AcSupply, inject};
use crate::{
    Ac, Backend, DcSupply, OperatingPoint, Spectra, Tran, TranSource, Transient,
};

/// Ngspice plotname for each analysis, used to validate a raw file is what we ran.
const PLOTNAME_OP: &str = "Operating Point";
const PLOTNAME_TRAN: &str = "Transient Analysis";
const PLOTNAME_AC: &str = "AC Analysis";

/// The independent-variable column ngspice emits for each swept analysis.
const AXIS_TIME: &str = "time";
const AXIS_FREQUENCY: &str = "frequency";

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

    fn run_op(
        &self,
        netlist: &str,
        supplies: &[DcSupply],
    ) -> Result<OperatingPoint, NgspiceError> {
        let deck = inject(netlist, supplies);
        operating_point(self.run_raw(&deck, "op")?)
    }

    fn run_tran(
        &self,
        netlist: &str,
        sources: &[TranSource],
        params: Tran,
    ) -> Result<Transient, NgspiceError> {
        let deck = inject(netlist, sources);
        transient(self.run_raw(&deck, &tran_command(&params))?)
    }

    fn run_ac(
        &self,
        netlist: &str,
        supplies: &[AcSupply],
        params: Ac,
    ) -> Result<Spectra, NgspiceError> {
        let deck = inject(netlist, supplies);
        spectra(self.run_raw(&deck, &ac_command(&params))?)
    }
}

impl Ngspice {
    /// Run one `.control` directive and parse the raw file it writes.
    fn run_raw(
        &self,
        netlist: &str,
        directive: &str,
    ) -> Result<RawTable, NgspiceError> {
        let dir = tempfile::tempdir().map_err(NgspiceError::Io)?;
        let deck_path = dir.path().join("deck.cir");
        let raw_path = dir.path().join("out.raw");

        std::fs::write(&deck_path, build_deck(netlist, directive, &raw_path))
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

        let raw =
            std::fs::read_to_string(&raw_path).map_err(NgspiceError::Io)?;
        parse_table(&raw)
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

fn build_deck(netlist: &str, directive: &str, raw_path: &Path) -> String {
    format!(
        "{netlist}\n.control\n{directive}\nset filetype=ascii\nwrite {raw}\n.endc\n.end\n",
        raw = raw_path.display(),
    )
}

fn tran_command(params: &Tran) -> String {
    format!("tran {} {}", params.step, params.stop)
}

fn ac_command(params: &Ac) -> String {
    format!(
        "ac {} {} {} {}",
        params.sweep.keyword(),
        params.points,
        params.fstart,
        params.fstop,
    )
}

/// A parsed raw file, before it is interpreted as a particular analysis domain.
#[derive(Debug)]
struct RawTable {
    plotname: String,
    n_points: usize,
    columns: Columns,
}

#[derive(Debug)]
enum Columns {
    Real(BTreeMap<String, Vec<f64>>),
    Complex(BTreeMap<String, Vec<Complex64>>),
}

/// Parse an ASCII ngspice raw file into a neutral [`RawTable`].
fn parse_table(raw: &str) -> Result<RawTable, NgspiceError> {
    let mut lines = raw.lines();
    let mut plotname = None;
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
            "Plotname" => plotname = Some(value.trim().to_owned()),
            "No. Variables" => n_vars = Some(parse_num(value, "count")?),
            "No. Points" => n_points = Some(parse_num(value, "count")?),
            "Flags" => complex = value.contains("complex"),
            _ => {}
        }
    }

    let plotname = plotname
        .ok_or_else(|| NgspiceError::Parse("missing Plotname".into()))?;
    let n_vars = n_vars
        .ok_or_else(|| NgspiceError::Parse("missing No. Variables".into()))?;
    let n_points = n_points
        .ok_or_else(|| NgspiceError::Parse("missing No. Points".into()))?;

    let mut names = Vec::with_capacity(n_vars);
    for line in lines.by_ref().take(n_vars) {
        let name = line.split_whitespace().nth(1).ok_or_else(|| {
            NgspiceError::Parse(format!("malformed variable line: {line:?}"))
        })?;
        names.push(name.to_owned());
    }

    lines
        .find(|line| line.starts_with("Values:"))
        .ok_or_else(|| NgspiceError::Parse("missing Values section".into()))?;

    let count = n_vars * n_points;
    let columns = if complex {
        let values = collect_values(lines, count, parse_complex)?;
        Columns::Complex(reshape(names, &values, n_vars, n_points))
    } else {
        let values = collect_values(lines, count, |s| parse_num(s, "value"))?;
        Columns::Real(reshape(names, &values, n_vars, n_points))
    };

    Ok(RawTable {
        plotname,
        n_points,
        columns,
    })
}

/// Interpret a raw file as an operating point: one real value per node.
fn operating_point(table: RawTable) -> Result<OperatingPoint, NgspiceError> {
    expect_plotname(&table, PLOTNAME_OP)?;
    let columns = real_columns(table.columns, "operating point")?;
    if table.n_points != 1 {
        return Err(NgspiceError::Parse(format!(
            "operating point has {} points, expected 1",
            table.n_points
        )));
    }
    let voltages = columns
        .into_iter()
        .filter_map(|(var, series)| {
            Some((node_name(&var)?.to_owned(), series[0]))
        })
        .collect();
    Ok(OperatingPoint::new(voltages))
}

/// Interpret a raw file as a transient run: a `time` axis plus node series.
fn transient(table: RawTable) -> Result<Transient, NgspiceError> {
    expect_plotname(&table, PLOTNAME_TRAN)?;
    let mut columns = real_columns(table.columns, "transient")?;
    let time = columns.remove(AXIS_TIME).ok_or_else(|| {
        NgspiceError::Parse("transient result missing time axis".into())
    })?;
    Ok(Transient::new(time, node_signals(columns)))
}

/// Interpret a raw file as an AC sweep: a `frequency` axis plus complex node series.
fn spectra(table: RawTable) -> Result<Spectra, NgspiceError> {
    expect_plotname(&table, PLOTNAME_AC)?;
    let Columns::Complex(mut columns) = table.columns else {
        return Err(NgspiceError::Parse(
            "AC result is not complex-valued".into(),
        ));
    };
    let frequency = columns
        .remove(AXIS_FREQUENCY)
        .ok_or_else(|| {
            NgspiceError::Parse("AC result missing frequency axis".into())
        })?
        .into_iter()
        .map(|c| c.re)
        .collect();
    Ok(Spectra::new(frequency, node_signals(columns)))
}

/// Error unless the raw file's plotname is the one this analysis produces.
fn expect_plotname(
    table: &RawTable,
    expected: &str,
) -> Result<(), NgspiceError> {
    if table.plotname == expected {
        Ok(())
    } else {
        Err(NgspiceError::Parse(format!(
            "expected plotname {expected:?}, got {:?}",
            table.plotname
        )))
    }
}

fn real_columns(
    columns: Columns,
    what: &str,
) -> Result<BTreeMap<String, Vec<f64>>, NgspiceError> {
    match columns {
        Columns::Real(m) => Ok(m),
        Columns::Complex(_) => Err(NgspiceError::Parse(format!(
            "{what} result is not real-valued"
        ))),
    }
}

/// Re-key voltage columns (`v(node)`) by bare node name, dropping non-voltage vars.
fn node_signals<T>(
    columns: BTreeMap<String, Vec<T>>,
) -> BTreeMap<String, Vec<T>> {
    columns
        .into_iter()
        .filter_map(|(var, series)| Some((node_name(&var)?.to_owned(), series)))
        .collect()
}

/// The node inside a voltage variable: `v(out)` -> `out`; `None` for other vars.
fn node_name(var: &str) -> Option<&str> {
    var.strip_prefix("v(")?.strip_suffix(')')
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
    use crate::{Sweep, Tolerance};

    const PARSE_EPS: Tolerance = Tolerance::Abs(1e-12);

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
        let path =
            format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(path).unwrap()
    }

    fn assert_parse_err(raw: &str, needle: &str) {
        let err = parse_table(raw).expect_err("expected parse failure");
        let msg = err.to_string();
        assert!(
            msg.contains(needle),
            "error {msg:?} did not mention {needle:?}"
        );
    }

    #[test]
    fn parses_op_point() {
        let table =
            parse_table(&fixture("voltage-divider/divider.raw")).unwrap();
        let op = operating_point(table).unwrap();
        assert!(op.node("vin").unwrap().near(5.0, PARSE_EPS));
        assert!(op.node("vout").unwrap().near(2.5, PARSE_EPS));
        assert!(op.node("nope").is_none());
    }

    #[test]
    fn reshapes_multiple_points() {
        let out = transient(parse_table(TRANSIENT).unwrap()).unwrap();
        let out = out.node("out").expect("node present");
        assert_eq!(out.time(), &[0.0, 1.0e-3]);
        assert_eq!(out.values(), &[1.0, 2.0]);
    }

    #[test]
    fn parses_complex_ac() {
        let spectra = spectra(parse_table(AC).unwrap()).unwrap();
        let out = spectra.node("out").expect("response present");
        assert_eq!(out.frequency(), &[1.0, 10.0]);
        assert_eq!(out.values()[0], Complex64::new(0.6, -0.8));
    }

    #[test]
    fn rejects_wrong_domain_plotname() {
        // A transient raw file handed to the operating-point interpreter.
        let err = operating_point(parse_table(TRANSIENT).unwrap())
            .expect_err("wrong plotname");
        assert!(err.to_string().contains("Operating Point"));
    }

    #[test]
    fn rejects_transient_without_time_axis() {
        let raw = "\
Plotname: Transient Analysis
Flags: real
No. Variables: 1
No. Points: 1
Variables:
 0 v(out) voltage
Values:
 0 1.0
";
        let err = transient(parse_table(raw).unwrap())
            .expect_err("missing time axis");
        assert!(err.to_string().contains("time axis"));
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
            "Plotname: Operating Point\nNo. Variables: 1\nNo. Points: 1\nVariables:\n 0 v(out) voltage\n",
            "Values",
        );
    }

    #[test]
    fn rejects_point_count_mismatch() {
        let raw = "Plotname: Operating Point\nNo. Variables: 1\nNo. Points: 2\nVariables:\n 0 v(out) voltage\nValues:\n 0 1.0\n";
        assert_parse_err(raw, "expected");
    }

    #[test]
    fn rejects_non_numeric_value() {
        let raw = "Plotname: Operating Point\nNo. Variables: 1\nNo. Points: 1\nVariables:\n 0 v(out) voltage\nValues:\n 0 oops\n";
        assert_parse_err(raw, "bad value");
    }

    #[test]
    fn tran_command_renders() {
        assert_eq!(
            tran_command(&Tran {
                step: 1e-3,
                stop: 5e-3,
            }),
            "tran 0.001 0.005"
        );
    }

    #[test]
    fn ac_command_renders() {
        assert_eq!(
            ac_command(&Ac {
                sweep: Sweep::Dec,
                points: 10,
                fstart: 1.0,
                fstop: 1e6,
            }),
            "ac dec 10 1 1000000"
        );
    }

    #[test]
    fn build_deck_wraps_netlist() {
        let deck =
            build_deck("* t\nv1 a 0 dc 1\n", "op", Path::new("/tmp/out.raw"));
        assert!(deck.contains("* t\nv1 a 0 dc 1\n"));
        assert!(deck.contains("\nop\n"));
        assert!(deck.contains("\nset filetype=ascii\n"));
        assert!(deck.contains("\nwrite /tmp/out.raw\n"));
        assert!(deck.contains("\n.endc\n"));
        assert!(deck.ends_with(".end\n"));
    }
}
