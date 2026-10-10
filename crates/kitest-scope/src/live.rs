//! The one live scope window and its line-delimited JSON-RPC socket.

use std::fs::{File, OpenOptions};
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::fs::{
    DirBuilderExt as _, FileTypeExt as _, MetadataExt as _, OpenOptionsExt as _,
};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};

use eframe::egui;

use crate::control::{self, Call};

/// One request for the window and its reply channel.
pub struct Envelope {
    pub call: Call,
    pub reply: Sender<String>,
}

/// This process owns the window or forwarded its capture to the owner.
pub enum Instance {
    Owner {
        requests: Receiver<Envelope>,
        context: Arc<OnceLock<egui::Context>>,
        socket: Socket,
    },
    Forwarded,
}

/// The socket inode owned by this window.
pub struct Socket {
    path: PathBuf,
    device: u64,
    inode: u64,
    lock: File,
}

impl Drop for Socket {
    fn drop(&mut self) {
        if let Err(error) = self.lock.lock() {
            eprintln!("scope socket cleanup lock: {error}");
            return;
        }
        if let Ok(metadata) = std::fs::symlink_metadata(&self.path)
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
            && let Err(error) = std::fs::remove_file(&self.path)
        {
            eprintln!("scope socket cleanup: {error}");
        }
        if let Err(error) = self.lock.unlock() {
            eprintln!("scope socket cleanup unlock: {error}");
        }
    }
}

/// Opens `path` in the live window or atomically acquires its socket.
pub fn connect_or_listen(path: &Path) -> std::io::Result<Instance> {
    connect_or_listen_at(path, &socket_path()?)
}

fn ownership_lock(socket: &Path) -> std::io::Result<File> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(socket.with_extension("lock"))?;
    let metadata = lock.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != effective_uid()
        || metadata.mode() & 0o077 != 0
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "scope lock must be a private user-owned file",
        ));
    }
    Ok(lock)
}

fn connect_or_listen_at(
    path: &Path,
    socket: &Path,
) -> std::io::Result<Instance> {
    validate_directory(
        socket
            .parent()
            .ok_or_else(|| std::io::Error::other("socket has no directory"))?,
        effective_uid(),
    )?;
    let lock = ownership_lock(socket)?;
    lock.lock()?;
    match std::fs::symlink_metadata(socket) {
        Ok(metadata) => {
            if !metadata.file_type().is_socket()
                || metadata.uid() != effective_uid()
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "scope endpoint is not a user-owned socket",
                ));
            }
            match UnixStream::connect(socket) {
                Ok(stream) => {
                    drop(stream);
                    lock.unlock()?;
                    let path = std::path::absolute(path)?;
                    let params = serde_json::json!({ "path": path });
                    return control::call(socket, "open", Some(params))
                        .map(|_| Instance::Forwarded)
                        .map_err(|error| {
                            std::io::Error::other(error.to_string())
                        });
                }
                Err(error)
                    if error.kind()
                        == std::io::ErrorKind::ConnectionRefused =>
                {
                    let current = std::fs::symlink_metadata(socket)?;
                    if current.dev() != metadata.dev()
                        || current.ino() != metadata.ino()
                    {
                        return Err(std::io::Error::other(
                            "scope socket changed during acquisition",
                        ));
                    }
                    std::fs::remove_file(socket)?;
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(socket)?;
    let metadata = std::fs::symlink_metadata(socket)?;
    let socket = Socket {
        path: socket.to_owned(),
        device: metadata.dev(),
        inode: metadata.ino(),
        lock,
    };
    socket.lock.unlock()?;
    let (send, requests) = mpsc::channel();
    let context = Arc::new(OnceLock::new());
    let shared = Arc::clone(&context);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let send = send.clone();
                    let context = Arc::clone(&shared);
                    std::thread::spawn(move || serve(stream, &send, &context));
                }
                Err(error) => eprintln!("scope socket accept: {error}"),
            }
        }
    });
    Ok(Instance::Owner {
        requests,
        context,
        socket,
    })
}

/// Dispatches messages and aggregates completed batch replies on one line.
fn serve(
    stream: UnixStream,
    send: &Sender<Envelope>,
    context: &OnceLock<egui::Context>,
) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    if writer
        .set_write_timeout(Some(control::IPC_TIMEOUT))
        .is_err()
    {
        return;
    }
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let message = control::parse_message(&line);
        let mut answers = Vec::new();
        for call in message.calls {
            let (reply, answer) = mpsc::channel();
            if call.id.is_some() {
                answers.push(answer);
            }
            if send.send(Envelope { call, reply }).is_err() {
                return;
            }
        }
        if let Some(context) = context.get() {
            context.request_repaint();
        }
        if answers.is_empty() {
            continue;
        }
        let mut replies = Vec::with_capacity(answers.len());
        for answer in answers {
            let Ok(answer) = answer.recv() else { return };
            replies.push(answer);
        }
        let response = if message.batch {
            format!("[{}]", replies.join(","))
        } else {
            replies.pop().unwrap()
        };
        if writeln!(writer, "{response}").is_err() {
            return;
        }
    }
}

fn effective_uid() -> u32 {
    // geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

fn validate_directory(directory: &Path, uid: u32) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(directory)?;
    if !directory.is_absolute()
        || !metadata.is_dir()
        || metadata.uid() != uid
        || metadata.mode() & 0o077 != 0
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "scope runtime directory must be private and user-owned",
        ));
    }
    Ok(())
}

fn socket_path_for(
    runtime: Option<PathBuf>,
    temporary: &Path,
    uid: u32,
) -> std::io::Result<PathBuf> {
    let directory = match runtime {
        Some(directory) => directory,
        None => {
            let directory = temporary.join(format!("kitest-scope-{uid}"));
            match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
                Ok(()) => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            directory
        }
    };
    validate_directory(&directory, uid)?;
    Ok(directory.join("kitest-scope.sock"))
}

/// The socket path in a validated private per-user directory.
pub fn socket_path() -> std::io::Result<PathBuf> {
    socket_path_for(
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
        &std::env::temp_dir(),
        effective_uid(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::Command;

    fn reply_to_requests(requests: Receiver<Envelope>) {
        for envelope in requests {
            if let Some(id) = envelope.call.id {
                let outcome = envelope
                    .call
                    .command
                    .map(|_| serde_json::json!({"fitted": true}));
                let _ = envelope.reply.send(control::reply(id, outcome));
            }
        }
    }

    fn test_connection() -> UnixStream {
        let (client, server) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let (send, requests) = mpsc::channel::<Envelope>();
        std::thread::spawn(move || serve(server, &send, &OnceLock::new()));
        std::thread::spawn(move || reply_to_requests(requests));
        client
    }

    fn private_directory() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(
            directory.path(),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        directory
    }

    #[test]
    fn a_batch_waits_for_deferred_replies_without_blocking_dispatch() {
        let (mut client, server) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let (send, requests) = mpsc::channel();
        std::thread::spawn(move || serve(server, &send, &OnceLock::new()));
        let responder = std::thread::spawn(move || {
            let png = requests.recv().unwrap();
            assert!(matches!(
                png.call.command,
                Ok(Command::Save {
                    format: control::Format::Png,
                    ..
                })
            ));
            let state = requests
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            assert_eq!(state.call.command, Ok(Command::State));
            state
                .reply
                .send(control::reply(
                    state.call.id.unwrap(),
                    Ok(serde_json::json!("state")),
                ))
                .unwrap();
            png.reply
                .send(control::reply(
                    png.call.id.unwrap(),
                    Ok(serde_json::json!("png")),
                ))
                .unwrap();
        });
        writeln!(client, r#"[{{"jsonrpc":"2.0","id":1,"method":"save","params":{{"format":"png","path":"/tmp/test.png"}}}},{{"jsonrpc":"2.0","id":2,"method":"state"}}]"#).unwrap();
        let mut line = String::new();
        BufReader::new(client).read_line(&mut line).unwrap();
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response[0]["result"], "png");
        assert_eq!(response[1]["result"], "state");
        responder.join().unwrap();
    }

    #[test]
    fn a_client_gets_the_reply_the_window_gives_its_request() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("scope.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (send, requests) = mpsc::channel::<Envelope>();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve(stream, &send, &OnceLock::new());
        });
        std::thread::spawn(move || {
            let envelope = requests.recv().unwrap();
            assert_eq!(envelope.call.command, Ok(Command::Fit));
            envelope
                .reply
                .send(control::reply(
                    envelope.call.id.unwrap(),
                    Ok(serde_json::json!({"fitted": true})),
                ))
                .unwrap();
        });
        assert_eq!(
            control::call(&socket, "fit", None).unwrap(),
            serde_json::json!({"fitted": true})
        );
    }

    #[test]
    fn notification_then_request_has_only_the_request_reply() {
        let mut client = test_connection();
        writeln!(client, r#"{{"jsonrpc":"2.0","method":"fit"}}"#).unwrap();
        writeln!(client, r#"{{"jsonrpc":"2.0","id":8,"method":"fit"}}"#)
            .unwrap();
        let mut line = String::new();
        BufReader::new(client).read_line(&mut line).unwrap();
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], 8);
    }

    #[test]
    fn mixed_batch_aggregates_errors_and_skips_notifications() {
        let mut client = test_connection();
        writeln!(client, r#"[{{"jsonrpc":"2.0","method":"fit"}},{{"jsonrpc":"2.0","id":9,"method":"fit","params":{{}}}},false,{{"jsonrpc":"2.0","id":null,"method":"dance"}}]"#).unwrap();
        let mut line = String::new();
        BufReader::new(client).read_line(&mut line).unwrap();
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response.as_array().unwrap().len(), 3);
        assert_eq!(response[0]["id"], 9);
        assert_eq!(response[1]["error"]["code"], -32600);
        assert_eq!(response[2]["id"], serde_json::Value::Null);
        assert_eq!(response[2]["error"]["code"], -32601);
    }

    #[test]
    fn invalid_message_does_not_kill_the_connection() {
        let mut client = test_connection();
        writeln!(client, "[]").unwrap();
        writeln!(client, "not json").unwrap();
        writeln!(client, r#"{{"jsonrpc":"2.0","id":3,"method":"fit"}}"#)
            .unwrap();
        let mut reader = BufReader::new(client);
        for code in [-32600, -32700] {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let response: serde_json::Value =
                serde_json::from_str(&line).unwrap();
            assert_eq!(response["error"]["code"], code);
        }
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap()["id"],
            3
        );
    }

    #[test]
    fn dropping_an_owner_does_not_remove_a_replacement_socket() {
        let directory = private_directory();
        let path = directory.path().join("scope.sock");
        let owner =
            connect_or_listen_at(Path::new("capture.json"), &path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();
        drop(owner);
        assert!(path.exists(), "owner removed another listener's socket");
        drop(replacement);
    }

    #[test]
    fn concurrent_starts_have_one_owner() {
        let directory = private_directory();
        let socket = directory.path().join("scope.sock");
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let (owners, owned) = mpsc::channel();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let socket = socket.clone();
                let barrier = Arc::clone(&barrier);
                let owners = owners.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    match connect_or_listen_at(
                        Path::new("capture.json"),
                        &socket,
                    )
                    .unwrap()
                    {
                        Instance::Owner {
                            requests, socket, ..
                        } => {
                            std::thread::spawn(move || {
                                reply_to_requests(requests)
                            });
                            owners.send(socket).unwrap();
                            true
                        }
                        Instance::Forwarded => false,
                    }
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .map(|handle| usize::from(handle.join().unwrap()))
                .sum::<usize>(),
            1
        );
        let owner = owned.recv().unwrap();
        assert!(socket.exists());
        drop(owner);
        assert!(!socket.exists());
    }

    #[test]
    fn fallback_is_private_and_per_user() {
        let directory = tempfile::tempdir().unwrap();
        let uid = effective_uid();
        let path = socket_path_for(None, directory.path(), uid).unwrap();
        assert_eq!(
            path.parent().unwrap().file_name().unwrap(),
            format!("kitest-scope-{uid}").as_str()
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap()).unwrap().mode() & 0o777,
            0o700
        );
        assert!(socket_path_for(None, directory.path(), uid + 1).is_err());
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(
            path.parent().unwrap(),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert!(socket_path_for(None, directory.path(), uid).is_err());
    }

    #[test]
    fn unsafe_endpoint_is_not_unlinked() {
        let directory = private_directory();
        let socket = directory.path().join("scope.sock");
        std::fs::write(&socket, "not a socket").unwrap();
        assert!(
            connect_or_listen_at(Path::new("capture.json"), &socket).is_err()
        );
        assert_eq!(std::fs::read_to_string(&socket).unwrap(), "not a socket");
    }

    #[test]
    fn stale_socket_is_replaced() {
        let directory = private_directory();
        let socket = directory.path().join("scope.sock");
        drop(UnixListener::bind(&socket).unwrap());
        assert!(matches!(
            connect_or_listen_at(Path::new("capture.json"), &socket).unwrap(),
            Instance::Owner { .. }
        ));
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
