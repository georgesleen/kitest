//! SPICE model libraries, matched to a part's Value.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The library kitest ships, in the same format as a project library.
const BUNDLED: &str = include_str!("../models/bundled.toml");

/// A set of model cards, each covering one or more part Values.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelLibrary {
    entries: Vec<ModelEntry>,
}

/// One model card and the Values it covers.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelEntry {
    pub values: Vec<String>,
    /// Where the card came from, such as a manufacturer and date.
    pub source: Option<String>,
    pub kind: ModelKind,
    /// The card text, ready to append to a deck.
    pub card: String,
}

/// What a card defines, which decides how a part's pins are wired to it.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelKind {
    /// A `.model` card; pins follow the device's roles.
    Model {
        name: String,
        /// The device type as written, lowercased, such as `npn` or `d`.
        device: String,
    },
    /// A `.subckt`; `ports` gives the pin role on each port, in port order.
    Subckt { name: String, ports: Vec<String> },
}

impl ModelKind {
    /// The model or subcircuit name the element line refers to.
    pub fn name(&self) -> &str {
        match self {
            Self::Model { name, .. } | Self::Subckt { name, .. } => name,
        }
    }
}

impl ModelLibrary {
    /// The library kitest ships.
    ///
    /// # Panics
    /// If the bundled library is malformed, which its own test rules out.
    pub fn bundled() -> Self {
        Self::parse(BUNDLED).expect("bundled model library is valid")
    }

    /// Load a library file.
    pub fn load(path: &Path) -> Result<Self, ModelError> {
        let text = std::fs::read_to_string(path).map_err(|source| {
            ModelError::Read {
                path: path.to_path_buf(),
                source,
            }
        })?;
        Self::parse(&text).map_err(|source| ModelError::InFile {
            path: path.to_path_buf(),
            source: Box::new(source),
        })
    }

    /// Parse library text.
    pub fn parse(text: &str) -> Result<Self, ModelError> {
        let raw: RawLibrary =
            toml::from_str(text).map_err(ModelError::Syntax)?;
        let mut entries: Vec<ModelEntry> = Vec::new();
        for raw_entry in raw.model {
            let entry = entry(raw_entry)?;
            for value in &entry.values {
                if entries.iter().any(|earlier| earlier.covers(value)) {
                    return Err(ModelError::Duplicate {
                        value: value.clone(),
                    });
                }
            }
            entries.push(entry);
        }
        Ok(Self { entries })
    }

    /// The entry covering the part Value `value`, ignoring ASCII case.
    pub fn find(&self, value: &str) -> Option<&ModelEntry> {
        self.entries.iter().find(|entry| entry.covers(value))
    }
}

impl ModelEntry {
    fn covers(&self, value: &str) -> bool {
        self.values
            .iter()
            .any(|covered| covered.eq_ignore_ascii_case(value))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLibrary {
    #[serde(default)]
    model: Vec<RawEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    values: Vec<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    ports: Option<Vec<String>>,
    card: String,
}

fn entry(raw: RawEntry) -> Result<ModelEntry, ModelError> {
    let label = raw.values.join(", ");
    if raw.values.is_empty() {
        return Err(ModelError::Card {
            values: label,
            message: "covers no values".into(),
        });
    }
    let tokens =
        first_directive(&raw.card).ok_or_else(|| ModelError::Card {
            values: label.clone(),
            message: "holds no .model or .subckt line".into(),
        })?;
    let directive = tokens[0].to_ascii_lowercase();
    let name = tokens.get(1).cloned().ok_or_else(|| ModelError::Card {
        values: label.clone(),
        message: format!("{directive} has no name"),
    })?;

    let kind = match (directive.as_str(), raw.ports) {
        (".model", None) => ModelKind::Model {
            device: tokens
                .get(2)
                .map(|device| device.to_ascii_lowercase())
                .ok_or_else(|| ModelError::Card {
                    values: label.clone(),
                    message: format!(".model {name} has no device type"),
                })?,
            name,
        },
        (".model", Some(_)) => {
            return Err(ModelError::Card {
                values: label,
                message:
                    "a .model card takes its pins from the device, not ports"
                        .into(),
            });
        }
        (".subckt", Some(ports)) => {
            let declared = tokens.len() - 2;
            if ports.len() != declared {
                return Err(ModelError::Card {
                    values: label,
                    message: format!(
                        ".subckt {name} has {declared} ports but {} roles are listed",
                        ports.len()
                    ),
                });
            }
            ModelKind::Subckt { name, ports }
        }
        (".subckt", None) => {
            return Err(ModelError::Card {
                values: label,
                message: format!(
                    ".subckt {name} needs ports listing each pin role"
                ),
            });
        }
        _ => unreachable!("first_directive only returns .model or .subckt"),
    };

    Ok(ModelEntry {
        values: raw.values,
        source: raw.source,
        kind,
        card: raw.card.trim().to_owned(),
    })
}

/// The tokens of the first `.model` or `.subckt` statement, continuations
/// joined, with a `.model` device type split from its parameter list.
fn first_directive(card: &str) -> Option<Vec<String>> {
    directives(card).into_iter().next()
}

/// The tokens of every `.model` and `.subckt` statement in `text`, as
/// `first_directive` splits them.
fn directives(text: &str) -> Vec<Vec<String>> {
    let mut found = Vec::new();
    let mut statement: Option<String> = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('*') {
            continue;
        }
        if let (Some(open), Some(rest)) =
            (&mut statement, line.strip_prefix('+'))
        {
            open.push(' ');
            open.push_str(rest);
            continue;
        }
        found.extend(statement.take().map(|open| tokens(&open)));
        let lower = line.to_ascii_lowercase();
        if lower.starts_with(".model") || lower.starts_with(".subckt") {
            statement = Some(line.to_owned());
        }
    }
    found.extend(statement.map(|open| tokens(&open)));
    found
}

fn tokens(statement: &str) -> Vec<String> {
    let statement = statement.split(';').next().unwrap_or(statement);
    let head = statement.split('(').next().unwrap_or(statement);
    let mut tokens: Vec<String> =
        head.split_whitespace().map(str::to_owned).collect();
    if tokens[0].eq_ignore_ascii_case(".subckt") {
        tokens.retain(|token| !token.contains('='));
        if let Some(position) = tokens
            .iter()
            .position(|token| token.eq_ignore_ascii_case("params:"))
        {
            tokens.truncate(position);
        }
    }
    tokens
}

/// Every model and subcircuit a SPICE file defines, by name. A subcircuit's
/// ports are its port names, in port order.
pub(crate) fn definitions(text: &str) -> Vec<ModelKind> {
    directives(text)
        .into_iter()
        .filter_map(|tokens| {
            let name = tokens.get(1)?.clone();
            if tokens[0].eq_ignore_ascii_case(".model") {
                let device = tokens.get(2)?.to_ascii_lowercase();
                Some(ModelKind::Model { name, device })
            } else {
                Some(ModelKind::Subckt {
                    name,
                    ports: tokens[2..].to_vec(),
                })
            }
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("could not read model library {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("in model library {path}")]
    InFile {
        path: PathBuf,
        #[source]
        source: Box<ModelError>,
    },

    #[error("model library is not valid TOML")]
    Syntax(#[source] toml::de::Error),

    #[error("model for {values}: {message}")]
    Card { values: String, message: String },

    #[error("more than one model covers {value}")]
    Duplicate { value: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_library_covers_the_2n3904_as_an_npn_model() {
        let library = ModelLibrary::bundled();
        let entry = library.find("2n3904").expect("2N3904 covered");
        assert_eq!(
            entry.kind,
            ModelKind::Model {
                name: "2N3904".into(),
                device: "npn".into(),
            }
        );
        assert!(entry.card.starts_with(".model 2N3904"));
    }

    #[test]
    fn an_unknown_value_has_no_model() {
        assert!(ModelLibrary::bundled().find("BC547").is_none());
    }

    #[test]
    fn one_entry_can_cover_several_values() {
        let library = ModelLibrary::parse(
            r#"
            [[model]]
            values = ["2N3904", "MMBT3904"]
            card = ".model Q3904 npn(bf=300)"
            "#,
        )
        .expect("parses");
        assert!(library.find("MMBT3904").is_some());
    }

    #[test]
    fn a_subckt_records_its_port_roles_in_order() {
        let library = ModelLibrary::parse(
            r#"
            [[model]]
            values = ["TL072"]
            ports = ["+", "-", "V+", "V-", "OUT"]
            card = """
            * vendor header
            .SUBCKT TL072 1 2 3 4 5 PARAMS: GAIN=1
            R1 1 2 1MEG
            .ENDS
            """
            "#,
        )
        .expect("parses");
        let entry = library.find("tl072").expect("covered");
        assert_eq!(
            entry.kind,
            ModelKind::Subckt {
                name: "TL072".into(),
                ports: vec![
                    "+".into(),
                    "-".into(),
                    "V+".into(),
                    "V-".into(),
                    "OUT".into()
                ],
            }
        );
    }

    #[test]
    fn a_model_card_split_across_continuation_lines_reads_its_type() {
        let library = ModelLibrary::parse(
            r#"
            [[model]]
            values = ["1N4148"]
            card = """
            .model 1N4148
            + D(is=2.52n rs=.568)
            """
            "#,
        )
        .expect("parses");
        assert_eq!(
            library.find("1N4148").expect("covered").kind,
            ModelKind::Model {
                name: "1N4148".into(),
                device: "d".into(),
            }
        );
    }

    #[test]
    fn rejects_a_subckt_whose_roles_do_not_match_its_ports() {
        let error = ModelLibrary::parse(
            r#"
            [[model]]
            values = ["X"]
            ports = ["A", "K"]
            card = ".subckt X 1 2 3"
            "#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("3 ports"), "{error}");
    }

    #[test]
    fn rejects_a_subckt_without_roles_and_a_model_with_them() {
        assert!(
            ModelLibrary::parse(
                "[[model]]\nvalues = [\"X\"]\ncard = \".subckt X 1 2\"\n"
            )
            .is_err()
        );
        assert!(
            ModelLibrary::parse(
                "[[model]]\nvalues = [\"X\"]\nports = [\"C\"]\ncard = \".model X npn\"\n"
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_two_entries_covering_one_value() {
        let error = ModelLibrary::parse(
            r#"
            [[model]]
            values = ["2N3904"]
            card = ".model A npn"
            [[model]]
            values = ["2n3904"]
            card = ".model B npn"
            "#,
        )
        .unwrap_err();
        assert!(matches!(error, ModelError::Duplicate { .. }), "{error}");
    }

    #[test]
    fn rejects_a_card_with_no_directive() {
        let error = ModelLibrary::parse(
            "[[model]]\nvalues = [\"X\"]\ncard = \"* only a comment\"\n",
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("no .model or .subckt"),
            "{error}"
        );
    }
}
