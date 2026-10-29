//! Project configuration from `shall.toml`.
//!
//! Only the parts used so far are read; unknown tables and keys are
//! ignored, so a configuration written for a later version still loads.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::diagnostic::Severity;
use crate::id::ItemType;
use crate::item::Status;
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
    /// Other projects whose items this project uses, from the
    /// `[[import]]` tables.
    #[serde(default, rename = "import")]
    pub imports: Vec<Import>,
    /// The members of a workspace, from the `[workspace]` table; a
    /// `shall.toml` with it describes a workspace, not a project.
    #[serde(default)]
    pub workspace: Option<Workspace>,
    /// Settings of the report, from the `[report]` table.
    #[serde(default)]
    pub report: Report,
    /// When `shall check` fails, from the `[check]` table.
    #[serde(default)]
    pub check: Check,
}

/// The `[check]` table: when `shall check` fails, see [`crate::gate`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Check {
    /// The mode; `report` when not given.
    #[serde(default)]
    pub mode: Mode,
    /// Statuses from which an item counts in mode `mature`; `["approved"]`
    /// when not given.
    #[serde(default = "default_mature_status")]
    pub mature_status: Vec<Status>,
    /// Modes for the defects of single levels, from `[check.level.<type>]`.
    #[serde(default, rename = "level")]
    pub levels: BTreeMap<ItemType, LevelCheck>,
    /// All other keys, see [`Check::unknown_keys`].
    #[serde(flatten)]
    other: BTreeMap<String, toml::Value>,
}

impl Default for Check {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            mature_status: default_mature_status(),
            levels: BTreeMap::new(),
            other: BTreeMap::new(),
        }
    }
}

impl Check {
    /// Keys of `[check]` other than `mode`, `mature_status` and `level`;
    /// often typos, which would leave the check in mode `report`.
    pub fn unknown_keys(&self) -> Vec<&str> {
        self.other.keys().map(String::as_str).collect()
    }
}

fn default_mature_status() -> Vec<Status> {
    vec![Status::Approved]
}

/// A `[check.level.<type>]` table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LevelCheck {
    /// The mode for the defects of the level's items.
    pub mode: Mode,
}

/// When `shall check` fails because of defects.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Never; the defects are only reported.
    #[default]
    Report,
    /// On errors that are new compared with an earlier run, e.g. on the
    /// target branch.
    New,
    /// On errors at items whose status is mature.
    Mature,
    /// On every error and warning.
    Strict,
}

impl Mode {
    /// All modes.
    pub const ALL: [Mode; 4] = [Mode::Report, Mode::New, Mode::Mature, Mode::Strict];

    /// The name of the mode, as in `shall.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Report => "report",
            Mode::New => "new",
            Mode::Mature => "mature",
            Mode::Strict => "strict",
        }
    }
}

impl std::str::FromStr for Mode {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Mode::ALL
            .into_iter()
            .find(|mode| mode.as_str() == text)
            .ok_or_else(|| format!("unknown mode '{text}': expected report, new, mature or strict"))
    }
}

/// The `[report]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Report {
    /// Address of a line of code at a commit, with the placeholders
    /// `{sha}`, `{path}` and `{line}`, e.g.
    /// `https://git.example.org/team/project/src/commit/{sha}/{path}#L{line}`.
    /// Locations in the report link there when the commit is known.
    #[serde(default)]
    pub code_url: Option<String>,
}

/// The placeholders of [`Report::code_url`].
pub const CODE_URL_PLACEHOLDERS: [&str; 3] = ["sha", "path", "line"];

/// An `[[import]]`: another project of the repository whose items, levels
/// and test results become part of this project's trace.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Import {
    /// Name of the import, e.g. `plat`; items of the import are marked
    /// with it.
    pub name: String,
    /// Directory of the imported project, relative to this project's
    /// root, e.g. `../../platform`.
    pub path: Option<String>,
    /// URL of an external repository; not supported yet.
    pub git: Option<String>,
    /// Fixed revision of an external repository; not supported yet, and an
    /// error for a `path` import, which always follows the current state.
    pub rev: Option<String>,
}

/// The `[workspace]` table of the `shall.toml` at the root of a repository.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Workspace {
    /// Directories of the member projects, relative to the workspace root;
    /// `*` matches one directory level, e.g. `projects/*`.
    #[serde(default)]
    pub members: Vec<String>,
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
    /// item's level would be ambiguous, and so are levels whose `needs`
    /// form a cycle, a level needing itself included, since every item
    /// would need covering without end, and so is a type that is both a
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
        if let Some(cycle) = level_cycle(&config.levels) {
            return Err(ConfigError(format!(
                "the needs of [[level]] form a cycle: {}",
                cycle.join(" -> ")
            )));
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
        if let Some(url) = &config.report.code_url {
            check_code_url(url)?;
        }
        if config.workspace.is_some()
            && (!config.levels.is_empty()
                || !config.imports.is_empty()
                || !config.variants.is_empty()
                || config.sources != Sources::default()
                || config.verification != Verification::default()
                || config.satisfied_by != SatisfiedBy::default()
                || config.report != Report::default()
                || config.check != Check::default())
        {
            return Err(ConfigError(
                "a [workspace] is no project: move [[level]], [[import]], [variant.*], \
                 [sources], [verification], [satisfied_by], [report] and [check] into a member"
                    .to_owned(),
            ));
        }
        let mut names = std::collections::HashSet::new();
        for import in &config.imports {
            if !is_variant_name(&import.name) {
                return Err(ConfigError(format!(
                    "invalid import name '{}': expected letters, digits, '_' or '-', \
                     starting with a letter or a digit",
                    import.name
                )));
            }
            if !names.insert(&import.name) {
                return Err(ConfigError(format!(
                    "more than one [[import]] has name '{}'",
                    import.name
                )));
            }
            match (&import.path, &import.git, &import.rev) {
                (Some(_), None, None) => {}
                (Some(_), None, Some(_)) => {
                    return Err(ConfigError(format!(
                        "import '{}': 'rev' needs 'git'; a 'path' import always follows \
                         the current state",
                        import.name
                    )));
                }
                (Some(_), Some(_), _) => {
                    return Err(ConfigError(format!(
                        "import '{}' has both 'path' and 'git'",
                        import.name
                    )));
                }
                (None, Some(_), _) => {
                    return Err(ConfigError(format!(
                        "import '{}': imports with 'git' are not supported yet",
                        import.name
                    )));
                }
                (None, None, _) => {
                    return Err(ConfigError(format!(
                        "import '{}' needs a 'path'",
                        import.name
                    )));
                }
            }
        }
        Ok(config)
    }

    /// Adds the levels, verification types and `[satisfied_by]` types of
    /// imported projects, so that their items are traced against their own
    /// configuration.
    ///
    /// The `targets` and `fallback` types of `[satisfied_by]` are joined: a
    /// type is a target if it is one anywhere, otherwise a fallback if it
    /// is one anywhere.
    ///
    /// A level of the same type in this project and an import must need
    /// the same types, since an item's needs would be ambiguous otherwise;
    /// the level stays where it is first defined, in this project or the
    /// first import that has it.
    pub fn with_imports<'a>(
        &self,
        imports: impl IntoIterator<Item = (&'a str, &'a Config)>,
    ) -> Result<Config, ConfigError> {
        let mut config = self.clone();
        for (name, import) in imports {
            for level in &import.levels {
                match config.level(&level.typ) {
                    Some(existing) if existing.needs != level.needs => {
                        return Err(ConfigError(format!(
                            "level '{}' of import '{name}' needs {:?}, but {:?} where it is \
                             defined first",
                            level.typ,
                            type_names(&level.needs),
                            type_names(&existing.needs),
                        )));
                    }
                    Some(_) => {}
                    None => config.levels.push(level.clone()),
                }
            }
            for typ in &import.verification.types {
                if !config.verification.types.contains(typ) {
                    config.verification.types.push(typ.clone());
                }
            }
            let satisfied_by = &mut config.satisfied_by;
            for typ in &import.satisfied_by.targets {
                if !satisfied_by.targets.contains(typ) {
                    satisfied_by.targets.push(typ.clone());
                }
            }
            satisfied_by
                .fallback
                .retain(|typ| !import.satisfied_by.targets.contains(typ));
            for typ in &import.satisfied_by.fallback {
                if !satisfied_by.targets.contains(typ) && !satisfied_by.fallback.contains(typ) {
                    satisfied_by.fallback.push(typ.clone());
                }
            }
        }
        if let Some(cycle) = level_cycle(&config.levels) {
            return Err(ConfigError(format!(
                "the needs of [[level]] and those of the imports form a cycle: {}",
                cycle.join(" -> ")
            )));
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

/// A cycle of levels that need each other, such as `swr -> dsn -> swr`,
/// first by the order of the levels; none if the needs form no cycle.
/// Every item of such a level would need a covering item of the next one
/// without end.
fn level_cycle(levels: &[Level]) -> Option<Vec<String>> {
    let position = |typ: &ItemType| levels.iter().position(|level| &level.typ == typ);
    // 0 unvisited, 1 on the path, 2 done; depth first without recursion.
    let mut state = vec![0u8; levels.len()];
    for root in 0..levels.len() {
        if state[root] != 0 {
            continue;
        }
        let mut path = vec![(root, 0usize)];
        state[root] = 1;
        while let Some(&mut (node, ref mut next)) = path.last_mut() {
            let needs: Vec<usize> = levels[node].needs.iter().filter_map(position).collect();
            if let Some(&child) = needs.get(*next) {
                *next += 1;
                match state[child] {
                    0 => {
                        state[child] = 1;
                        path.push((child, 0));
                    }
                    1 => {
                        let start = path
                            .iter()
                            .position(|&(on_path, _)| on_path == child)
                            .expect("the child is on the path");
                        let mut cycle: Vec<String> = path[start..]
                            .iter()
                            .map(|&(level, _)| levels[level].typ.to_string())
                            .collect();
                        cycle.push(levels[child].typ.to_string());
                        return Some(cycle);
                    }
                    _ => {}
                }
            } else {
                state[node] = 2;
                path.pop();
            }
        }
    }
    None
}

/// A `code_url` must be an `http` or `https` address whose placeholders
/// are all known.
fn check_code_url(url: &str) -> Result<(), ConfigError> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(ConfigError(format!(
            "[report] code_url '{url}' must start with https:// or http://"
        )));
    }
    let mut rest = url;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            return Err(ConfigError(format!(
                "[report] code_url '{url}' has an unclosed '{{'"
            )));
        };
        let name = &after[..close];
        if !CODE_URL_PLACEHOLDERS.contains(&name) {
            return Err(ConfigError(format!(
                "[report] code_url '{url}' has the unknown placeholder '{{{name}}}'; \
                 known are {{sha}}, {{path}} and {{line}}"
            )));
        }
        rest = &after[close + 1..];
    }
    Ok(())
}

fn type_names(types: &[ItemType]) -> Vec<&str> {
    types.iter().map(ItemType::as_str).collect()
}

/// Whether the name may name a variant or an import: it is used as a
/// directory name.
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
    fn reads_imports_and_workspaces() {
        let config =
            Config::parse("[[import]]\nname = \"plat\"\npath = \"../../platform\"\n").unwrap();
        assert_eq!(config.imports[0].name, "plat");
        assert_eq!(config.imports[0].path.as_deref(), Some("../../platform"));
        let workspace =
            Config::parse("[workspace]\nmembers = [\"platform\", \"projects/*\"]\n").unwrap();
        assert_eq!(
            workspace.workspace.unwrap().members,
            ["platform", "projects/*"]
        );
    }

    #[test]
    fn rejects_invalid_imports_and_workspaces() {
        for (text, message) in [
            (
                "[[import]]\nname = \"a\"\npath = \"x\"\n[[import]]\nname = \"a\"\npath = \"y\"\n",
                "more than one [[import]] has name 'a'",
            ),
            (
                "[[import]]\nname = \"a.b\"\npath = \"x\"\n",
                "invalid import name 'a.b'",
            ),
            (
                "[[import]]\nname = \"a\"\npath = \"x\"\nrev = \"v1\"\n",
                "import 'a': 'rev' needs 'git'",
            ),
            (
                "[[import]]\nname = \"a\"\ngit = \"https://x\"\nrev = \"v1\"\n",
                "import 'a': imports with 'git' are not supported yet",
            ),
            ("[[import]]\nname = \"a\"\n", "import 'a' needs a 'path'"),
            (
                "[workspace]\nmembers = []\n[[level]]\ntype = \"x\"\nname = \"X\"\n",
                "a [workspace] is no project",
            ),
        ] {
            let error = Config::parse(text).unwrap_err().to_string();
            assert!(error.contains(message), "{error}");
        }
    }

    #[test]
    fn imports_add_levels_and_verification_types() {
        let project = Config::parse(
            "[[level]]\ntype = \"hlr\"\nname = \"H\"\n\n[[level]]\ntype = \"swr\"\nname = \"S\"\nneeds = [\"dsn\"]\n",
        )
        .unwrap();
        let platform = Config::parse(
            "[[level]]\ntype = \"swr\"\nname = \"Software\"\nneeds = [\"dsn\"]\n\n\
             [[level]]\ntype = \"dsn\"\nname = \"D\"\nneeds = [\"utest\"]\n\n\
             [verification]\ntypes = [\"utest\"]\n",
        )
        .unwrap();
        let merged = project.with_imports([("plat", &platform)]).unwrap();
        let types: Vec<&str> = merged
            .levels
            .iter()
            .map(|level| level.typ.as_str())
            .collect();
        assert_eq!(types, ["hlr", "swr", "dsn"]);
        assert_eq!(merged.level(&typ("swr")).unwrap().name, "S");
        assert_eq!(merged.verification.types, [typ("utest")]);
        let with_targets =
            Config::parse("[satisfied_by]\ntargets = [\"swr\"]\nfallback = [\"dsn\", \"hlr\"]\n")
                .unwrap();
        let fallback = Config::parse("[satisfied_by]\nfallback = [\"swr\"]\n").unwrap();
        let merged = fallback
            .with_imports([("plat", &with_targets)])
            .unwrap()
            .satisfied_by;
        assert_eq!(merged.targets, [typ("cap"), typ("swr")]);
        assert_eq!(merged.fallback, [typ("dsn"), typ("hlr")]);

        let other =
            Config::parse("[[level]]\ntype = \"swr\"\nname = \"S\"\nneeds = [\"impl\"]\n").unwrap();
        let back = Config::parse("[[level]]\ntype = \"swr\"\nname = \"S\"\nneeds = [\"dsn\"]\n\n[[level]]\ntype = \"dsn\"\nname = \"D\"\nneeds = [\"hlr\"]\n").unwrap();
        let hlr =
            Config::parse("[[level]]\ntype = \"hlr\"\nname = \"H\"\nneeds = [\"swr\"]\n").unwrap();
        let error = hlr.with_imports([("plat", &back)]).unwrap_err();
        assert!(
            error
                .to_string()
                .ends_with("form a cycle: hlr -> swr -> dsn -> hlr"),
            "{error}"
        );
        let error = project.with_imports([("plat", &other)]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid shall.toml: level 'swr' of import 'plat' needs [\"impl\"], but [\"dsn\"] \
             where it is defined first"
        );
    }

    #[test]
    fn reads_and_checks_the_code_url() {
        let config = Config::parse(
            "[report]\ncode_url = \"https://git.example.org/p/src/commit/{sha}/{path}#L{line}\"\n",
        )
        .unwrap();
        assert!(config.report.code_url.unwrap().contains("{sha}"));
        for (url, message) in [
            ("ftp://x/{path}", "must start with https://"),
            ("https://x/{sha}/{file}", "unknown placeholder '{file}'"),
            ("https://x/{sha", "unclosed '{'"),
        ] {
            let error = Config::parse(&format!("[report]\ncode_url = \"{url}\"\n"))
                .unwrap_err()
                .to_string();
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
    fn reads_check_modes() {
        let config = Config::parse(
            "[check]\nmode = \"new\"\nmature_status = [\"proposed\", \"approved\"]\n\n\
             [check.level.hlr]\nmode = \"mature\"\n",
        )
        .unwrap();
        assert_eq!(config.check.mode, Mode::New);
        assert_eq!(
            config.check.mature_status,
            [Status::Proposed, Status::Approved]
        );
        assert_eq!(config.check.levels[&typ("hlr")].mode, Mode::Mature);
        assert_eq!(Config::default().check.mature_status, [Status::Approved]);
        let error = Config::parse("[check]\nmode = \"loose\"\n").unwrap_err();
        assert!(
            error.to_string().contains("unknown variant `loose`"),
            "{error}"
        );
        assert_eq!("strict".parse::<Mode>(), Ok(Mode::Strict));
        assert!("x".parse::<Mode>().is_err());
    }

    #[test]
    fn rejects_cycles_of_levels() {
        let error = Config::parse(
            "[[level]]\ntype = \"sys\"\nname = \"S\"\nneeds = [\"swr\"]\n\n\
             [[level]]\ntype = \"swr\"\nname = \"W\"\nneeds = [\"dsn\"]\n\n\
             [[level]]\ntype = \"dsn\"\nname = \"D\"\nneeds = [\"impl\", \"swr\"]\n",
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid shall.toml: the needs of [[level]] form a cycle: swr -> dsn -> swr"
        );
        let error = Config::parse("[[level]]\ntype = \"req\"\nname = \"R\"\nneeds = [\"req\"]\n")
            .unwrap_err();
        assert!(error.to_string().ends_with("cycle: req -> req"), "{error}");
        // A diamond is no cycle.
        Config::parse(
            "[[level]]\ntype = \"a\"\nname = \"A\"\nneeds = [\"b\", \"c\"]\n\n\
             [[level]]\ntype = \"b\"\nname = \"B\"\nneeds = [\"c\"]\n\n\
             [[level]]\ntype = \"c\"\nname = \"C\"\n",
        )
        .unwrap();
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
