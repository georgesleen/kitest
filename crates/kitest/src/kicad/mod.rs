//! Reading KiCad schematics through `kicad-cli` and the schematic files.

mod design;
mod element;
mod netlist;
mod node;
mod pins;
mod probe;
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
pub use netlist::{Netlist, NetlistError, Transistor};
pub(crate) use node::node_name;
pub use probe::{Probe, ProbeError, ProbeProblem};
pub(crate) use rail::{is_ground, is_spice_ground};
use sexpr::SexpError;
pub(crate) use value::{Unit, parse as parse_value};

use crate::sim::exit_reason;
pub use supplies::{
    Corner, Power, Rail, RailKind, SupplyError, SupplyProblem, VoltageOrigin,
};

/// SPICE deck terminator; kitest works in bodies, so the exporter strips it.
const SPICE_END: &str = ".end";

/// The KiCad command-line program kitest drives.
const KICAD_CLI: &str = "kicad-cli";

/// Export the schematic at `sch` to a SPICE netlist body (no trailing `.end`).
pub fn export_netlist(sch: &Path) -> Result<String, KicadError> {
    let netlist = run_export(sch, "spice")?;
    Ok(strip_end(&netlist).to_owned())
}

/// Export the schematic at `sch` as kitest reads it.
pub fn export_design(sch: &Path) -> Result<Design, KicadError> {
    let xml::Export {
        mut components,
        mut parts,
        nets,
    } = xml::parse(&run_export(sch, "kicadxml")?)?;
    let facts = schematic::read(sch)?;
    let project = sch.parent().unwrap_or(Path::new("."));

    for component in &mut components {
        component.excluded_from_sim =
            facts.excluded_from_sim.contains(&component.reference);
        component.dnp = facts.dnp.contains(&component.reference);
        resolve_model_file(&mut component.fields, project);
    }
    for part in parts.values_mut() {
        resolve_model_file(&mut part.fields, project);
    }
    resolve_through_kicad(sch, &mut components, &mut parts);

    Ok(Design {
        components,
        parts,
        nets,
        rails: facts.rails.into_iter().collect(),
    })
}

/// Rewrite a `Sim.Library` field as the absolute path KiCad would open:
/// `${VAR}` expanded, with `KIPRJMOD` as the project directory, and a
/// relative path taken from the project directory.
fn resolve_model_file(
    fields: &mut std::collections::BTreeMap<String, String>,
    project: &Path,
) {
    let Some(path) = fields
        .get_mut(element::SIM_LIBRARY)
        .filter(|path| !path.trim().is_empty())
    else {
        return;
    };
    let mut expanded = String::new();
    let mut rest = path.trim();
    while let Some(start) = rest.find("${") {
        expanded.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            break;
        };
        let name = &after[..end];
        let value = if name == "KIPRJMOD" {
            Some(project.display().to_string())
        } else {
            std::env::var(name).ok()
        };
        match value {
            Some(value) => expanded.push_str(&value),
            None => expanded.push_str(&rest[start..start + 3 + end]),
        }
        rest = &after[end + 1..];
    }
    expanded.push_str(rest);
    let resolved = project.join(&expanded);
    *path = resolved.display().to_string();
}

/// Resolve `Sim.Library` paths left holding a `${VAR}` KiCad sets only in
/// its own process, such as `${KICAD9_SYMBOL_DIR}` on KiCad's stock
/// simulation symbols, from the `.include` lines of KiCad's own SPICE
/// export. A path keeps its variable if KiCad cannot export or names no
/// single file ending as it does.
fn resolve_through_kicad(
    sch: &Path,
    components: &mut [Component],
    parts: &mut std::collections::BTreeMap<LibraryId, LibraryPart>,
) {
    let unresolved = |fields: &std::collections::BTreeMap<String, String>| {
        fields
            .get(element::SIM_LIBRARY)
            .is_some_and(|path| path.contains("${"))
    };
    let any = components
        .iter()
        .any(|component| unresolved(&component.fields))
        || parts.values().any(|part| unresolved(&part.fields));
    if !any {
        return;
    }
    // KiCad exits 2 when any part's model looks inconsistent to it, yet still
    // writes the netlist with every include resolved, so read it regardless.
    let Ok((_, Some(spice))) = export(sch, "spice") else {
        return;
    };
    let includes: Vec<&str> = spice
        .lines()
        .filter_map(|line| line.trim().strip_prefix(".include "))
        .map(|path| path.trim().trim_matches('"'))
        .collect();
    let fields = components
        .iter_mut()
        .map(|component| &mut component.fields)
        .chain(parts.values_mut().map(|part| &mut part.fields));
    for fields in fields {
        let Some(path) = fields.get_mut(element::SIM_LIBRARY) else {
            continue;
        };
        let Some(close) = path.rfind('}').filter(|_| path.contains("${"))
        else {
            continue;
        };
        let tail = &path[close + 1..];
        let mut matching = includes
            .iter()
            .filter(|include| !tail.is_empty() && include.ends_with(tail));
        if let (Some(include), None) = (matching.next(), matching.next()) {
            *path = (*include).to_owned();
        }
    }
}

/// Run `kicad-cli sch export netlist` on `sch` in `format` and return the file.
fn run_export(sch: &Path, format: &str) -> Result<String, KicadError> {
    let (output, file) = export(sch, format)?;
    if !output.status.success() {
        return Err(KicadError::Exec {
            path: sch.to_path_buf(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    file.ok_or_else(|| {
        KicadError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "kicad-cli reported success but wrote no netlist",
        ))
    })
}

/// Run `kicad-cli sch export netlist` on `sch` in `format`: its process
/// output, and the file it wrote, if it wrote one, whatever its exit status.
fn export(
    sch: &Path,
    format: &str,
) -> Result<(std::process::Output, Option<String>), KicadError> {
    if matches!(sch.try_exists(), Ok(false)) {
        return Err(KicadError::MissingSchematic {
            path: sch.to_path_buf(),
        });
    }

    let dir = tempfile::tempdir().map_err(KicadError::Io)?;
    let out_path = dir.path().join("netlist");

    let output = Command::new(KICAD_CLI)
        .args(["sch", "export", "netlist", "--format", format])
        .arg("-o")
        .arg(&out_path)
        .arg(sch)
        .output()
        .map_err(spawn_error)?;
    Ok((output, std::fs::read_to_string(&out_path).ok()))
}

/// Classify a failure to launch kicad-cli.
fn spawn_error(err: std::io::Error) -> KicadError {
    if err.kind() == std::io::ErrorKind::NotFound {
        KicadError::NotInstalled(err)
    } else {
        KicadError::Spawn(err)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KicadError {
    #[error("schematic {} does not exist", path.display())]
    MissingSchematic { path: PathBuf },

    #[error(
        "kicad-cli was not found on PATH; install KiCad 9 or later, or enter the nix dev shell"
    )]
    NotInstalled(#[source] std::io::Error),

    #[error("could not launch kicad-cli")]
    Spawn(#[source] std::io::Error),

    #[error("io error while running kicad-cli")]
    Io(#[source] std::io::Error),

    #[error(
        "kicad-cli failed ({}) on {}:\n{stderr}",
        exit_reason(*code),
        path.display()
    )]
    Exec {
        path: PathBuf,
        code: Option<i32>,
        stderr: String,
    },

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

    #[test]
    fn missing_schematic_is_reported_before_running_kicad_cli() {
        let dir = tempfile::tempdir().unwrap();
        let sch = dir.path().join("missing.kicad_sch");

        for err in [
            export_netlist(&sch).expect_err("schematic is missing"),
            export_design(&sch).expect_err("schematic is missing"),
        ] {
            assert!(
                matches!(&err, KicadError::MissingSchematic { path } if *path == sch),
                "{err:?}"
            );
            assert!(err.to_string().contains(&sch.display().to_string()));
        }
    }

    #[test]
    fn absent_kicad_cli_points_at_path() {
        let err = spawn_error(std::io::ErrorKind::NotFound.into());
        let message = err.to_string();
        assert!(matches!(err, KicadError::NotInstalled(_)), "{err:?}");
        assert!(message.contains("kicad-cli") && message.contains("PATH"));
    }

    #[test]
    fn other_launch_failures_stay_generic() {
        let err = spawn_error(std::io::ErrorKind::PermissionDenied.into());
        assert!(matches!(err, KicadError::Spawn(_)), "{err:?}");
    }

    #[test]
    fn failed_export_names_the_schematic_and_exit_status() {
        let err = KicadError::Exec {
            path: PathBuf::from("/tmp/board.kicad_sch"),
            code: Some(3),
            stderr: "boom".into(),
        };
        let message = err.to_string();
        assert!(message.contains("/tmp/board.kicad_sch"), "{message}");
        assert!(message.contains("exit status 3"), "{message}");
        assert!(!message.contains("Some("), "{message}");
    }
}
