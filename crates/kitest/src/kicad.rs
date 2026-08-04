//! Exporting a KiCad schematic to a SPICE netlist via `kicad-cli`.

use std::path::Path;
use std::process::Command;

/// SPICE deck terminator; kitest works in bodies, so the exporter strips it.
const SPICE_END: &str = ".end";

/// Export the schematic at `sch` to a SPICE netlist body (no trailing `.end`).
pub fn export_netlist(sch: &Path) -> Result<String, KicadError> {
    let dir = tempfile::tempdir().map_err(KicadError::Io)?;
    let out_path = dir.path().join("netlist.cir");

    let output = Command::new("kicad-cli")
        .args(["sch", "export", "netlist", "--format", "spice"])
        .arg("-o")
        .arg(&out_path)
        .arg(sch)
        .output()
        .map_err(KicadError::Spawn)?;

    if !output.status.success() {
        return Err(KicadError::Exec {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    let netlist = std::fs::read_to_string(&out_path).map_err(KicadError::Io)?;
    Ok(strip_end(&netlist).to_owned())
}

#[derive(Debug, thiserror::Error)]
pub enum KicadError {
    #[error("could not launch kicad-cli")]
    Spawn(#[source] std::io::Error),

    #[error("io error while running kicad-cli")]
    Io(#[source] std::io::Error),

    #[error("kicad-cli exited with status {code:?}:\n{stderr}")]
    Exec { code: Option<i32>, stderr: String },
}

/// Drop a trailing `.end` line so the result is a netlist body.
fn strip_end(netlist: &str) -> &str {
    let trimmed = netlist.trim_end();
    match trimmed.len().checked_sub(SPICE_END.len()) {
        Some(cut)
            if trimmed
                .get(cut..)
                .is_some_and(|tail| tail.eq_ignore_ascii_case(SPICE_END)) =>
        {
            trimmed[..cut].trim_end()
        }
        _ => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_trailing_end() {
        assert_eq!(
            strip_end(".title x\nR1 a b 1k\n.end\n"),
            ".title x\nR1 a b 1k"
        );
    }

    #[test]
    fn strips_end_case_insensitively() {
        assert_eq!(strip_end("R1 a b 1k\n.END\n"), "R1 a b 1k");
    }

    #[test]
    fn tolerates_missing_end() {
        assert_eq!(strip_end("R1 a b 1k\n"), "R1 a b 1k");
    }
}
