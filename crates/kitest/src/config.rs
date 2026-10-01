//! Test conditions a project declares in `kitest.toml`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// File name kitest looks for beside a KiCad project.
pub const CONFIG_FILE: &str = "kitest.toml";

/// A project's declared test conditions.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Config {
    /// Voltages for each rail; more than one runs each as a separate corner.
    pub supplies: BTreeMap<String, Vec<f64>>,
    /// Project model libraries, resolved against the config file's directory.
    pub model_libraries: Vec<PathBuf>,
}

impl Config {
    /// Load `kitest.toml` from `directory`, or an empty config if it has none.
    pub fn for_project(directory: &Path) -> Result<Self, ConfigError> {
        let path = directory.join(CONFIG_FILE);
        if path.exists() {
            Self::load(&path)
        } else {
            Ok(Self::default())
        }
    }

    /// Load the config file at `path`.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| {
            ConfigError::Read {
                path: path.to_path_buf(),
                source,
            }
        })?;
        let directory = path.parent().unwrap_or(Path::new("."));
        Self::parse(&text, directory).map_err(|error| match error {
            ConfigError::Syntax { source, .. } => ConfigError::Syntax {
                path: path.to_path_buf(),
                source,
            },
            other => other,
        })
    }

    /// Parse config text, resolving relative paths against `directory`.
    pub fn parse(text: &str, directory: &Path) -> Result<Self, ConfigError> {
        let raw: Raw =
            toml::from_str(text).map_err(|source| ConfigError::Syntax {
                path: PathBuf::from(CONFIG_FILE),
                source,
            })?;

        let mut supplies = BTreeMap::new();
        for (rail, voltages) in raw.supplies {
            let voltages = match voltages {
                Voltages::One(volts) => vec![volts],
                Voltages::Many(list) => list,
            };
            if voltages.is_empty() {
                return Err(ConfigError::EmptySupply { rail });
            }
            if voltages.iter().any(|volts| !volts.is_finite()) {
                return Err(ConfigError::NonFiniteSupply { rail });
            }
            supplies.insert(rail, voltages);
        }

        Ok(Self {
            supplies,
            model_libraries: raw
                .models
                .libraries
                .into_iter()
                .map(|library| directory.join(library))
                .collect(),
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    #[serde(default)]
    supplies: BTreeMap<String, Voltages>,
    #[serde(default)]
    models: RawModels,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawModels {
    #[serde(default)]
    libraries: Vec<PathBuf>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Voltages {
    One(f64),
    Many(Vec<f64>),
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not a valid kitest config")]
    Syntax {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("supply {rail} lists no voltages")]
    EmptySupply { rail: String },

    #[error("supply {rail} has a voltage that is not a finite number")]
    NonFiniteSupply { rail: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config, ConfigError> {
        Config::parse(text, Path::new("/project"))
    }

    #[test]
    fn a_scalar_supply_is_one_corner_and_a_list_is_several() {
        let config = parse("[supplies]\nVCC = 9\nVBAT = [3.0, 3.7, 4.2]\n")
            .expect("parses");
        assert_eq!(config.supplies["VCC"], [9.0]);
        assert_eq!(config.supplies["VBAT"], [3.0, 3.7, 4.2]);
    }

    #[test]
    fn rail_names_keep_their_signs_and_case() {
        let config = parse("[supplies]\n\"+3V3\" = 3.3\n\"-15V\" = -15\n")
            .expect("parses");
        assert_eq!(config.supplies["+3V3"], [3.3]);
        assert_eq!(config.supplies["-15V"], [-15.0]);
    }

    #[test]
    fn model_libraries_resolve_against_the_config_directory() {
        let config = parse("[models]\nlibraries = [\"models/vendor.toml\"]\n")
            .expect("parses");
        assert_eq!(
            config.model_libraries,
            [PathBuf::from("/project/models/vendor.toml")]
        );
    }

    #[test]
    fn an_empty_file_declares_nothing() {
        assert_eq!(parse("").expect("parses"), Config::default());
    }

    #[test]
    fn rejects_unknown_sections_and_keys() {
        assert!(matches!(
            parse("[supply]\nVCC = 9\n"),
            Err(ConfigError::Syntax { .. })
        ));
        assert!(matches!(
            parse("[models]\nlibrary = []\n"),
            Err(ConfigError::Syntax { .. })
        ));
    }

    #[test]
    fn rejects_an_empty_or_non_numeric_supply() {
        assert!(matches!(
            parse("[supplies]\nVCC = []\n"),
            Err(ConfigError::EmptySupply { rail }) if rail == "VCC"
        ));
        assert!(matches!(
            parse("[supplies]\nVCC = \"9V\"\n"),
            Err(ConfigError::Syntax { .. })
        ));
        assert!(matches!(
            parse("[supplies]\nVCC = nan\n"),
            Err(ConfigError::NonFiniteSupply { rail }) if rail == "VCC"
        ));
    }

    #[test]
    fn a_project_without_a_config_declares_nothing() {
        let directory = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            Config::for_project(directory.path()).expect("loads"),
            Config::default()
        );
    }

    #[test]
    fn load_names_the_file_on_a_syntax_error() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join(CONFIG_FILE);
        std::fs::write(&path, "[supplies\n").expect("write");
        let error = Config::load(&path).unwrap_err();
        assert!(error.to_string().contains(CONFIG_FILE), "{error}");
    }
}
