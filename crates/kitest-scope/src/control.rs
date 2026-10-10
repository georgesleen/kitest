//! The scope's command protocol: JSON-RPC 2.0, one message per line.
//!
//! Every action the window offers is a [`Command`]. The menus, the live
//! window's socket, and `kitest-scope query` all apply commands the same way.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::plot::Measurement;

/// The protocol version every message carries.
const JSONRPC: &str = "2.0";

/// The code for a request that is not JSON.
const PARSE_ERROR: i64 = -32700;
/// The code for JSON that is not a request.
const INVALID_REQUEST: i64 = -32600;
/// The code for a method the scope does not have.
const METHOD_NOT_FOUND: i64 = -32601;
/// The code for parameters a method cannot take.
pub const INVALID_PARAMS: i64 = -32602;
/// The code for a valid command the scope could not carry out.
pub const FAILED: i64 = -32000;

/// The methods the scope answers.
pub const METHODS: &[&str] = &[
    "open",
    "state",
    "view",
    "show",
    "zoom",
    "fit",
    "cursors",
    "measure",
    "measurements",
    "move",
    "reference",
    "trigger",
    "save",
];

/// One thing a client asks the scope to do.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Command {
    /// Open the capture at `path`, keeping the layout for the same capture.
    Open { path: PathBuf },
    /// Report what the window shows.
    State,
    /// Show one instrument: the capture's own view, or one derived from it.
    View { name: ViewName },
    /// Show or hide a trace.
    Show { trace: String, shown: bool },
    /// Show `x` on the x axis and `y` on pane `pane`'s y axis, in their units.
    Zoom {
        #[serde(default)]
        x: Option<[f64; 2]>,
        #[serde(default)]
        y: Option<[f64; 2]>,
        #[serde(default)]
        pane: usize,
    },
    /// Fit every axis to the shown traces.
    Fit,
    /// Place cursors A and B on the x axis; a missing or `null` one is removed.
    Cursors {
        #[serde(default)]
        a: Option<f64>,
        #[serde(default)]
        b: Option<f64>,
    },
    /// Measure one trace over `over`, or else between the cursors or across
    /// the visible window.
    Measure {
        trace: String,
        #[serde(default)]
        quantity: Option<String>,
        measurements: Vec<Measurement>,
        #[serde(default)]
        over: Option<[f64; 2]>,
    },
    /// Set what a pane measures in its strip.
    Measurements {
        pane: usize,
        measurements: Vec<Measurement>,
    },
    /// Move a trace's channel into pane `to`, or into a new pane.
    Move {
        trace: String,
        #[serde(default)]
        quantity: Option<String>,
        #[serde(default)]
        to: Option<usize>,
    },
    /// Divide every response by a trace's, or show absolute responses.
    Reference {
        #[serde(default)]
        trace: Option<String>,
    },
    /// Align each run's first crossing of `level` on `edge` to time zero, or
    /// clear the trigger when `trace` is missing.
    Trigger {
        #[serde(default)]
        trace: Option<String>,
        #[serde(default)]
        edge: Edge,
        #[serde(default)]
        level: Option<f64>,
    },
    /// Write the window as a PNG or the shown traces as CSV.
    Save { format: Format, path: PathBuf },
}

/// An instrument a capture is shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewName {
    /// The capture's own view: a waveform or a Bode plot.
    Primary,
    Spectrum,
    GroupDelay,
}

/// The direction of a trigger's crossing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    #[default]
    Rising,
    Falling,
}

/// What a save writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Png,
    Csv,
}

/// Why a command was refused, with its JSON-RPC error code.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Error {
    pub code: i64,
    pub message: String,
}

impl Error {
    /// A command whose parameters do not fit what the scope shows.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: INVALID_PARAMS,
            message: message.into(),
        }
    }

    /// A valid command the scope could not carry out.
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            code: FAILED,
            message: message.into(),
        }
    }
}

/// A request as it arrives, before its method is checked.
#[derive(Deserialize)]
struct Request {
    jsonrpc: String,
    #[serde(default)]
    id: Json,
    method: String,
    #[serde(default)]
    params: Option<Json>,
}

/// A parsed request: its id, kept for the reply, and its command.
#[derive(Debug)]
pub struct Call {
    pub id: Json,
    pub command: Result<Command, Error>,
}

/// The request in `line`.
pub fn parse(line: &str) -> Call {
    let request: Request = match serde_json::from_str::<Json>(line) {
        Err(error) => {
            return Call {
                id: Json::Null,
                command: Err(Error {
                    code: PARSE_ERROR,
                    message: error.to_string(),
                }),
            };
        }
        Ok(json) => {
            let id = json.get("id").cloned().unwrap_or(Json::Null);
            match serde_json::from_value(json) {
                Ok(request) => request,
                Err(error) => {
                    return Call {
                        id,
                        command: Err(Error {
                            code: INVALID_REQUEST,
                            message: error.to_string(),
                        }),
                    };
                }
            }
        }
    };
    let id = request.id;
    if request.jsonrpc != JSONRPC {
        return Call {
            id,
            command: Err(Error {
                code: INVALID_REQUEST,
                message: format!("jsonrpc must be \"{JSONRPC}\""),
            }),
        };
    }
    if !METHODS.contains(&request.method.as_str()) {
        return Call {
            id,
            command: Err(Error {
                code: METHOD_NOT_FOUND,
                message: format!(
                    "no method {:?}; the scope has {}",
                    request.method,
                    METHODS.join(", ")
                ),
            }),
        };
    }
    let mut tagged = serde_json::Map::new();
    tagged.insert("method".to_owned(), Json::String(request.method));
    if let Some(params) = request.params.filter(|params| !params.is_null()) {
        tagged.insert("params".to_owned(), params);
    }
    let command = serde_json::from_value(Json::Object(tagged))
        .map_err(|error| Error::invalid(error.to_string()));
    Call { id, command }
}

/// The reply line to the request `id`.
pub fn reply(id: Json, outcome: Result<Json, Error>) -> String {
    let message = match outcome {
        Ok(result) => {
            serde_json::json!({ "jsonrpc": JSONRPC, "id": id, "result": result })
        }
        Err(error) => {
            serde_json::json!({ "jsonrpc": JSONRPC, "id": id, "error": error })
        }
    };
    message.to_string()
}

/// Why a client could not get a reply.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("no scope window is running")]
    NoWindow,
    #[error("scope socket: {0}")]
    Io(#[from] std::io::Error),
    #[error("the scope sent a reply that is not JSON-RPC: {0}")]
    Reply(String),
    #[error("{message} (code {code})")]
    Refused { code: i64, message: String },
}

/// Sends `method` with `params` to the window at `socket` and returns its result.
pub fn call(
    socket: &Path,
    method: &str,
    params: Option<Json>,
) -> Result<Json, ClientError> {
    let mut stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound
                    | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err(ClientError::NoWindow);
        }
        Err(error) => return Err(error.into()),
    };
    let mut request =
        serde_json::json!({ "jsonrpc": JSONRPC, "id": 1, "method": method });
    if let Some(params) = params {
        request["params"] = params;
    }
    writeln!(stream, "{request}")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let reply: Json = serde_json::from_str(&line)
        .map_err(|_| ClientError::Reply(line.trim().to_owned()))?;
    if let Some(error) = reply.get("error") {
        return Err(ClientError::Refused {
            code: error.get("code").and_then(Json::as_i64).unwrap_or(FAILED),
            message: error
                .get("message")
                .and_then(Json::as_str)
                .unwrap_or("refused")
                .to_owned(),
        });
    }
    reply
        .get("result")
        .cloned()
        .ok_or_else(|| ClientError::Reply(line.trim().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(line: &str) -> Result<Command, Error> {
        parse(line).command
    }

    #[test]
    fn a_request_parses_to_its_command_and_keeps_its_id() {
        let call = parse(
            r#"{"jsonrpc":"2.0","id":7,"method":"measure","params":{"trace":"/OUT","measurements":["rms","half_power"],"over":[1e-6,2e-6]}}"#,
        );
        assert_eq!(call.id, serde_json::json!(7));
        assert_eq!(
            call.command,
            Ok(Command::Measure {
                trace: "/OUT".into(),
                quantity: None,
                measurements: vec![Measurement::Rms, Measurement::HalfPower],
                over: Some([1e-6, 2e-6]),
            })
        );
    }

    #[test]
    fn a_method_without_parameters_takes_none_or_null() {
        assert_eq!(
            command(r#"{"jsonrpc":"2.0","id":1,"method":"state"}"#),
            Ok(Command::State)
        );
        assert_eq!(
            command(r#"{"jsonrpc":"2.0","id":1,"method":"fit","params":null}"#),
            Ok(Command::Fit)
        );
        assert_eq!(
            command(
                r#"{"jsonrpc":"2.0","id":1,"method":"cursors","params":{"a":1.0}}"#
            ),
            Ok(Command::Cursors {
                a: Some(1.0),
                b: None
            })
        );
    }

    #[test]
    fn each_failure_has_its_json_rpc_code() {
        let code = |line: &str| command(line).unwrap_err().code;
        assert_eq!(code("not json"), PARSE_ERROR);
        assert_eq!(code(r#"{"jsonrpc":"2.0","id":1}"#), INVALID_REQUEST);
        assert_eq!(
            code(r#"{"jsonrpc":"1.0","id":1,"method":"state"}"#),
            INVALID_REQUEST
        );
        assert_eq!(
            code(r#"{"jsonrpc":"2.0","id":1,"method":"dance"}"#),
            METHOD_NOT_FOUND
        );
        assert_eq!(
            code(
                r#"{"jsonrpc":"2.0","id":1,"method":"view","params":{"name":"smith"}}"#
            ),
            INVALID_PARAMS
        );
    }

    #[test]
    fn every_method_name_parses() {
        let params = [
            ("open", r#"{"path":"/tmp/a.json"}"#),
            ("state", "null"),
            ("view", r#"{"name":"group_delay"}"#),
            ("show", r#"{"trace":"v","shown":false}"#),
            ("zoom", r#"{"x":[0,1]}"#),
            ("fit", "null"),
            ("cursors", "{}"),
            ("measure", r#"{"trace":"v","measurements":[]}"#),
            ("measurements", r#"{"pane":0,"measurements":["min"]}"#),
            ("move", r#"{"trace":"v"}"#),
            ("reference", "{}"),
            ("trigger", r#"{"trace":"v","edge":"falling","level":0.5}"#),
            ("save", r#"{"format":"csv","path":"/tmp/a.csv"}"#),
        ];
        assert_eq!(params.len(), METHODS.len());
        for (method, params) in params {
            let line = format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#
            );
            assert!(command(&line).is_ok(), "{line}: {:?}", command(&line));
        }
    }

    #[test]
    fn a_reply_carries_the_id_and_a_result_or_an_error() {
        let ok: Json = serde_json::from_str(&reply(
            serde_json::json!(3),
            Ok(serde_json::json!({"x": 1})),
        ))
        .unwrap();
        assert_eq!(
            ok,
            serde_json::json!({"jsonrpc":"2.0","id":3,"result":{"x":1}})
        );
        let refused: Json =
            serde_json::from_str(&reply(Json::Null, Err(Error::failed("no"))))
                .unwrap();
        assert_eq!(refused["error"]["code"], FAILED);
    }
}
