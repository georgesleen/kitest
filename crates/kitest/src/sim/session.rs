//! A live transient simulation, held open in an `ngspice -p` pipe.

use std::io::{BufRead, BufReader, PipeReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

use tempfile::TempDir;

use super::exit_reason;
use super::ngspice::spawn_error;
use crate::analysis::key;
use crate::{Ngspice, NgspiceError, Voltage};

/// Relative tolerance between a requested time and the time the solver lands on.
const TIME_TOLERANCE: f64 = 1e-12;

/// Shortest advance, as a fraction of the maximum step, that ngspice keeps as
/// a breakpoint.
const RESOLUTION: f64 = 1e-3;

/// Line ngspice echoes back to mark the end of one command batch.
const SYNC: &str = "kitest-sync";

/// Prefix for the element names of drives, kept distinct from the design's own.
const DRIVE_PREFIX: &str = "vktdrive";

/// A voltage source that holds `node` against ground and that a session sets.
#[derive(Debug, Clone)]
pub struct Drive {
    node: String,
    volts: f64,
}

impl Drive {
    /// A drive on `node`, a SPICE node or schematic net name, at `volts`
    /// from time zero.
    pub fn new(node: &str, volts: f64) -> Self {
        Self {
            node: node.to_owned(),
            volts,
        }
    }
}

/// A transient simulation that the host advances, reads and changes.
///
/// Time only moves forward. A read returns the solution at the current
/// time. A drive change takes effect immediately after the current time,
/// so a read at the time of a change returns the value before it.
pub struct Session {
    child: Child,
    stdin: ChildStdin,
    output: BufReader<PipeReader>,
    drives: Vec<String>,
    stop: f64,
    resolution: f64,
    now: f64,
    started: bool,
    _dir: TempDir,
}

/// An error from a live session.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Launch(#[from] NgspiceError),

    #[error("io error while talking to ngspice")]
    Io(#[source] std::io::Error),

    #[error("ngspice exited ({}):\n{output}", exit_reason(*code))]
    Exited { code: Option<i32>, output: String },

    #[error("ngspice reported a problem with `{command}`:\n{output}")]
    Command { command: String, output: String },

    #[error("{value} is not a finite number")]
    NotFinite { value: f64 },

    #[error("time {requested} s is before the current time {now} s")]
    Backwards { requested: f64, now: f64 },

    #[error(
        "time {requested} s is less than {resolution} s after the current time {now} s"
    )]
    TooClose {
        requested: f64,
        now: f64,
        resolution: f64,
    },

    #[error("time {requested} s is after the stop time {stop} s")]
    PastStop { requested: f64, stop: f64 },

    #[error(
        "ngspice stopped at {landed} s, not at the requested {requested} s"
    )]
    Missed {
        requested: f64,
        landed: f64,
        output: String,
    },

    #[error("no drive holds node {node}")]
    UnknownDrive { node: String },

    #[error("node {node} is not in the circuit")]
    UnknownNode { node: String },

    #[error(
        "a drive cannot change at time zero; give its initial value to Drive::new"
    )]
    AtStart,
}

impl Ngspice {
    /// Start a live transient simulation of `netlist` with `drives`, solved
    /// up to time zero.
    ///
    /// The run ends at `stop` seconds. The solver takes no internal step
    /// longer than `max_step` seconds, and two times the session stops at
    /// must be at least a thousandth of `max_step` apart.
    ///
    /// # Errors
    ///
    /// Returns an error if ngspice does not start, or reports a problem
    /// with the netlist or the initial solution.
    pub fn session(
        &self,
        netlist: &str,
        drives: &[Drive],
        stop: f64,
        max_step: f64,
    ) -> Result<Session, SessionError> {
        for value in [stop, max_step] {
            finite(value)?;
        }
        let dir = tempfile::tempdir().map_err(SessionError::Io)?;
        let deck_path = dir.path().join("deck.cir");
        std::fs::write(&deck_path, deck(netlist, drives, stop, max_step))
            .map_err(SessionError::Io)?;

        let (reader, writer) = std::io::pipe().map_err(SessionError::Io)?;
        let mut child = {
            let mut command = Command::new(self.binary());
            command
                .arg("-p")
                .arg(&deck_path)
                .stdin(Stdio::piped())
                .stdout(writer.try_clone().map_err(SessionError::Io)?)
                .stderr(writer);
            command
                .spawn()
                .map_err(|source| spawn_error(self.binary(), source))?
        };
        let stdin = child.stdin.take().expect("stdin is piped");

        let mut session = Session {
            child,
            stdin,
            output: BufReader::new(reader),
            drives: drives.iter().map(|drive| key(&drive.node)).collect(),
            stop,
            resolution: RESOLUTION * max_step,
            now: 0.0,
            started: false,
            _dir: dir,
        };
        session.exchange(&[
            "set numdgt=17",
            "stop after 1",
            "run",
            "delete all",
        ])?;
        Ok(session)
    }
}

impl Session {
    /// The simulation time in seconds, as the solver landed on it.
    pub fn time(&self) -> f64 {
        self.now
    }

    /// Run the simulation forward to `time` seconds.
    ///
    /// A `time` equal to the current time does nothing.
    ///
    /// # Errors
    ///
    /// Returns an error if `time` is before the current time, too close
    /// after it, or after the stop time, or if the solver does not land on
    /// `time`.
    pub fn advance_to(&mut self, time: f64) -> Result<(), SessionError> {
        finite(time)?;
        if same_time(time, self.now) {
            return Ok(());
        }
        if time < self.now {
            return Err(SessionError::Backwards {
                requested: time,
                now: self.now,
            });
        }
        if time > self.stop && !same_time(time, self.stop) {
            return Err(SessionError::PastStop {
                requested: time,
                stop: self.stop,
            });
        }
        if time - self.now < self.resolution {
            return Err(SessionError::TooClose {
                requested: time,
                now: self.now,
                resolution: self.resolution,
            });
        }

        self.started = true;
        let stop = format!("stop when time = {time:e}");
        let lines = self.exchange(&[
            &stop,
            "resume",
            "delete all",
            "print time[length(time)-1]",
        ])?;
        let landed =
            value(&lines, "time[length(time)-1]").ok_or_else(|| {
                SessionError::Command {
                    command: stop.clone(),
                    output: lines.join("\n"),
                }
            })?;
        self.now = landed;
        if !same_time(landed, time) {
            return Err(SessionError::Missed {
                requested: time,
                landed,
                output: lines.join("\n"),
            });
        }
        Ok(())
    }

    /// Set the drive on `node` to `volts`, from just after the current time.
    ///
    /// # Errors
    ///
    /// Returns an error if no drive holds `node`, if the session is still
    /// at time zero, or if ngspice rejects the change.
    pub fn set(&mut self, node: &str, volts: f64) -> Result<(), SessionError> {
        finite(volts)?;
        let index = self
            .drives
            .iter()
            .position(|drive| *drive == key(node))
            .ok_or_else(|| SessionError::UnknownDrive {
                node: node.to_owned(),
            })?;
        if !self.started {
            return Err(SessionError::AtStart);
        }
        let alter =
            format!("alter {DRIVE_PREFIX}{n} dc = {volts:e}", n = index + 1);
        self.exchange(&[&alter])?;
        Ok(())
    }

    /// The voltage at `node`, a SPICE node or schematic net name, at the
    /// current time.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` is not in the circuit.
    pub fn node(&mut self, node: &str) -> Result<Voltage, SessionError> {
        let vector = format!("v({})", key(node));
        // Before the first advance each vector holds one point, which
        // ngspice treats as a scalar that cannot be indexed.
        let expression = if self.started {
            format!("{vector}[length(time)-1]")
        } else {
            vector
        };
        let lines = match self.exchange(&[&format!("print {expression}")]) {
            Err(SessionError::Command { output, .. })
                if output.contains("is not available") =>
            {
                return Err(SessionError::UnknownNode {
                    node: node.to_owned(),
                });
            }
            result => result?,
        };
        value(&lines, &expression).map(Voltage::new).ok_or_else(|| {
            SessionError::UnknownNode {
                node: node.to_owned(),
            }
        })
    }

    /// End the session and stop ngspice.
    ///
    /// # Errors
    ///
    /// Returns an error if ngspice does not exit cleanly.
    pub fn finish(mut self) -> Result<(), SessionError> {
        writeln!(self.stdin, "quit").map_err(SessionError::Io)?;
        self.stdin.flush().map_err(SessionError::Io)?;
        let mut output = String::new();
        self.output
            .read_to_string(&mut output)
            .map_err(SessionError::Io)?;
        let status = self.child.wait().map_err(SessionError::Io)?;
        if !status.success() {
            return Err(SessionError::Exited {
                code: status.code(),
                output,
            });
        }
        Ok(())
    }

    /// Send `commands`, then return ngspice's output up to the sync line.
    fn exchange(
        &mut self,
        commands: &[&str],
    ) -> Result<Vec<String>, SessionError> {
        for command in commands {
            writeln!(self.stdin, "{command}").map_err(SessionError::Io)?;
        }
        writeln!(self.stdin, "echo {SYNC}").map_err(SessionError::Io)?;
        self.stdin.flush().map_err(SessionError::Io)?;

        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            let read =
                self.output.read_line(&mut line).map_err(SessionError::Io)?;
            if read == 0 {
                let status = self.child.wait().map_err(SessionError::Io)?;
                return Err(SessionError::Exited {
                    code: status.code(),
                    output: lines.join("\n"),
                });
            }
            let line = line.trim_end();
            if line == SYNC {
                break;
            }
            lines.push(line.to_owned());
        }
        if lines.iter().any(|line| is_problem(line)) {
            return Err(SessionError::Command {
                command: commands.join("; "),
                output: lines.join("\n"),
            });
        }
        Ok(lines)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// The deck for a session: `netlist`, one source per drive, and the run.
fn deck(netlist: &str, drives: &[Drive], stop: f64, max_step: f64) -> String {
    let mut deck = netlist.to_owned();
    for (index, drive) in drives.iter().enumerate() {
        deck.push_str(&format!(
            "\n{DRIVE_PREFIX}{n} {node} 0 dc {volts:e}",
            n = index + 1,
            node = crate::kicad::node_name(&drive.node),
            volts = drive.volts,
        ));
    }
    deck.push_str(&format!(
        "\n.tran {max_step:e} {stop:e} 0 {max_step:e}\n.end\n"
    ));
    deck
}

/// The number ngspice printed for `expression`, from a `print` output line.
fn value(lines: &[String], expression: &str) -> Option<f64> {
    lines.iter().find_map(|line| {
        line.strip_prefix(expression)?
            .strip_prefix(" = ")?
            .trim()
            .parse()
            .ok()
    })
}

/// Whether an ngspice output line reports an error, a warning, or an abort.
fn is_problem(line: &str) -> bool {
    let line = line.trim_start().to_lowercase();
    line.starts_with("error")
        || line.starts_with("warning")
        || line.contains("aborted")
}

/// Whether two times are equal within [`TIME_TOLERANCE`].
fn same_time(a: f64, b: f64) -> bool {
    (a - b).abs() <= TIME_TOLERANCE * a.abs().max(b.abs())
}

/// Error unless `value` is finite.
fn finite(value: f64) -> Result<(), SessionError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(SessionError::NotFinite { value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_appends_drives_and_run() {
        assert_eq!(
            deck("* rc\nr1 in out 1k", &[Drive::new("in", 0.5)], 1e-3, 1e-6),
            "* rc\nr1 in out 1k\nvktdrive1 in 0 dc 5e-1\n.tran 1e-6 1e-3 0 1e-6\n.end\n"
        );
    }

    #[test]
    fn value_reads_the_printed_expression() {
        let lines = vec![
            "ngspice 12 -> print time[length(time)-1]".to_owned(),
            "time[length(time)-1] = 1.00000000000000002e-03".to_owned(),
        ];
        assert_eq!(value(&lines, "time[length(time)-1]"), Some(1e-3));
        assert_eq!(value(&lines, "v(out)"), None);
    }

    #[test]
    fn errors_and_warnings_are_problems() {
        assert!(is_problem("Error: indexing a scalar (time)"));
        assert!(is_problem(
            "Warning from checkvalid: vector x is not available"
        ));
        assert!(is_problem("run simulation(s) aborted"));
        assert!(!is_problem("doAnalyses: pause requested"));
    }

    #[test]
    fn same_time_is_relative() {
        assert!(same_time(1e-3, 1e-3 * (1.0 + 1e-13)));
        assert!(!same_time(1e-3, 1e-3 * (1.0 + 1e-11)));
        assert!(same_time(0.0, 0.0));
    }
}
