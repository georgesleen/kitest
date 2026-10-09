//! kitest's scope: a viewer for simulation traces, and the file it opens.
//!
//! A writer such as kitest depends on this crate with default features off,
//! which leaves only the [`Capture`] file format. The `viewer` feature adds the
//! window, run as the `kitest-scope` binary.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The format version this crate writes and reads.
pub const VERSION: u32 = 2;

/// One simulation run's traces and the expectations checked on them, as
/// saved to a scope file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capture {
    /// What the capture shows, the same across re-runs, such as a probe's name.
    pub name: String,
    /// The corner the run was at, such as `VCC=9 V`, if it was at one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner: Option<String>,
    /// The traces.
    #[serde(flatten)]
    pub data: Data,
    /// What the run was checked for, drawn over the traces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectations: Vec<Expectation>,
}

/// A capture's traces over its axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Data {
    /// Node voltages over time, in seconds.
    Transient { time: Vec<f64>, traces: Vec<Trace> },
    /// Complex node responses over frequency, in hertz: a Bode plot's data.
    Ac {
        frequency: Vec<f64>,
        traces: Vec<AcTrace>,
    },
}

/// One node's real values, one per point of the capture's axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    pub name: String,
    pub values: Vec<f64>,
}

/// One node's complex response, split into real and imaginary parts, one per
/// point of the capture's frequency axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcTrace {
    pub name: String,
    pub re: Vec<f64>,
    pub im: Vec<f64>,
}

/// One check on a trace: where it expected the trace to be, and whether it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expectation {
    /// The trace checked, by name.
    pub trace: String,
    pub passed: bool,
    /// What was measured against what was expected.
    pub message: String,
    pub region: Region,
}

/// Where an expectation holds a trace.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Region {
    /// Between `low` and `high` from `start` to `end` on the capture's axis.
    Band {
        start: f64,
        end: f64,
        low: f64,
        high: f64,
    },
    /// A dominant frequency between `low` and `high` hertz.
    Frequency { low: f64, high: f64 },
}

/// A scope file's version alone.
#[derive(Deserialize)]
struct Version {
    version: u32,
}

/// The file around a capture: its format version, then the capture.
#[derive(Serialize, Deserialize)]
struct File<C> {
    version: u32,
    #[serde(flatten)]
    capture: C,
}

impl Capture {
    /// A capture of `data` named `name`, at no corner and with no expectations.
    pub fn new(name: impl Into<String>, data: Data) -> Self {
        Self {
            name: name.into(),
            corner: None,
            data,
            expectations: Vec::new(),
        }
    }

    /// The file name a writer saves the capture under, the same across
    /// re-runs of the same name and corner.
    pub fn file_name(&self) -> String {
        let mut stem = self.name.clone();
        if let Some(corner) = &self.corner {
            stem = format!("{stem} {corner}");
        }
        let stem: String = stem
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{stem}.json")
    }

    /// Write the capture to `path` as a scope file.
    pub fn save(&self, path: &Path) -> Result<(), FileError> {
        let file = File {
            version: VERSION,
            capture: self,
        };
        let text = serde_json::to_string(&file).map_err(FileError::Format)?;
        std::fs::write(path, text).map_err(FileError::Io)
    }

    /// Read the scope file at `path`.
    pub fn load(path: &Path) -> Result<Self, FileError> {
        let text = std::fs::read_to_string(path).map_err(FileError::Io)?;
        let version: Version =
            serde_json::from_str(&text).map_err(FileError::Format)?;
        if version.version != VERSION {
            return Err(FileError::Version(version.version));
        }
        let file: File<Capture> =
            serde_json::from_str(&text).map_err(FileError::Format)?;
        file.capture.check()?;
        Ok(file.capture)
    }

    /// The names of the capture's traces.
    pub fn trace_names(&self) -> Vec<&str> {
        match &self.data {
            Data::Transient { traces, .. } => {
                traces.iter().map(|trace| trace.name.as_str()).collect()
            }
            Data::Ac { traces, .. } => {
                traces.iter().map(|trace| trace.name.as_str()).collect()
            }
        }
    }

    /// Every trace has one value per point of the axis, and every expectation
    /// names a trace.
    fn check(&self) -> Result<(), FileError> {
        let short = |name: &str, len: usize, axis: usize| {
            (len != axis).then(|| FileError::Length {
                trace: name.to_owned(),
                len,
                axis,
            })
        };
        let problem = match &self.data {
            Data::Transient { time, traces } => {
                traces.iter().find_map(|trace| {
                    short(&trace.name, trace.values.len(), time.len())
                })
            }
            Data::Ac { frequency, traces } => traces.iter().find_map(|trace| {
                short(&trace.name, trace.re.len(), frequency.len()).or_else(
                    || short(&trace.name, trace.im.len(), frequency.len()),
                )
            }),
        };
        if let Some(problem) = problem {
            return Err(problem);
        }
        let names = self.trace_names();
        match self
            .expectations
            .iter()
            .find(|expectation| !names.contains(&expectation.trace.as_str()))
        {
            Some(expectation) => {
                Err(FileError::UnknownTrace(expectation.trace.clone()))
            }
            None => Ok(()),
        }
    }
}

/// Why a scope file could not be written or read.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("scope file: {0}")]
    Io(std::io::Error),
    #[error("scope file is not valid: {0}")]
    Format(serde_json::Error),
    #[error(
        "scope file is version {0}, but this scope reads version {VERSION}"
    )]
    Version(u32),
    #[error(
        "scope file trace {trace} has {len} values for an axis of {axis} points"
    )]
    Length {
        trace: String,
        len: usize,
        axis: usize,
    },
    #[error("scope file checks trace {0}, which it does not hold")]
    UnknownTrace(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bode() -> Capture {
        Capture {
            name: "PRB1".into(),
            corner: Some("VCC=9 V".into()),
            data: Data::Ac {
                frequency: vec![1.0, 10.0],
                traces: vec![AcTrace {
                    name: "vout".into(),
                    re: vec![1.0, 0.5],
                    im: vec![0.0, -0.5],
                }],
            },
            expectations: vec![Expectation {
                trace: "vout".into(),
                passed: false,
                message: "outside".into(),
                region: Region::Frequency {
                    low: 2.0,
                    high: 3.0,
                },
            }],
        }
    }

    #[test]
    fn a_saved_capture_loads_back_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(bode().file_name());
        bode().save(&path).unwrap();
        assert_eq!(Capture::load(&path).unwrap(), bode());
    }

    #[test]
    fn the_file_name_is_safe_and_names_the_corner() {
        assert_eq!(bode().file_name(), "PRB1_VCC_9_V.json");
        let mut slashed = bode();
        (slashed.name, slashed.corner) = ("/OUT".into(), None);
        assert_eq!(slashed.file_name(), "_OUT.json");
    }

    #[test]
    fn a_file_from_another_version_or_with_a_short_trace_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        let write = |text: &str| std::fs::write(&path, text).unwrap();

        write(r#"{"version":1,"kind":"transient","time":[],"traces":[]}"#);
        assert!(matches!(Capture::load(&path), Err(FileError::Version(1))));

        write(concat!(
            r#"{"version":2,"name":"n","kind":"transient","time":[0,1],"#,
            r#""traces":[{"name":"vout","values":[0]}]}"#
        ));
        let Err(FileError::Length { trace, len, axis }) = Capture::load(&path)
        else {
            panic!("a short trace loaded");
        };
        assert_eq!((trace.as_str(), len, axis), ("vout", 1, 2));
    }

    #[test]
    fn an_expectation_on_a_missing_trace_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        let mut capture = bode();
        capture.expectations[0].trace = "vin".into();
        capture.save(&path).unwrap();
        assert!(
            matches!(Capture::load(&path), Err(FileError::UnknownTrace(trace)) if trace == "vin")
        );
    }
}
