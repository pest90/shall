//! Project configuration from `shall.toml`.
//!
//! Only the parts used so far are read; unknown tables and keys are
//! ignored, so a configuration written for a later version still loads.

use serde::Deserialize;
use thiserror::Error;

use crate::id::ItemType;

/// File name of the project configuration.
pub const FILE_NAME: &str = "shall.toml";

/// Contents of `shall.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Config {
    /// Levels of the project, from the `[[level]]` tables.
    #[serde(default, rename = "level")]
    pub levels: Vec<Level>,
    /// Verification types, from the `[verification]` table.
    #[serde(default)]
    pub verification: Verification,
}

/// Types of items that verify others, e.g. tests and reviews.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Verification {
    /// Verification types such as `utest`, `itest` or `vrev`.
    #[serde(default)]
    pub types: Vec<ItemType>,
}

/// A level such as system or software requirements.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Level {
    /// Type of the level's items, e.g. `swr`.
    #[serde(rename = "type")]
    pub typ: ItemType,
    /// Readable name, e.g. `L3 – Software Requirements`.
    pub name: String,
    /// Types that must cover an item of this level, unless the item
    /// states its own `Needs:`.
    #[serde(default)]
    pub needs: Vec<ItemType>,
}

impl Config {
    /// Parses the contents of `shall.toml`.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|error| ConfigError(error.to_string()))
    }

    /// Returns the level of the given type.
    pub fn level(&self, typ: &ItemType) -> Option<&Level> {
        self.levels.iter().find(|level| &level.typ == typ)
    }

    /// Returns whether items of the given type cover items of another
    /// level, i.e. whether some level needs this type.
    pub fn has_parents(&self, typ: &ItemType) -> bool {
        self.levels.iter().any(|level| level.needs.contains(typ))
    }
}

/// Error for an invalid `shall.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid {FILE_NAME}: {0}")]
pub struct ConfigError(String);

#[cfg(test)]
mod tests {
    use super::*;

    fn typ(s: &str) -> ItemType {
        s.parse().unwrap()
    }

    #[test]
    fn reads_levels_and_ignores_other_tables() {
        let config = Config::parse(
            r#"
[[level]]
type  = "swr"
name  = "Software"
needs = ["dsn"]

[[level]]
type = "dsn"
name = "Design"
needs = ["impl", "utest"]

[verification]
types = ["utest", "vrev"]

[sources]
impl = ["src/**"]
"#,
        )
        .unwrap();
        assert_eq!(config.levels.len(), 2);
        assert_eq!(config.verification.types, [typ("utest"), typ("vrev")]);
        assert_eq!(config.level(&typ("dsn")).unwrap().name, "Design");
        assert!(config.level(&typ("impl")).is_none());
        assert!(config.has_parents(&typ("dsn")));
        assert!(!config.has_parents(&typ("swr")));
    }

    #[test]
    fn rejects_invalid_types() {
        let error = Config::parse("[[level]]\ntype = \"l3\"\nname = \"x\"\n").unwrap_err();
        assert!(error.to_string().contains("invalid type 'l3'"), "{error}");
    }

    #[test]
    fn empty_file_has_no_levels() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }
}
