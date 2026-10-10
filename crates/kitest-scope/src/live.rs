//! The one live scope window, and the JSON-RPC requests clients send it over
//! a Unix socket.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};

use eframe::egui;

use crate::control::{self, Call};

/// One request for the window, and where its reply line goes.
pub struct Envelope {
    pub call: Call,
    pub reply: Sender<String>,
}

/// The result of starting the scope: this process owns the window, or handed
/// its capture to the process that does.
pub enum Instance {
    Owner {
        requests: Receiver<Envelope>,
        context: Arc<OnceLock<egui::Context>>,
        socket: Socket,
    },
    Forwarded,
}

/// Removes the socket when its window closes.
pub struct Socket {
    path: PathBuf,
}

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Opens `path` in the running window, or starts serving requests for this
/// one.
///
/// # Errors
///
/// When the socket cannot be bound, or the running window refuses `path`.
pub fn connect_or_listen(path: &Path) -> std::io::Result<Instance> {
    let socket = socket_path();
    if UnixStream::connect(&socket).is_ok() {
        let path = std::path::absolute(path)?;
        let params = serde_json::json!({ "path": path });
        return match control::call(&socket, "open", Some(params)) {
            Ok(_) => Ok(Instance::Forwarded),
            Err(error) => Err(std::io::Error::other(error.to_string())),
        };
    }
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    let (send, requests) = mpsc::channel();
    let context = Arc::new(OnceLock::new());
    let shared = Arc::clone(&context);
    std::thread::spawn(move || {
        for stream in listener.incoming().map_while(Result::ok) {
            let send = send.clone();
            let context = Arc::clone(&shared);
            std::thread::spawn(move || serve(stream, &send, &context));
        }
    });
    Ok(Instance::Owner {
        requests,
        context,
        socket: Socket { path: socket },
    })
}

/// Answers each request line on `stream` with the window's reply line.
fn serve(
    stream: UnixStream,
    send: &Sender<Envelope>,
    context: &OnceLock<egui::Context>,
) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let (reply, answer) = mpsc::channel();
        let envelope = Envelope {
            call: control::parse(&line),
            reply,
        };
        if send.send(envelope).is_err() {
            return;
        }
        if let Some(context) = context.get() {
            context.request_repaint();
        }
        let Ok(answer) = answer.recv() else { return };
        if writeln!(writer, "{answer}").is_err() {
            return;
        }
    }
}

/// The socket path, private to this user's runtime directory.
pub fn socket_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("kitest-scope.sock")
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;
    use std::sync::mpsc;

    use super::{Envelope, serve, socket_path};
    use crate::control::{self, Command};

    #[test]
    fn socket_uses_the_runtime_directory() {
        let path = socket_path();
        assert_eq!(path.file_name().unwrap(), "kitest-scope.sock");
        assert!(path.is_absolute());
    }

    #[test]
    fn a_client_gets_the_reply_the_window_gives_its_request() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("scope.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let (send, requests) = mpsc::channel::<Envelope>();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve(stream, &send, &OnceLock::new());
        });
        std::thread::spawn(move || {
            let envelope = requests.recv().unwrap();
            assert_eq!(envelope.call.command, Ok(Command::Fit));
            let reply = control::reply(
                envelope.call.id,
                Ok(serde_json::json!({"fitted": true})),
            );
            envelope.reply.send(reply).unwrap();
        });
        let result = control::call(&socket, "fit", None).unwrap();
        assert_eq!(result, serde_json::json!({"fitted": true}));
    }

    #[test]
    fn a_client_without_a_window_says_so() {
        let directory = tempfile::tempdir().unwrap();
        let error =
            control::call(&directory.path().join("none.sock"), "state", None)
                .unwrap_err();
        assert!(matches!(error, control::ClientError::NoWindow), "{error}");
    }
}
