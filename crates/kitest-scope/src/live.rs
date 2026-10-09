//! The one live scope window and the paths later launches hand to it.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

/// The result of starting the scope: this process owns the window, or handed
/// its path to the process that does.
pub enum Instance {
    Owner {
        paths: Receiver<PathBuf>,
        _socket: Socket,
    },
    Forwarded,
}

/// Removes the single-instance socket when its window closes.
pub struct Socket {
    path: PathBuf,
}

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Hands `path` to the running scope, or listens for paths from later launches.
pub fn connect_or_listen(path: &Path) -> std::io::Result<Instance> {
    let socket = socket_path();
    match UnixStream::connect(&socket) {
        Ok(mut stream) => {
            writeln!(stream, "{}", path.display())?;
            return Ok(Instance::Forwarded);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            let _ = std::fs::remove_file(&socket);
        }
    }

    let listener = UnixListener::bind(&socket)?;
    let (send, paths) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if send.send(PathBuf::from(line)).is_err() {
                    return;
                }
            }
        }
    });
    Ok(Instance::Owner {
        paths,
        _socket: Socket { path: socket },
    })
}

/// The socket path private to this user's runtime directory.
fn socket_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("kitest-scope.sock")
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Read as _, Write as _};

    use super::socket_path;

    #[test]
    fn socket_uses_the_runtime_directory() {
        let path = socket_path();
        assert_eq!(path.file_name().unwrap(), "kitest-scope.sock");
        assert!(path.is_absolute());
    }

    #[test]
    fn one_unix_stream_carries_one_path_line() {
        let (mut writer, reader) =
            std::os::unix::net::UnixStream::pair().unwrap();
        writeln!(writer, "/tmp/capture.json").unwrap();
        drop(writer);
        let mut text = String::new();
        BufReader::new(reader).read_to_string(&mut text).unwrap();
        assert_eq!(text, "/tmp/capture.json\n");
    }
}
