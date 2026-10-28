//! Project configuration from `shall.toml`.
//!
//! Only the parts used so far are read; unknown tables and keys are
//! ignored, so a configuration written for a later version still loads.

use std::collections::BTreeMap;

use serde::Deserialize;
use thiserror::Error;

use crate::diagnostic::Severity;
use crate::id::ItemType;
use crate::markdown::is_feature;

/// File name of the project configuration.
pub const FILE_NAME: &str = "shall.toml";

/// Contents of `shall.toml`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Config {
    /// Levels of the project, from the `[[level]]` tables.
    #[serde(default, rename = "level")]
    pub levels: Vec<Level>,
    /// Verification types, from the `[verification]` table.
    #[serde(default)]
    pub verification: Verification,
    /// Files to read, from the `[sources]` table.
    #[serde(default)]
    pub sources: Sources,
    /// Allowed targets of `Satisfied-by:`, from the `[satisfied_by]` table.
    #[serde(default)]
    pub satisfied_by: SatisfiedBy,
    /// Variants by name, from the `[variant.<name>]` tables.
    #[serde(default, rename = "variant")]
    pub variants: BTreeMap<String, Variant>,
}

/// A variant of the product: a selection of features, see
/// [`crate::variant`].
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Variant {
    /// Selected features, e.g. `["motor_ctrl", "can"]`.
    #[serde(default)]
    pub features: Vec<String>,
    /// JUnit XML files with the test results of this variant's build;
    /// when not given, those of `[sources]`. An empty list means no test
    /// results.
    #[serde(default)]
    pub junit: Option<Vec<String>>,
    /// All other keys, see [`Variant::unknown_keys`].
    #[serde(flatten)]
    other: BTreeMap<String, toml::Value>,
}

impl Variant {
    /// Creates a variant that selects `features`, with the test results
    /// of `[sources]`.
    pub fn new(features: Vec<String>) -> Self {
        Self {
            features,
            ..Self::default()
        }
    }

    /// Keys of the table other than `features` and `junit`; a later
    /// version may give them a meaning, but they are often typos such as
    /// `feature`.
    pub fn unknown_keys(&self) -> Vec<&str> {
        self.other.keys().map(String::as_str).collect()
    }
}

/// Types that items may name in `Satisfied-by:`.
///
/// Reused items are meant to be linked through a stable level, e.g. the
/// capabilities of a platform, so that the platform can rework the levels
/// below without affecting projects.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SatisfiedBy {
    /// Allowed types; `["cap"]` when not given.
    #[serde(default = "default_targets")]
    pub targets: Vec<ItemType>,
    /// Types that are allowed too, but get a warning, e.g. software
    /// requirements of a platform for a need no capability meets yet.
    #[serde(default)]
    pub fallback: Vec<ItemType>,
}

impl Default for SatisfiedBy {
    fn default() -> Self {
        Self {
            targets: default_targets(),
            fallback: Vec::new(),
        }
    }
}

fn default_targets() -> Vec<ItemType> {
    vec!["cap".parse().expect("'cap' is a valid type")]
}

/// Glob patterns of the files to read, relative to the project root,
/// e.g. `src/**/*.{cpp,hpp}`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Sources {
    /// Requirement files; when not given, all Markdown files of the project.
    #[serde(default)]
    pub requirements: Option<Vec<String>>,
    /// JUnit XML files with test results.
    #[serde(default)]
    pub junit: Vec<String>,
    /// All other keys, see [`Sources::code`].
    #[serde(flatten)]
    other: BTreeMap<String, toml::Value>,
}

impl Sources {
    /// Patterns of code and test files to scan for tags, under any other
    /// key such as `impl` or `utest`. The key only groups the patterns; the
    /// type of an item comes from its tag. A file matched under several
    /// keys is one file. Keys whose value is not a list of strings are left
    /// out, see [`Sources::invalid_keys`].
    pub fn code(&self) -> BTreeMap<&str, Vec<&str>> {
        self.other
            .iter()
            .filter_map(|(key, value)| Some((key.as_str(), string_list(value)?)))
            .collect()
    }

    /// Keys of `[sources]` whose value is not a list of strings; a later
    /// version may give them a meaning.
    pub fn invalid_keys(&self) -> Vec<&str> {
        self.other
            .iter()
            .filter(|(_, value)| string_list(value).is_none())
            .map(|(key, _)| key.as_str())
            .collect()
    }
}

fn string_list(value: &toml::Value) -> Option<Vec<&str>> {
    value.as_array()?.iter().map(toml::Value::as_str).collect()
}

/// Types of items that verify others, e.g. tests and reviews.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Verification {
    /// Verification types such as `utest`, `itest` or `vrev`. Tags of
    /// these types are matched with test results.
    #[serde(default)]
    pub types: Vec<ItemType>,
    /// Severity of a test without result, `error` (default) or `warning`.
    #[serde(default)]
    pub not_executed: NotExecuted,
}

/// Severity of a test without result, see [`Verification::not_executed`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotExecuted {
    /// A test without result is an error.
    #[default]
    Error,
    /// A test without result is a warning.
    Warning,
}

impl From<NotExecuted> for Severity {
    fn from(value: NotExecuted) -> Self {
        match value {
            NotExecuted::Error => Severity::Error,
            NotExecuted::Warning => Severity::Warning,
        }
    }
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
    ///
    /// Two `[[level]]` tables with the same type are an error, since an
    /// item's level would be ambiguous, and so is a type that is both a
    /// target and a fallback of `[satisfied_by]`. Variant names must start
    /// with a letter or digit, followed by letters, digits, `_` or `-`, and
    /// differ in more than case, since reports use them as directory names
    /// next to `index.html`; feature names follow Cargo's rules.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text).map_err(|error| ConfigError(error.to_string()))?;
        let mut seen = std::collections::HashSet::new();
        for level in &config.levels {
            if !seen.insert(&level.typ) {
                return Err(ConfigError(format!(
                    "more than one [[level]] has type '{}'",
                    level.typ
                )));
            }
        }
        let satisfied_by = &config.satisfied_by;
        if let Some(typ) = satisfied_by
            .fallback
            .iter()
            .find(|typ| satisfied_by.targets.contains(typ))
        {
            return Err(ConfigError(format!(
                "type '{typ}' is both a target and a fallback of [satisfied_by]"
            )));
        }
        let mut lower = std::collections::HashMap::new();
        for (name, variant) in &config.variants {
            if !is_variant_name(name) {
                return Err(ConfigError(format!(
                    "invalid variant name '{name}': expected letters, digits, '_' or '-', \
                     starting with a letter or a digit"
                )));
            }
            if let Some(other) = lower.insert(name.to_ascii_lowercase(), name) {
                return Err(ConfigError(format!(
                    "variant names '{other}' and '{name}' differ only in case"
                )));
            }
            if let Some(feature) = variant.features.iter().find(|f| !is_feature(f)) {
                return Err(ConfigError(format!(
                    "invalid feature '{feature}' in [variant.{name}]: expected letters, \
                     digits, '_', '-', '+' or '.', starting with a letter, a digit or '_'"
                )));
            }
        }
        Ok(config)
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

fn is_variant_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
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
not-executed = "warning"

[sources]
impl = ["src/**"]
utest = ["tests/**", "src/**/*_test.cpp"]
junit = ["build/*.xml"]
"#,
        )
        .unwrap();
        assert_eq!(config.levels.len(), 2);
        assert_eq!(config.verification.types, [typ("utest"), typ("vrev")]);
        assert_eq!(config.verification.not_executed, NotExecuted::Warning);
        assert_eq!(config.sources.requirements, None);
        assert_eq!(config.sources.junit, ["build/*.xml"]);
        let code = config.sources.code();
        assert_eq!(code.keys().copied().collect::<Vec<_>>(), ["impl", "utest"]);
        assert_eq!(code["utest"][1], "src/**/*_test.cpp");
        assert_eq!(config.level(&typ("dsn")).unwrap().name, "Design");
        assert!(config.level(&typ("impl")).is_none());
        assert!(config.has_parents(&typ("dsn")));
        assert!(!config.has_parents(&typ("swr")));
        assert_eq!(config.satisfied_by, SatisfiedBy::default());
        assert_eq!(config.satisfied_by.targets, [typ("cap")]);
    }

    #[test]
    fn reads_satisfied_by_targets() {
        let config =
            Config::parse("[satisfied_by]\ntargets = [\"feat\"]\nfallback = [\"swr\", \"dsn\"]\n")
                .unwrap();
        assert_eq!(config.satisfied_by.targets, [typ("feat")]);
        assert_eq!(config.satisfied_by.fallback, [typ("swr"), typ("dsn")]);
        let config = Config::parse("[satisfied_by]\nfallback = [\"swr\"]\n").unwrap();
        assert_eq!(config.satisfied_by.targets, [typ("cap")]);
    }

    #[test]
    fn reads_variants() {
        let config = Config::parse(
            "[variant.basis]\nfeatures = [\"motor_ctrl\", \"can\"]\n\n\
             [variant.premium]\nfeatures = [\"diag-uds\"]\njunit = [\"build/premium/*.xml\"]\n",
        )
        .unwrap();
        assert_eq!(
            config.variants.keys().collect::<Vec<_>>(),
            ["basis", "premium"]
        );
        assert_eq!(config.variants["basis"].features, ["motor_ctrl", "can"]);
        assert_eq!(config.variants["basis"].junit, None);
        assert!(config.variants["basis"].unknown_keys().is_empty());
        let typo = Config::parse("[variant.a]\nfeature = [\"x\"]\n").unwrap();
        assert_eq!(typo.variants["a"].unknown_keys(), ["feature"]);
        assert_eq!(
            config.variants["premium"].junit.as_deref().unwrap(),
            ["build/premium/*.xml"]
        );
    }

    #[test]
    fn rejects_invalid_variant_and_feature_names() {
        for (text, message) in [
            ("[variant.\"../x\"]\n", "invalid variant name '../x'"),
            ("[variant._x]\n", "invalid variant name '_x'"),
            (
                "[variant.\"index.html\"]\n",
                "invalid variant name 'index.html'",
            ),
            (
                "[variant.Basis]\n[variant.basis]\n",
                "variant names 'Basis' and 'basis' differ only in case",
            ),
            (
                "[variant.a]\nfeatures = [\"ota nightly\"]\n",
                "invalid feature 'ota nightly' in [variant.a]",
            ),
        ] {
            let error = Config::parse(text).unwrap_err().to_string();
            assert!(error.contains(message), "{error}");
        }
    }

    #[test]
    fn rejects_satisfied_by_type_that_is_target_and_fallback() {
        let error = Config::parse("[satisfied_by]\nfallback = [\"cap\"]\n").unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid shall.toml: type 'cap' is both a target and a fallback of [satisfied_by]"
        );
    }

    #[test]
    fn rejects_duplicate_level_types() {
        let error = Config::parse(
            "[[level]]\ntype = \"dsn\"\nname = \"A\"\n\n[[level]]\ntype = \"dsn\"\nname = \"B\"\n",
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid shall.toml: more than one [[level]] has type 'dsn'"
        );
    }

    #[test]
    fn rejects_invalid_types() {
        let error = Config::parse("[[level]]\ntype = \"l3\"\nname = \"x\"\n").unwrap_err();
        assert!(error.to_string().contains("invalid type 'l3'"), "{error}");
    }

    #[test]
    fn reads_requirement_sources() {
        let config = Config::parse("[sources]\nrequirements = [\"reqs/**/*.md\"]\n").unwrap();
        assert_eq!(
            config.sources.requirements.as_deref().unwrap(),
            ["reqs/**/*.md"]
        );
        assert!(config.sources.code().is_empty());
    }

    #[test]
    fn keeps_loading_with_unexpected_source_keys() {
        let config = Config::parse(
            "[sources]\nimpl = [\"src/**\"]\nexclude = \"build/**\"\n[sources.later]\nglobs = [\"x\"]\n",
        )
        .unwrap();
        assert_eq!(config.sources.code().len(), 1);
        assert_eq!(config.sources.invalid_keys(), ["exclude", "later"]);
    }

    #[test]
    fn rejects_unknown_severities() {
        let error = Config::parse("[verification]\nnot-executed = \"info\"\n").unwrap_err();
        assert!(
            error.to_string().contains("unknown variant `info`"),
            "{error}"
        );
    }

    #[test]
    fn empty_file_has_no_levels() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }
}
