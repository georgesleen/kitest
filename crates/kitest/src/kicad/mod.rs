//! Reading KiCad schematics through `kicad-cli` and the schematic files.

mod design;
mod element;
mod netlist;
mod node;
mod pins;
mod rail;
mod schematic;
mod sexpr;
mod supplies;
mod value;
mod xml;

use std::path::{Path, PathBuf};
use std::process::Command;

pub use design::{
    Component, Design, LibraryId, LibraryPart, LibraryPin, Net, Node, PinKind,
};
pub use netlist::{Netlist, NetlistError};
use sexpr::SexpError;
pub use supplies::{
    Corner, Power, Rail, RailKind, SupplyError, SupplyProblem, VoltageOrigin,
};

/// SPICE deck terminator; kitest works in bodies, so the exporter strips it.
const SPICE_END: &str = ".end";

/// Export the schematic at `sch` to a SPICE netlist body (no trailing `.end`).
pub fn export_netlist(sch: &Path) -> Result<String, KicadError> {
    let netlist = run_export(sch, "spice")?;
    Ok(strip_end(&netlist).to_owned())
}

/// Export the schematic at `sch` as kitest reads it.
pub fn export_design(sch: &Path) -> Result<Design, KicadError> {
    let xml::Export {
        mut components,
        parts,
        nets,
    } = xml::parse(&run_export(sch, "kicadxml")?)?;
    let facts = schematic::read(sch)?;

    for component in &mut components {
        component.excluded_from_sim =
            facts.excluded_from_sim.contains(&component.reference);
        component.dnp = facts.dnp.contains(&component.reference);
    }

    Ok(Design {
        components,
        parts,
        nets,
        rails: facts.rails.into_iter().collect(),
    })
}

/// Run `kicad-cli sch export netlist` on `sch` in `format` and return the file.
fn run_export(sch: &Path, format: &str) -> Result<String, KicadError> {
    let dir = tempfile::tempdir().map_err(KicadError::Io)?;
    let out_path = dir.path().join("netlist");

    let output = Command::new("kicad-cli")
        .args(["sch", "export", "netlist", "--format", format])
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

    std::fs::read_to_string(&out_path).map_err(KicadError::Io)
}

#[derive(Debug, thiserror::Error)]
pub enum KicadError {
    #[error("could not launch kicad-cli")]
    Spawn(#[source] std::io::Error),

    #[error("io error while running kicad-cli")]
    Io(#[source] std::io::Error),

    #[error("kicad-cli exited with status {code:?}:\n{stderr}")]
    Exec { code: Option<i32>, stderr: String },

    #[error("kicadxml export is not valid XML")]
    Xml(#[source] roxmltree::Error),

    #[error("kicadxml export is malformed: {0}")]
    MalformedExport(String),

    #[error("could not read schematic {path}")]
    ReadSchematic {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("schematic {path} is malformed")]
    Schematic {
        path: PathBuf,
        #[source]
        source: SexpError,
    },
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
