//! The scope's command protocol: JSON-RPC 2.0, one message per line.
//!
//! Every action the window offers is a [`Command`]. The menus, the live
//! window's socket, and `kitest-scope query` all apply commands the same way.

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::plot::Measurement;

/// The protocol version every message carries.
const JSONRPC: &str = "2.0";
pub const IPC_TIMEOUT: Duration = Duration::from_secs(30);

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
#[serde(
    tag = "method",
    content = "params",
    rename_all = "snake_case",
    deny_unknown_fields
)]
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

/// A parsed request: its id, kept for the reply, and its command.
#[derive(Debug)]
pub struct Call {
    pub id: Option<Json>,
    pub command: Result<Command, Error>,
}

/// The request in `line`, for the headless command path.
pub fn parse(line: &str) -> Call {
    match serde_json::from_str(line) {
        Ok(json) => parse_request(json),
        Err(error) => invalid(PARSE_ERROR, error.to_string()),
    }
}

/// One wire message, possibly a batch.
pub struct Message {
    pub calls: Vec<Call>,
    pub batch: bool,
}

/// Parses a complete JSON-RPC message.
pub fn parse_message(line: &str) -> Message {
    match serde_json::from_str::<Json>(line) {
        Ok(Json::Array(values)) if !values.is_empty() => Message {
            calls: values.into_iter().map(parse_request).collect(),
            batch: true,
        },
        Ok(json) => Message {
            calls: vec![parse_request(json)],
            batch: false,
        },
        Err(error) => Message {
            calls: vec![invalid(PARSE_ERROR, error.to_string())],
            batch: false,
        },
    }
}

fn invalid(code: i64, message: impl Into<String>) -> Call {
    Call {
        id: Some(Json::Null),
        command: Err(Error {
            code,
            message: message.into(),
        }),
    }
}

fn parse_request(json: Json) -> Call {
    let Some(request) = json.as_object() else {
        return invalid(INVALID_REQUEST, "request must be an object");
    };
    let id = request.get("id").cloned();
    if id.as_ref().is_some_and(|id| {
        !matches!(id, Json::Null | Json::String(_) | Json::Number(_))
    }) {
        return invalid(
            INVALID_REQUEST,
            "id must be a string, number, or null",
        );
    }
    if request.get("jsonrpc").and_then(Json::as_str) != Some(JSONRPC) {
        return invalid(
            INVALID_REQUEST,
            format!("jsonrpc must be \"{JSONRPC}\""),
        );
    }
    let Some(method) = request.get("method").and_then(Json::as_str) else {
        return invalid(INVALID_REQUEST, "method must be a string");
    };
    let command = (|| {
        if !METHODS.contains(&method) {
            return Err(Error {
                code: METHOD_NOT_FOUND,
                message: format!(
                    "no method {method:?}; the scope has {}",
                    METHODS.join(", ")
                ),
            });
        }
        let params = request.get("params").filter(|params| !params.is_null());
        if params.is_some_and(|params| !params.is_object()) {
            return Err(Error::invalid("params must be an object"));
        }
        let mut tagged = serde_json::Map::new();
        tagged.insert("method".to_owned(), Json::String(method.to_owned()));
        if matches!(method, "state" | "fit") {
            if params
                .is_some_and(|params| !params.as_object().unwrap().is_empty())
            {
                return Err(Error::invalid("method takes no parameters"));
            }
        } else if let Some(params) = params {
            tagged.insert("params".to_owned(), params.clone());
        }
        serde_json::from_value(Json::Object(tagged))
            .map_err(|error| Error::invalid(error.to_string()))
    })();
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
    let stream = match UnixStream::connect(socket) {
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
    call_stream(stream, method, params, IPC_TIMEOUT)
}

fn call_stream(
    mut stream: UnixStream,
    method: &str,
    params: Option<Json>,
    timeout: Duration,
) -> Result<Json, ClientError> {
    let deadline = Instant::now() + timeout;
    let remaining = || {
        deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "scope IPC deadline exceeded",
                )
            })
    };
    let mut request =
        serde_json::json!({ "jsonrpc": JSONRPC, "id": 1, "method": method });
    if let Some(params) = params {
        request["params"] = params;
    }
    let wire = format!("{request}\n");
    let mut bytes = wire.as_bytes();
    while !bytes.is_empty() {
        stream.set_write_timeout(Some(remaining()?))?;
        match stream.write(bytes) {
            Ok(0) => {
                return Err(std::io::Error::from(
                    std::io::ErrorKind::WriteZero,
                )
                .into());
            }
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                continue;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let mut line = Vec::new();
    loop {
        stream.set_read_timeout(Some(remaining()?))?;
        let mut buffer = [0; 4096];
        match stream.read(&mut buffer) {
            Ok(0) => {
                return Err(ClientError::Reply(
                    "connection closed before reply".into(),
                ));
            }
            Ok(count) => {
                if let Some(end) =
                    buffer[..count].iter().position(|byte| *byte == b'\n')
                {
                    line.extend_from_slice(&buffer[..end]);
                    break;
                }
                line.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                continue;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let malformed =
        || ClientError::Reply(String::from_utf8_lossy(&line).into_owned());
    let reply: Json = serde_json::from_slice(&line).map_err(|_| malformed())?;
    if reply.get("jsonrpc").and_then(Json::as_str) != Some(JSONRPC)
        || reply.get("id") != Some(&serde_json::json!(1))
        || reply.get("result").is_some() == reply.get("error").is_some()
    {
        return Err(malformed());
    }
    if let Some(error) = reply.get("error") {
        let code = error
            .get("code")
            .and_then(Json::as_i64)
            .ok_or_else(malformed)?;
        let message = error
            .get("message")
            .and_then(Json::as_str)
            .ok_or_else(malformed)?;
        return Err(ClientError::Refused {
            code,
            message: message.to_owned(),
        });
    }
    reply.get("result").cloned().ok_or_else(malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_null_ids_and_invalid_ids_are_distinct() {
        assert_eq!(parse(r#"{"jsonrpc":"2.0","method":"fit"}"#).id, None);
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":null,"method":"fit"}"#).id,
            Some(Json::Null)
        );
        for id in ["true", "[]", "{}"] {
            let call = parse(&format!(
                r#"{{"jsonrpc":"2.0","id":{id},"method":"fit"}}"#
            ));
            assert_eq!(call.id, Some(Json::Null));
            assert_eq!(call.command.unwrap_err().code, INVALID_REQUEST);
        }
        let notification = parse(r#"{"jsonrpc":"2.0","method":"dance"}"#);
        assert_eq!(notification.id, None);
        assert_eq!(notification.command.unwrap_err().code, METHOD_NOT_FOUND);
    }

    #[test]
    fn unit_methods_accept_empty_objects_but_reject_invalid_params() {
        for method in ["state", "fit"] {
            assert!(parse(&format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{{}}}}"#)).command.is_ok());
            for params in [r#"{"extra":1}"#, "[]", "1", "true", r#""text""#] {
                assert_eq!(parse(&format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#)).command.unwrap_err().code, INVALID_PARAMS);
            }
        }
        assert_eq!(parse(r#"{"jsonrpc":"2.0","id":1,"method":"cursors","params":{"typo":1}}"#).command.unwrap_err().code, INVALID_PARAMS);
    }

    #[test]
    fn client_rejects_mismatched_or_malformed_envelopes() {
        for response in [
            r#"{"jsonrpc":"1.0","id":1,"result":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"result":{}}"#,
            r#"{"jsonrpc":"2.0","result":{}}"#,
            r#"{"jsonrpc":"2.0","id":1,"result":{},"error":{"code":-1,"message":"no"}}"#,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":"bad","message":4}}"#,
        ] {
            let (client, mut server) = UnixStream::pair().unwrap();
            let response = response.to_owned();
            let handle = std::thread::spawn(move || {
                let mut request = String::new();
                std::io::BufRead::read_line(
                    &mut std::io::BufReader::new(&mut server),
                    &mut request,
                )
                .unwrap();
                assert!(request.ends_with('\n'));
                writeln!(server, "{response}").unwrap();
            });
            assert!(matches!(
                call_stream(client, "state", None, Duration::from_secs(1)),
                Err(ClientError::Reply(_))
            ));
            handle.join().unwrap();
        }
    }

    #[test]
    fn client_read_and_write_have_deadlines() {
        let (client, server) = UnixStream::pair().unwrap();
        let start = Instant::now();
        assert!(matches!(
            call_stream(client, "state", None, Duration::from_millis(30)),
            Err(ClientError::Io(_))
        ));
        assert!(start.elapsed() < Duration::from_secs(2));
        drop(server);

        let (client, server) = UnixStream::pair().unwrap();
        let start = Instant::now();
        let params = serde_json::json!({"path": "x".repeat(2 * 1024 * 1024)});
        assert!(matches!(
            call_stream(
                client,
                "open",
                Some(params),
                Duration::from_millis(30)
            ),
            Err(ClientError::Io(_))
        ));
        assert!(start.elapsed() < Duration::from_secs(2));
        drop(server);
    }

    #[test]
    fn trickling_reply_cannot_extend_the_deadline() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let handle = std::thread::spawn(move || {
            let mut request = String::new();
            std::io::BufRead::read_line(
                &mut std::io::BufReader::new(&mut server),
                &mut request,
            )
            .unwrap();
            assert!(request.ends_with('\n'));
            for _ in 0..20 {
                if server.write_all(b" ").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let start = Instant::now();
        assert!(matches!(
            call_stream(client, "state", None, Duration::from_millis(40)),
            Err(ClientError::Io(_))
        ));
        assert!(start.elapsed() < Duration::from_millis(150));
        handle.join().unwrap();
    }

    fn command(line: &str) -> Result<Command, Error> {
        parse(line).command
    }

    #[test]
    fn a_request_parses_to_its_command_and_keeps_its_id() {
        let call = parse(
            r#"{"jsonrpc":"2.0","id":7,"method":"measure","params":{"trace":"/OUT","measurements":["rms","half_power"],"over":[1e-6,2e-6]}}"#,
        );
        assert_eq!(call.id, Some(serde_json::json!(7)));
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
