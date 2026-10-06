//! kitest's scope: a viewer for simulation traces, and the file it opens.
//!
//! A writer such as kitest depends on this crate with default features off,
//! which leaves only the [`Capture`] file format. The `viewer` feature adds the
//! window, run as the `kitest-scope` binary.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The format version this crate writes and reads.
pub const VERSION: u32 = 1;

/// One simulation run's traces, as saved to a scope file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capture {
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

/// The file around a capture: its format version, then the capture.
#[derive(Serialize, Deserialize)]
struct File<C> {
    version: u32,
    #[serde(flatten)]
    capture: C,
}

impl Capture {
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
        let file: File<Capture> =
            serde_json::from_str(&text).map_err(FileError::Format)?;
        if file.version != VERSION {
            return Err(FileError::Version(file.version));
        }
        file.capture.check()?;
        Ok(file.capture)
    }

    /// Every trace has one value per point of the axis.
    fn check(&self) -> Result<(), FileError> {
        let short = |name: &str, len: usize, axis: usize| {
            (len != axis).then(|| FileError::Length {
                trace: name.to_owned(),
                len,
                axis,
            })
        };
        let problem = match self {
            Capture::Transient { time, traces } => {
                traces.iter().find_map(|trace| {
                    short(&trace.name, trace.values.len(), time.len())
                })
            }
            Capture::Ac { frequency, traces } => {
                traces.iter().find_map(|trace| {
                    short(&trace.name, trace.re.len(), frequency.len()).or_else(
                        || short(&trace.name, trace.im.len(), frequency.len()),
                    )
                })
            }
        };
        problem.map_or(Ok(()), Err)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bode() -> Capture {
        Capture::Ac {
            frequency: vec![1.0, 10.0],
            traces: vec![AcTrace {
                name: "vout".into(),
                re: vec![1.0, 0.5],
                im: vec![0.0, -0.5],
            }],
        }
    }

    #[test]
    fn a_saved_capture_loads_back_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bode.json");
        bode().save(&path).unwrap();
        assert_eq!(Capture::load(&path).unwrap(), bode());
    }

    #[test]
    fn a_file_from_another_version_or_with_a_short_trace_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        let write = |text: &str| std::fs::write(&path, text).unwrap();

        write(r#"{"version":2,"kind":"transient","time":[],"traces":[]}"#);
        assert!(matches!(Capture::load(&path), Err(FileError::Version(2))));

        write(concat!(
            r#"{"version":1,"kind":"transient","time":[0,1],"#,
            r#""traces":[{"name":"vout","values":[0]}]}"#
        ));
        let Err(FileError::Length { trace, len, axis }) = Capture::load(&path)
        else {
            panic!("a short trace loaded");
        };
        assert_eq!((trace.as_str(), len, axis), ("vout", 1, 2));
    }
}
