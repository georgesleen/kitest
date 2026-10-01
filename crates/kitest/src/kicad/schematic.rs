//! Reading the schematic files for what the `kicadxml` export omits.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::KicadError;
use super::sexpr::{self, Sexp};

/// Power rails and per-part flags, gathered across every sheet.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct SheetFacts {
    pub(crate) rails: BTreeSet<String>,
    pub(crate) excluded_from_sim: BTreeSet<String>,
    pub(crate) dnp: BTreeSet<String>,
}

/// Read the root schematic at `root` and every sheet it includes.
pub(crate) fn read(root: &Path) -> Result<SheetFacts, KicadError> {
    let mut facts = SheetFacts::default();
    let mut pending = vec![root.to_path_buf()];
    let mut visited = BTreeSet::new();

    while let Some(path) = pending.pop() {
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !visited.insert(key) {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|source| {
            KicadError::ReadSchematic {
                path: path.clone(),
                source,
            }
        })?;
        let sheet =
            sexpr::parse(&text).map_err(|source| KicadError::Schematic {
                path: path.clone(),
                source,
            })?;
        let directory = path.parent().unwrap_or(Path::new("."));
        pending.extend(sub_sheets(&sheet, directory));
        gather(&sheet, &mut facts);
    }
    Ok(facts)
}

/// Record rails and flags from one parsed sheet.
fn gather(sheet: &Sexp, facts: &mut SheetFacts) {
    let power: BTreeSet<&str> = sheet
        .child("lib_symbols")
        .into_iter()
        .flat_map(|symbols| symbols.children("symbol"))
        .filter(|symbol| symbol.child("power").is_some())
        .filter_map(|symbol| symbol.arg(1))
        .collect();

    for symbol in sheet.children("symbol") {
        let Some(library) = symbol.child("lib_id").and_then(|id| id.arg(1))
        else {
            continue;
        };
        if power.contains(library) {
            if let Some(rail) = property(symbol, "Value") {
                facts.rails.insert(rail.to_owned());
            }
            continue;
        }
        let references = references(symbol);
        if flag(symbol, "exclude_from_sim") {
            facts.excluded_from_sim.extend(references.iter().cloned());
        }
        if flag(symbol, "dnp") {
            facts.dnp.extend(references);
        }
    }
}

/// Every reference a placed symbol has, one per sheet instance.
fn references(symbol: &Sexp) -> Vec<String> {
    let from_instances: Vec<String> = symbol
        .child("instances")
        .into_iter()
        .flat_map(|instances| instances.children("project"))
        .flat_map(|project| project.children("path"))
        .filter_map(|path| path.child("reference").and_then(|r| r.arg(1)))
        .map(str::to_owned)
        .collect();
    if from_instances.is_empty() {
        property(symbol, "Reference")
            .map(str::to_owned)
            .into_iter()
            .collect()
    } else {
        from_instances
    }
}

/// Paths of the sheets placed on `sheet`, resolved against `directory`.
fn sub_sheets(sheet: &Sexp, directory: &Path) -> Vec<PathBuf> {
    sheet
        .children("sheet")
        .filter_map(|placed| {
            property(placed, "Sheetfile")
                .or_else(|| property(placed, "Sheet file"))
        })
        .map(|file| directory.join(file))
        .collect()
}

/// The value of the property named `name`.
fn property<'a>(node: &'a Sexp, name: &str) -> Option<&'a str> {
    node.children("property")
        .find(|property| property.arg(1) == Some(name))
        .and_then(|property| property.arg(2))
}

/// True if the flag list `name` reads `yes`.
fn flag(node: &Sexp, name: &str) -> bool {
    node.child(name).and_then(|flag| flag.arg(1)) == Some("yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = r##"(kicad_sch
        (lib_symbols
            (symbol "power:VCC" (power global))
            (symbol "Device:R_US"))
        (symbol (lib_id "power:VCC")
            (property "Reference" "#PWR01")
            (property "Value" "VCC"))
        (symbol (lib_id "Device:R_US")
            (exclude_from_sim yes) (dnp no)
            (property "Reference" "R1")
            (instances (project "p" (path "/a" (reference "R1")))))
        (symbol (lib_id "Device:R_US")
            (exclude_from_sim no) (dnp yes)
            (property "Reference" "R2"))
        (sheet (property "Sheetname" "sub") (property "Sheetfile" "sub.kicad_sch")))"##;

    const SUB: &str = r#"(kicad_sch
        (lib_symbols (symbol "power:+3V3" (power)))
        (symbol (lib_id "power:+3V3") (property "Value" "+3V3"))
        (sheet (property "Sheetfile" "root.kicad_sch")))"#;

    fn project() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(directory.path().join("root.kicad_sch"), ROOT)
            .expect("write root");
        std::fs::write(directory.path().join("sub.kicad_sch"), SUB)
            .expect("write sub");
        directory
    }

    #[test]
    fn collects_rails_from_every_sheet() {
        let directory = project();
        let facts =
            read(&directory.path().join("root.kicad_sch")).expect("reads");
        let rails: Vec<_> = facts.rails.iter().map(String::as_str).collect();
        assert_eq!(rails, ["+3V3", "VCC"]);
    }

    #[test]
    fn records_sim_exclusion_and_dnp_by_reference() {
        let directory = project();
        let facts =
            read(&directory.path().join("root.kicad_sch")).expect("reads");
        assert_eq!(facts.excluded_from_sim, BTreeSet::from(["R1".to_owned()]));
        assert_eq!(facts.dnp, BTreeSet::from(["R2".to_owned()]));
    }

    #[test]
    fn names_the_file_that_fails_to_parse() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("bad.kicad_sch");
        std::fs::write(&path, "(kicad_sch (symbol").expect("write");
        let error = read(&path).unwrap_err();
        assert!(error.to_string().contains("bad.kicad_sch"), "{error}");
    }
}
