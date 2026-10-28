//! Matching test results to the tests that verification tags point at.
//!
//! Only tags whose type is a verification type of `shall.toml` (e.g.
//! `utest`) are matched; the test is the definition found right after the
//! tag (see [`crate::tags`]). How a test is named in JUnit XML depends on
//! how it is defined:
//!
//! | Definition | `classname` | `name` |
//! | --- | --- | --- |
//! | gtest `TEST(S, N)`, `TEST_F`, `GTEST_TEST` | `S` | `N` |
//! | gtest `TEST_P(S, N)` | `S` or `Instance/S` | `N/…` |
//! | gtest `TYPED_TEST(S, N)`, `TYPED_TEST_P` | `S/…` or `Instance/S/…` | `N` |
//! | Rust `#[test] fn n` | test binary, e.g. `crate` | `module::path::n` |
//! | Rust `#[rstest]` with cases, `#[test_case]` | test binary | `module::path::n::…` |
//!
//! A parameterized or typed test has one result per instance or case. A
//! gtest `file` attribute gives a result to the file of a verification
//! tag it names (equal to it or ending with `/` and its path; the longest
//! such file). Results of another tagged file never count; if results
//! name the tag's file, only those count, so that equal test names in
//! other test programs do not match.
//!
//! For Rust, the module path is the one of the file within its crate,
//! followed by the inline modules around the test; `#[path]` attributes
//! are not followed. The crate is the directory before the last `src`
//! component, or else before the last `tests` component:
//!
//! - `src/a/b.rs` and `src/a/b/mod.rs` are `a::b` (also below
//!   `src/tests/`);
//! - `src/lib.rs` and `src/main.rs` add nothing, nor do `src/bin/x.rs` and
//!   `src/bin/x/main.rs` in the binary `x`, whose other files are modules;
//! - `tests/x.rs` and `tests/x/main.rs` add nothing in the integration test
//!   `x`; another file `tests/x/a.rs` is either module `a` of that test or,
//!   as for shared code in `tests/common/`, module `x::a` of some test;
//!   `tests/x/mod.rs` is only the shared module `x`.
//!
//! Every result named `path::name::…` counts as a case of the test
//! `path::name`, also one of a module `name` next to the function.
//!
//! A result whose `classname::name` is that path matches as well if its
//! name has no `::` and its `<testsuite>` has `package="test"` or is not
//! named like its class, as libtest's own JUnit output writes it
//! (`classname="a::tests" name="n"` in `<testsuite name="test"
//! package="test">`); nextest names already hold the full path and name
//! the suite like the class, the binary, without a `package`. When the
//! class name is a `cargo nextest` binary ID (`crate`, `crate::bin/x` or
//! `crate::x`), it must fit the file: the library for `src/lib.rs`, the
//! library or the package's binary for `src/main.rs` and other modules
//! under `src/` (any binary, without the package name), `crate::bin/x`
//! for `src/bin/x.rs`, `crate::x` for `tests/x.rs`. With the package names
//! from [`Packages`], the crate must be the package of the crate
//! directory. Without them,
//! if results of several crates remain, those of the crate named like the
//! crate's directory are taken; a result of another crate with the same
//! test path may then match.
//!
//! All results of a test count: it failed if one of them failed,
//! otherwise it passed if one passed; if all were skipped, it was
//! skipped. A test without any result was not executed; `cargo nextest`
//! leaves ignored tests out of its JUnit output, so they are not executed,
//! not skipped.
//!
//! Defects, at the line of the tag: a failed test is an error, a skipped
//! one a warning, one that was not executed has the severity of
//! `[verification] not-executed`. A verification tag without a test
//! definition after it gets a warning, since it can have no result.
//! Results that match no tag are not reported.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::Serialize;

use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::id::ItemId;
use crate::item::{Item, Location, Origin, TestKind, TestName};
use crate::junit::{Outcome, TestResult};

/// Outcome of the tests of all verification tags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evaluation {
    /// One entry per verification tag item, in the order of the items.
    pub runs: Vec<TestRun>,
    /// Failed, skipped and not executed tests.
    pub defects: Vec<Diagnostic>,
}

/// The test of one verification tag item and its results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestRun {
    /// The tag item.
    pub item: ItemId,
    /// Where the tag is.
    pub location: Location,
    /// The test as runners name it, e.g. `Shutdown.DisablesPwm` or
    /// `motor::tests::shuts_down`; none without a test definition.
    pub test: Option<String>,
    /// The combined outcome.
    pub verdict: Verdict,
    /// The matching results, as ascending indices into the results passed
    /// to [`evaluate`]: unlike locations, they also tell apart the results
    /// of a JUnit file written on one line.
    pub results: Vec<usize>,
}

/// Combined outcome of the results of a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// At least one result passed and none failed.
    Passed,
    /// At least one result failed.
    Failed,
    /// All results were skipped.
    Skipped,
    /// There is no result.
    NotExecuted,
    /// No test definition follows the tag.
    NoTest,
}

/// Package names of Rust crates, by the directory of their `Cargo.toml`
/// relative to the project root (`""` for the root itself), e.g.
/// `crates/core` → `proj-core`, and the directories of the projects whose
/// files are traced together, e.g. the members of a workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Packages {
    names: BTreeMap<String, String>,
    projects: BTreeSet<String>,
}

impl Packages {
    /// Adds the package of the `Cargo.toml` in `dir`, if it defines one.
    pub fn add(&mut self, dir: impl Into<String>, cargo_toml: &str) {
        if let Some(name) = package_name(cargo_toml) {
            self.names.insert(dir.into(), name);
        }
    }

    /// Adds the directory of a project below the root, such as
    /// `platform` in a workspace: the path of a Rust file below it is read
    /// from there, so that a project in a directory named `src` or `tests`
    /// is no crate root.
    pub fn add_project(&mut self, dir: impl Into<String>) {
        let dir = dir.into();
        if !dir.is_empty() {
            self.projects.insert(dir);
        }
    }

    fn get(&self, dir: &str) -> Option<&str> {
        self.names.get(dir).map(String::as_str)
    }

    /// Splits a path into the closest project directory and the rest.
    fn split_project<'p>(&self, path: &'p str) -> (Option<&'p str>, &'p str) {
        self.projects
            .iter()
            .filter(|dir| {
                path.strip_prefix(dir.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|dir| dir.len())
            .map_or((None, path), |dir| {
                (Some(&path[..dir.len()]), &path[dir.len() + 1..])
            })
    }
}

/// The `[package] name` of a `Cargo.toml`.
fn package_name(cargo_toml: &str) -> Option<String> {
    let manifest: toml::Table = cargo_toml.parse().ok()?;
    let name = manifest.get("package")?.get("name")?.as_str()?;
    Some(name.to_owned())
}

/// Matches the results to the tests of the verification tags in `items`.
pub fn evaluate(
    items: &[Item],
    results: &[TestResult],
    config: &Config,
    packages: &Packages,
) -> Evaluation {
    evaluate_variant(items, &[], results, config, packages)
}

/// Matches the results to the tests of the verification tags in the
/// `inside` items of a [variant](crate::variant). The files of `outside`
/// tags still own the results that name them, so that such a result never
/// counts for an equally named test inside; a result that names no file,
/// such as a gtest result without `file` attribute, goes to every tag of
/// that test in the variant, as without a variant.
pub fn evaluate_variant(
    inside: &[Item],
    outside: &[Item],
    results: &[TestResult],
    config: &Config,
    packages: &Packages,
) -> Evaluation {
    let items = inside;
    let tagged: BTreeSet<&str> = inside
        .iter()
        .chain(outside)
        .filter(|item| {
            matches!(item.origin, Origin::Tag { test: Some(_), .. })
                && config.verification.types.contains(item.id.typ())
        })
        .map(|item| item.location.path())
        .collect();
    let index = Index::new(results, packages, tagged);
    let mut runs = Vec::new();
    let mut defects = Vec::new();
    let mut reported = BTreeSet::new();
    for item in items {
        let Origin::Tag { text, test } = &item.origin else {
            continue;
        };
        if !config.verification.types.contains(item.id.typ()) {
            continue;
        }
        let Some(test) = test else {
            let message = format!("tag {text}: no test definition follows the tag");
            push_once(
                &mut defects,
                &mut reported,
                Diagnostic::warning(item.location.clone(), message),
            );
            runs.push(TestRun {
                item: item.id.clone(),
                location: item.location.clone(),
                test: None,
                verdict: Verdict::NoTest,
                results: Vec::new(),
            });
            continue;
        };
        let (name, matched) = index.matching(test, item.location.path());
        let verdict = verdict(&matched);
        let defect = match verdict {
            Verdict::Failed => Some(Diagnostic::error(
                item.location.clone(),
                format!("test {name} failed"),
            )),
            Verdict::Skipped => Some(Diagnostic::warning(
                item.location.clone(),
                format!("test {name} was skipped"),
            )),
            Verdict::NotExecuted => Some(Diagnostic {
                location: item.location.clone(),
                severity: config.verification.not_executed.into(),
                message: format!("test {name} was not executed: no result in the JUnit files"),
            }),
            Verdict::Passed | Verdict::NoTest => None,
        };
        if let Some(defect) = defect {
            push_once(&mut defects, &mut reported, defect);
        }
        runs.push(TestRun {
            item: item.id.clone(),
            location: item.location.clone(),
            test: Some(name),
            verdict,
            results: matched
                .iter()
                .map(|&result| {
                    results
                        .element_offset(result)
                        .expect("matched results come from `results`")
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        });
    }
    Evaluation { runs, defects }
}

/// Adds a defect unless the same one is already there, e.g. from a tag
/// with several targets.
fn push_once(defects: &mut Vec<Diagnostic>, reported: &mut BTreeSet<String>, defect: Diagnostic) {
    if reported.insert(defect.to_string()) {
        defects.push(defect);
    }
}

fn verdict(results: &[&TestResult]) -> Verdict {
    if results.is_empty() {
        Verdict::NotExecuted
    } else if results
        .iter()
        .any(|result| matches!(result.outcome, Outcome::Failed { .. }))
    {
        Verdict::Failed
    } else if results
        .iter()
        .any(|result| result.outcome == Outcome::Passed)
    {
        Verdict::Passed
    } else {
        Verdict::Skipped
    }
}

/// Results by name, to find the candidates of a test quickly.
struct Index<'a> {
    /// gtest results by test name without the `/…` instance suffix.
    by_name: HashMap<&'a str, Vec<&'a TestResult>>,
    /// Rust results by full path: the name, and `classname::name` for
    /// names without `::` as libtest writes them (suite with
    /// `package="test"`, or not named like the class).
    by_path: BTreeMap<String, Vec<&'a TestResult>>,
    packages: &'a Packages,
    /// Files with tags, to give a gtest result with a `file` attribute to
    /// the tagged file it names most precisely.
    tagged: BTreeSet<&'a str>,
}

impl<'a> Index<'a> {
    fn new(results: &'a [TestResult], packages: &'a Packages, tagged: BTreeSet<&'a str>) -> Self {
        let mut by_name: HashMap<&str, Vec<&TestResult>> = HashMap::new();
        let mut by_path: BTreeMap<String, Vec<&TestResult>> = BTreeMap::new();
        for result in results {
            let base = result.name.split('/').next().unwrap_or_default();
            by_name.entry(base).or_default().push(result);
            by_path.entry(result.name.clone()).or_default().push(result);
            // nextest names its suite like the class, the test binary;
            // libtest marks its suite with package="test".
            let libtest = result.package.as_deref() == Some("test");
            if !result.classname.is_empty()
                && !result.name.contains("::")
                && (libtest || result.classname != result.suite)
            {
                by_path
                    .entry(format!("{}::{}", result.classname, result.name))
                    .or_default()
                    .push(result);
            }
        }
        Self {
            by_name,
            by_path,
            packages,
            tagged,
        }
    }

    /// The name of the test as runners report it and its results.
    fn matching(&self, test: &TestName, path: &str) -> (String, Vec<&'a TestResult>) {
        match (&test.suite, test.kind) {
            (
                Some(suite),
                TestKind::Gtest | TestKind::GtestParameterized | TestKind::GtestTyped,
            ) => {
                let candidates = self.by_name.get(test.name.as_str());
                let matched: Vec<&TestResult> = candidates
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|result| gtest_matches(test.kind, suite, &test.name, result))
                    .collect();
                // Results of another tagged file are not this file's; of
                // the others, those that name this file win.
                let matched: Vec<&TestResult> = matched
                    .into_iter()
                    .filter(|result| {
                        result
                            .file
                            .as_deref()
                            .and_then(|file| self.owner(file))
                            .is_none_or(|owner| owner == path)
                    })
                    .collect();
                let same_file: Vec<&TestResult> = matched
                    .iter()
                    .copied()
                    .filter(|result| {
                        result.file.as_deref().and_then(|file| self.owner(file)) == Some(path)
                    })
                    .collect();
                let matched = if same_file.is_empty() {
                    matched
                } else {
                    same_file
                };
                (test.to_string(), matched)
            }
            _ => {
                let mut first = None;
                for file in RustFile::interpretations_in(path, self.packages) {
                    let full = file.path_of(test);
                    let matched = file.select(&full, &self.rust_candidates(&full), self.packages);
                    if !matched.is_empty() {
                        return (full, matched);
                    }
                    first.get_or_insert(full);
                }
                (first.unwrap_or_default(), Vec::new())
            }
        }
    }

    /// The tagged file that a gtest `file` attribute names: the longest
    /// one it ends with.
    fn owner(&self, file: &str) -> Option<&str> {
        self.tagged
            .iter()
            .copied()
            .filter(|path| same_path(file, path))
            .max_by_key(|path| path.len())
    }

    /// Results named `full`, or `full::…` for the cases of `#[rstest]`
    /// and `#[test_case]`.
    fn rust_candidates(&self, full: &str) -> Vec<&'a TestResult> {
        let cases = format!("{full}::");
        let mut candidates: Vec<&TestResult> = self
            .by_path
            .get(full)
            .into_iter()
            .flatten()
            .copied()
            .collect();
        candidates.extend(
            self.by_path
                .range::<str, _>((
                    std::ops::Bound::Included(cases.as_str()),
                    std::ops::Bound::Unbounded,
                ))
                .take_while(|(name, _)| name.starts_with(&cases))
                .flat_map(|(_, results)| results.iter().copied()),
        );
        // A result can be found by its name and by `classname::name`.
        let mut seen = HashSet::new();
        candidates.retain(|result| seen.insert(std::ptr::from_ref(*result)));
        candidates
    }
}

/// Whether the `file` attribute of a gtest result names the project file
/// `path`: equal, or ending with `/` and the path.
fn same_path(file: &str, path: &str) -> bool {
    let file = file.replace('\\', "/");
    file == path || file.ends_with(&format!("/{path}"))
}

fn gtest_matches(kind: TestKind, suite: &str, name: &str, result: &TestResult) -> bool {
    let class = result.classname.as_str();
    let in_suite = |class: &str| class == suite || class.ends_with(&format!("/{suite}"));
    match kind {
        TestKind::GtestParameterized => {
            in_suite(class)
                && result
                    .name
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with('/'))
        }
        TestKind::GtestTyped => {
            result.name == name
                && class
                    .rsplit_once('/')
                    .is_some_and(|(class, _)| in_suite(class))
        }
        _ => result.name == name && class == suite,
    }
}

/// Which crate target a Rust file belongs to, as far as its path tells.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    /// `src/lib.rs`, the library.
    Lib,
    /// A module under `src/`, which may belong to the library or to the
    /// binary of `src/main.rs`.
    Module,
    /// `src/main.rs`, the binary named like the package.
    Main,
    /// The binary `src/bin/<name>.rs` or `src/bin/<name>/`.
    Bin(String),
    /// The integration test `tests/<name>.rs` or `tests/<name>/`.
    Test(String),
    /// Shared code under `tests/`, or a file outside of `src/` and
    /// `tests/`.
    Unknown,
}

/// What the path of a Rust file tells about its tests.
#[derive(Debug, PartialEq, Eq)]
struct RustFile {
    /// Directory that contains `src/` or `tests/`, relative to the project
    /// root, `""` for the root itself.
    crate_dir: Option<String>,
    target: Target,
    /// Module path of the file within its crate target.
    modules: Vec<String>,
}

impl RustFile {
    /// The possible readings of a path below the closest project
    /// directory of `packages`, the likelier first.
    fn interpretations_in(path: &str, packages: &Packages) -> Vec<Self> {
        let (project, rest) = packages.split_project(path);
        let mut files = Self::interpretations(rest);
        if let Some(project) = project {
            for file in &mut files {
                file.crate_dir = file.crate_dir.take().map(|dir| {
                    if dir.is_empty() {
                        project.to_owned()
                    } else {
                        format!("{project}/{dir}")
                    }
                });
            }
        }
        files
    }

    /// The possible readings of a path, the likelier first.
    fn interpretations(path: &str) -> Vec<Self> {
        let parts: Vec<&str> = path.split('/').collect();
        let root = parts
            .iter()
            .rposition(|part| *part == "src")
            .or_else(|| parts.iter().rposition(|part| *part == "tests"));
        let Some(root) = root else {
            return vec![Self {
                crate_dir: None,
                target: Target::Unknown,
                modules: Vec::new(),
            }];
        };
        let crate_dir = Some(parts[..root].join("/"));
        let rest = &parts[root + 1..];
        let file = |target, modules: &[&str]| Self {
            crate_dir: crate_dir.clone(),
            target,
            modules: modules
                .iter()
                .filter(|part| **part != "mod.rs")
                .map(|part| stem(part).to_owned())
                .collect(),
        };
        let name = |part: &str| stem(part).to_owned();
        match (parts[root], rest) {
            ("src", ["lib.rs"]) => vec![file(Target::Lib, &[])],
            ("src", ["main.rs"]) => vec![file(Target::Main, &[])],
            ("src", ["bin", bin] | ["bin", bin, "main.rs"]) => {
                vec![file(Target::Bin(name(bin)), &[])]
            }
            ("src", ["bin", bin, modules @ ..]) => vec![file(Target::Bin(name(bin)), modules)],
            ("src", modules) => vec![file(Target::Module, modules)],
            ("tests", [test] | [test, "main.rs"]) => vec![file(Target::Test(name(test)), &[])],
            // Cargo does not build tests/x/mod.rs as a test of its own.
            (_, [_, "mod.rs"]) => vec![file(Target::Unknown, rest)],
            (_, [dir, modules @ ..]) => vec![
                file(Target::Test(name(dir)), modules),
                file(Target::Unknown, rest),
            ],
            (_, []) => vec![file(Target::Unknown, &[])],
        }
    }

    /// The full path of a test in this file.
    fn path_of(&self, test: &TestName) -> String {
        let parts: Vec<&str> = self
            .modules
            .iter()
            .map(String::as_str)
            .chain(test.module.iter().map(String::as_str))
            .chain([test.name.as_str()])
            .collect();
        parts.join("::")
    }

    /// The candidates that fit this file, see the module documentation.
    fn select<'a>(
        &self,
        full: &str,
        candidates: &[&'a TestResult],
        packages: &Packages,
    ) -> Vec<&'a TestResult> {
        let package = self.crate_dir.as_deref().and_then(|dir| packages.get(dir));
        let cases = format!("{full}::");
        let fits = |result: &&TestResult| {
            // Only results named by their path within the binary carry
            // the binary in the class name.
            if result.name != full && !result.name.starts_with(&cases) {
                return true;
            }
            let Some((name, target)) = nextest_binary(&result.classname) else {
                return true;
            };
            if package.is_some_and(|package| package != name) {
                return false;
            }
            let bin = target.and_then(|target| target.strip_prefix("bin/"));
            match (&self.target, target) {
                (Target::Lib, _) => target.is_none(),
                (Target::Module, None) => true,
                (Target::Module, Some(_)) => {
                    bin.is_some_and(|bin| package.is_none_or(|package| bin == package))
                }
                (Target::Main, _) => {
                    bin.is_some_and(|bin| package.is_none_or(|package| bin == package))
                }
                (Target::Bin(own), _) => bin == Some(own.as_str()),
                (Target::Test(own), Some(target)) => target == own,
                (Target::Test(_), None) => false,
                (Target::Unknown, _) => true,
            }
        };
        let fitting: Vec<&TestResult> = candidates.iter().copied().filter(fits).collect();
        if package.is_some() {
            return fitting;
        }
        let crates: BTreeSet<&str> = fitting
            .iter()
            .filter_map(|result| Some(nextest_binary(&result.classname)?.0))
            .collect();
        let dir_name = self
            .crate_dir
            .as_deref()
            .and_then(|dir| dir.rsplit('/').next())
            .filter(|name| !name.is_empty());
        if crates.len() > 1
            && let Some(dir) = dir_name
        {
            let own: Vec<&TestResult> = fitting
                .iter()
                .copied()
                .filter(|result| {
                    nextest_binary(&result.classname).is_some_and(|(name, _)| name == dir)
                })
                .collect();
            if !own.is_empty() {
                return own;
            }
        }
        fitting
    }
}

/// Splits a `cargo nextest` binary ID such as `crate`, `crate::bin/x` or
/// `crate::x` into crate and target; none for other class names.
fn nextest_binary(classname: &str) -> Option<(&str, Option<&str>)> {
    let valid = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    match classname.split_once("::") {
        None => valid(classname).then_some((classname, None)),
        Some((name, target)) => {
            let plain = target.split_once('/').map_or(target, |(_, plain)| plain);
            (valid(name) && valid(plain)).then_some((name, Some(target)))
        }
    }
}

fn stem(file: &str) -> &str {
    file.strip_suffix(".rs").unwrap_or(file)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{junit, tags};

    const CONFIG: &str = "[verification]\ntypes = [\"utest\"]\n";

    fn evaluate_files(sources: &[(&str, &str)], xml: &str, config: &str) -> Evaluation {
        evaluate_with(sources, xml, config, &Packages::default())
    }

    fn evaluate_with(
        sources: &[(&str, &str)],
        xml: &str,
        config: &str,
        packages: &Packages,
    ) -> Evaluation {
        let items: Vec<Item> = sources
            .iter()
            .flat_map(|(path, text)| tags::scan(Path::new(path), text).unwrap().items)
            .collect();
        let parsed = junit::parse(Path::new("out.xml"), xml).unwrap();
        assert_eq!(parsed.diagnostics, []);
        evaluate(
            &items,
            &parsed.results,
            &Config::parse(config).unwrap(),
            packages,
        )
    }

    fn messages(evaluation: &Evaluation) -> Vec<String> {
        evaluation.defects.iter().map(ToString::to_string).collect()
    }

    fn verdicts(evaluation: &Evaluation) -> Vec<(Option<&str>, Verdict, usize)> {
        evaluation
            .runs
            .iter()
            .map(|run| (run.test.as_deref(), run.verdict, run.results.len()))
            .collect()
    }

    fn testcase(classname: &str, name: &str, body: &str) -> String {
        format!("<testcase classname=\"{classname}\" name=\"{name}\">{body}</testcase>\n")
    }

    fn suite(cases: &[String]) -> String {
        format!(
            "<testsuites><testsuite name=\"all\">\n{}</testsuite></testsuites>",
            cases.concat()
        )
    }

    #[test]
    fn matches_gtest_definitions() {
        let source = "\
// [utest->dsn~a~1]
TEST(Plain, Works) {}
// [utest->dsn~a~1]
TEST_F(Fixture, Fails) {}
// [utest->dsn~a~1]
TEST_P(Param, Each) {}
// [utest->dsn~a~1]
TYPED_TEST(Typed, Each) {}
// [utest->dsn~a~1]
TEST(Plain, Missing) {}
// [utest->dsn~a~1]
TEST(Plain, Skips) {}
// [impl->dsn~a~1]
void not_a_test() {}
// [utest->dsn~a~1]
int helper() {}
";
        let xml = suite(&[
            testcase("Plain", "Works", ""),
            testcase("Fixture", "Fails", "<failure message=\"x\"/>"),
            testcase("Other", "Fails", ""),
            testcase("Inst/Param", "Each/0", ""),
            testcase("Inst/Param", "Each/1", ""),
            testcase("Param", "Each/named", ""),
            testcase("Inst/Param", "EachOther/0", ""),
            testcase("Typed/0", "Each", ""),
            testcase("Inst/Typed/1", "Each", ""),
            testcase("Typed", "Each", ""),
            testcase("Plain", "Skips", "<skipped/>"),
            testcase("Plain", "Missing/0", ""),
        ]);
        let evaluation = evaluate_files(&[("t.cpp", source)], &xml, CONFIG);
        assert_eq!(
            verdicts(&evaluation),
            [
                (Some("Plain.Works"), Verdict::Passed, 1),
                (Some("Fixture.Fails"), Verdict::Failed, 1),
                (Some("Param.Each"), Verdict::Passed, 3),
                (Some("Typed.Each"), Verdict::Passed, 2),
                (Some("Plain.Missing"), Verdict::NotExecuted, 0),
                (Some("Plain.Skips"), Verdict::Skipped, 1),
                (None, Verdict::NoTest, 0),
            ]
        );
        assert_eq!(
            messages(&evaluation),
            [
                "t.cpp:3: test Fixture.Fails failed",
                "t.cpp:9: test Plain.Missing was not executed: no result in the JUnit files",
                "t.cpp:11: test Plain.Skips was skipped",
                "t.cpp:15: tag [utest->dsn~a~1]: no test definition follows the tag",
            ]
        );
    }

    #[test]
    fn one_failed_instance_fails_the_test() {
        let source = "// [utest->dsn~a~1, dsn~b~1]\nTEST_P(Param, Each) {}\n";
        let xml = suite(&[
            testcase("Inst/Param", "Each/0", "<skipped/>"),
            testcase("Inst/Param", "Each/1", "<error message=\"crash\"/>"),
            testcase("Inst/Param", "Each/2", ""),
        ]);
        let evaluation = evaluate_files(&[("t.cpp", source)], &xml, CONFIG);
        assert_eq!(
            verdicts(&evaluation),
            [
                (Some("Param.Each"), Verdict::Failed, 3),
                (Some("Param.Each"), Verdict::Failed, 3),
            ]
        );
        // One defect for both targets of the tag.
        assert_eq!(messages(&evaluation), ["t.cpp:1: test Param.Each failed"]);
    }

    #[test]
    fn only_verification_types_are_matched() {
        let source = "// [itest->dsn~a~1]\nTEST(A, B) {}\n";
        let evaluation = evaluate_files(&[("t.cpp", source)], "<testsuites/>", CONFIG);
        assert_eq!(evaluation.runs, []);
        let config = "[verification]\ntypes = [\"utest\", \"itest\"]\nnot-executed = \"warning\"\n";
        let evaluation = evaluate_files(&[("t.cpp", source)], "<testsuites/>", config);
        assert_eq!(
            evaluation.defects[0].severity,
            crate::diagnostic::Severity::Warning
        );
    }

    #[test]
    fn matches_rust_tests_by_module_path_and_binary() {
        let lib = "\
#[cfg(test)]
mod tests {
    // [utest->dsn~a~1]
    #[test]
    fn parses() {}
}
";
        let integration = "// [utest->dsn~a~1]\n#[test]\nfn runs() {}\n";
        let xml = suite(&[
            testcase("core", "id::tests::parses", "<failure/>"),
            testcase("cli", "id::tests::parses", ""),
            testcase("core::bin/tool", "id::tests::parses", ""),
            testcase("core", "runs", ""),
            testcase("core::cli", "runs", ""),
            testcase("core::other", "runs", "<failure/>"),
            testcase("core::bin/tool", "main_test", ""),
            testcase("app::bin/app", "tests::lone", ""),
        ]);
        let sources = [
            ("crates/core/src/id.rs", lib),
            ("crates/core/tests/cli.rs", integration),
            (
                "crates/core/src/bin/tool.rs",
                "// [utest->dsn~a~1]\n#[test]\nfn main_test() {}\n",
            ),
            ("crates/core/src/id/mod.rs", lib),
            (
                "app/src/main.rs",
                "mod tests {\n// [utest->dsn~a~1]\n#[test]\nfn lone() {}\n}\n",
            ),
        ];
        let evaluation = evaluate_files(&sources, &xml, CONFIG);
        assert_eq!(
            verdicts(&evaluation),
            [
                // core and its binary, which may include src/id.rs, not cli.
                (Some("id::tests::parses"), Verdict::Failed, 2),
                (Some("runs"), Verdict::Passed, 1),
                (Some("main_test"), Verdict::Passed, 1),
                (Some("id::tests::parses"), Verdict::Failed, 2),
                (Some("tests::lone"), Verdict::Passed, 1),
            ]
        );
    }

    #[test]
    fn derives_module_paths_from_files() {
        let files = |path: &str| -> Vec<(Option<String>, Target, String)> {
            RustFile::interpretations(path)
                .into_iter()
                .map(|file| (file.crate_dir, file.target, file.modules.join("::")))
                .collect()
        };
        let dir = |name: &str| Some(name.to_owned());
        let test = |name: &str| Target::Test(name.to_owned());
        let bin = |name: &str| Target::Bin(name.to_owned());
        assert_eq!(files("src/lib.rs"), [(dir(""), Target::Lib, String::new())]);
        assert_eq!(
            files("src/main.rs"),
            [(dir(""), Target::Main, String::new())]
        );
        assert_eq!(
            files("c/a/src/x/y.rs"),
            [(dir("c/a"), Target::Module, "x::y".into())]
        );
        assert_eq!(
            files("a/src/x/mod.rs"),
            [(dir("a"), Target::Module, "x".into())]
        );
        // A module directory named tests is no integration test.
        assert_eq!(
            files("a/src/tests/mod.rs"),
            [(dir("a"), Target::Module, "tests".into())]
        );
        assert_eq!(
            files("a/src/tests/f.rs"),
            [(dir("a"), Target::Module, "tests::f".into())]
        );
        assert_eq!(
            files("a/src/bin/t.rs"),
            [(dir("a"), bin("t"), String::new())]
        );
        assert_eq!(
            files("a/src/bin/t/main.rs"),
            [(dir("a"), bin("t"), String::new())]
        );
        assert_eq!(
            files("a/src/bin/t/m.rs"),
            [(dir("a"), bin("t"), "m".into())]
        );
        assert_eq!(
            files("a/tests/t.rs"),
            [(dir("a"), test("t"), String::new())]
        );
        assert_eq!(
            files("a/tests/t/main.rs"),
            [(dir("a"), test("t"), String::new())]
        );
        assert_eq!(
            files("a/tests/common/mod.rs"),
            [(dir("a"), Target::Unknown, "common".into())]
        );
        assert_eq!(
            files("a/tests/t/helpers.rs"),
            [
                (dir("a"), test("t"), "helpers".into()),
                (dir("a"), Target::Unknown, "t::helpers".into()),
            ]
        );
        assert_eq!(files("lib.rs"), [(None, Target::Unknown, String::new())]);
    }

    fn rust_test(attributes: &str, name: &str) -> String {
        format!("mod tests {{\n// [utest->dsn~a~1]\n{attributes}\nfn {name}() {{}}\n}}\n")
    }

    #[test]
    fn matches_the_cases_of_rstest_and_test_case() {
        let source = [
            rust_test("#[rstest]\n#[case(1)]\n#[case(2)]", "add"),
            rust_test("#[test_case(1 ; \"one\")]", "sub"),
            rust_test("#[test]", "plain"),
        ]
        .concat();
        let xml = suite(&[
            testcase("c", "tests::add::case_1", ""),
            testcase("c", "tests::add::case_2", "<failure/>"),
            testcase("c", "tests::sub::one", ""),
            testcase("c", "tests::addition", "<failure/>"),
            testcase("c", "tests::plain", ""),
            testcase("c", "tests::plain_other", "<failure/>"),
        ]);
        let evaluation = evaluate_files(&[("c/src/lib.rs", &source)], &xml, CONFIG);
        assert_eq!(
            verdicts(&evaluation),
            [
                (Some("tests::add"), Verdict::Failed, 2),
                (Some("tests::sub"), Verdict::Passed, 1),
                (Some("tests::plain"), Verdict::Passed, 1),
            ]
        );
    }

    #[test]
    fn binaries_take_only_their_own_results() {
        let source = rust_test("#[test]", "works");
        let xml = suite(&[
            testcase("c", "tests::works", "<failure/>"),
            testcase("c::bin/a", "tests::works", ""),
            testcase("c::bin/b", "tests::works", "<failure/>"),
            testcase("c::bin/c", "tests::works", ""),
            testcase("c::t", "tests::works", "<failure/>"),
        ]);
        let mut packages = Packages::default();
        packages.add("c", "[package]\nname = \"c\"\n");
        let evaluation = evaluate_with(
            &[
                ("c/src/bin/a.rs", &source),
                ("c/src/main.rs", &source),
                ("c/tests/t/helpers.rs", &source),
                ("c/tests/common/mod.rs", &source),
            ],
            &xml,
            CONFIG,
            &packages,
        );
        assert_eq!(
            verdicts(&evaluation),
            [
                (Some("tests::works"), Verdict::Passed, 1),
                // The package's binary, not the library or other binaries.
                (Some("tests::works"), Verdict::Passed, 1),
                // Module helpers of the integration test t.
                (Some("helpers::tests::works"), Verdict::NotExecuted, 0),
                (Some("common::tests::works"), Verdict::NotExecuted, 0),
            ]
        );
        let xml = suite(&[
            testcase("c::t", "helpers::tests::works", ""),
            testcase("c::u", "common::tests::works", "<failure/>"),
        ]);
        let evaluation = evaluate_with(
            &[
                ("c/tests/t/helpers.rs", &source),
                ("c/tests/common/mod.rs", &source),
            ],
            &xml,
            CONFIG,
            &packages,
        );
        assert_eq!(
            verdicts(&evaluation),
            [
                (Some("helpers::tests::works"), Verdict::Passed, 1),
                (Some("common::tests::works"), Verdict::Failed, 1),
            ]
        );
    }

    #[test]
    fn package_names_keep_crates_apart() {
        let source = rust_test("#[test]", "it_works");
        let xml = suite(&[
            testcase("proj-b", "tests::it_works", ""),
            testcase("proj-c", "tests::it_works", "<failure/>"),
        ]);
        let sources = [
            ("crates/a/src/lib.rs", source.as_str()),
            ("crates/c/src/lib.rs", &source),
        ];
        // Without package names, the crate named like the directory wins
        // only among several; a single other crate is taken.
        assert_eq!(
            verdicts(&evaluate_files(&sources, &xml, CONFIG)),
            [
                (Some("tests::it_works"), Verdict::Failed, 2),
                (Some("tests::it_works"), Verdict::Failed, 2),
            ]
        );
        let mut packages = Packages::default();
        packages.add("crates/a", "[package]\nname = \"proj-a\"\n");
        packages.add("crates/c", "[package]\nname = \"proj-c\"\n");
        packages.add("crates/x", "[workspace]\n");
        assert_eq!(
            verdicts(&evaluate_with(&sources, &xml, CONFIG, &packages)),
            [
                (Some("tests::it_works"), Verdict::NotExecuted, 0),
                (Some("tests::it_works"), Verdict::Failed, 1),
            ]
        );
    }

    #[test]
    fn gtest_results_with_a_file_must_name_the_tag_file() {
        let source = "// [utest->dsn~a~1]\nTEST(Utils, Parses) {}\n";
        let case = |file: &str, body: &str| {
            format!(
                "<testcase classname=\"Utils\" name=\"Parses\" file=\"{file}\">{body}</testcase>\n"
            )
        };
        let xml = suite(&[
            case("/ci/build/proj/app/tests/utils.cpp", ""),
            case("C:\\proj\\lib\\tests\\utils.cpp", "<failure/>"),
        ]);
        let evaluation = evaluate_files(
            &[
                ("app/tests/utils.cpp", source),
                ("lib/tests/utils.cpp", source),
                ("x/utils.cpp", source),
            ],
            &xml,
            CONFIG,
        );
        assert_eq!(
            verdicts(&evaluation),
            [
                (Some("Utils.Parses"), Verdict::Passed, 1),
                (Some("Utils.Parses"), Verdict::Failed, 1),
                // Both results belong to the other tagged files.
                (Some("Utils.Parses"), Verdict::NotExecuted, 0),
            ]
        );
    }

    #[test]
    fn the_library_takes_no_binary_results() {
        let source = rust_test("#[test]", "works");
        let xml = suite(&[
            testcase("c", "tests::works", ""),
            testcase("c::bin/a", "tests::works", "<failure/>"),
            testcase("c", "m::tests::works", ""),
            testcase("c::bin/a", "m::tests::works", "<failure/>"),
            testcase("c::bin/c", "m::tests::works", ""),
        ]);
        let mut packages = Packages::default();
        packages.add("c", "[package]\nname = \"c\"\n");
        let sources = [("c/src/lib.rs", source.as_str()), ("c/src/m.rs", &source)];
        assert_eq!(
            verdicts(&evaluate_with(&sources, &xml, CONFIG, &packages)),
            [
                (Some("tests::works"), Verdict::Passed, 1),
                // The library or the package's binary, not src/bin/a.rs.
                (Some("m::tests::works"), Verdict::Passed, 2),
            ]
        );
    }

    #[test]
    fn nextest_names_hold_the_full_path() {
        // In the root package motor, src/motor.rs is module motor; the
        // library's own tests::works is a different test.
        let source = rust_test("#[test]", "works");
        let xml = suite(&[
            testcase("motor", "tests::works", "<failure/>"),
            testcase("motor", "motor::tests::works", ""),
        ]);
        let evaluation = evaluate_files(&[("src/motor.rs", &source)], &xml, CONFIG);
        assert_eq!(
            verdicts(&evaluation),
            [(Some("motor::tests::works"), Verdict::Passed, 1)]
        );
    }

    #[test]
    fn rust_paths_are_read_within_their_project() {
        // A workspace member in a directory named `src`: its integration
        // test tests/integ.rs is the test target integ of package plat.
        let source = rust_test("#[test]", "works");
        let xml = suite(&[testcase("plat::integ", "tests::works", "<failure/>")]);
        let sources = [("src/plat/tests/integ.rs", source.as_str())];
        let mut packages = Packages::default();
        packages.add("src/plat", "[package]\nname = \"plat\"\n");
        // Without the project, the first `src` looks like a crate root.
        let without = evaluate_with(&sources, &xml, CONFIG, &packages);
        assert_eq!(
            verdicts(&without),
            [(
                Some("plat::tests::integ::tests::works"),
                Verdict::NotExecuted,
                0
            )]
        );
        packages.add_project("src/plat");
        assert_eq!(
            verdicts(&evaluate_with(&sources, &xml, CONFIG, &packages)),
            [(Some("tests::works"), Verdict::Failed, 1)]
        );
    }

    #[test]
    fn gtest_results_go_to_the_most_precise_file() {
        let source = "// [utest->dsn~a~1]\nTEST(Utils, Parses) {}\n";
        let case = |file: &str, body: &str| {
            format!(
                "<testcase classname=\"Utils\" name=\"Parses\" file=\"{file}\">{body}</testcase>\n"
            )
        };
        let xml = suite(&[
            case("/ci/proj/app/tests/utils.cpp", "<failure/>"),
            case("/ci/proj/tests/utils.cpp", ""),
        ]);
        let evaluation = evaluate_files(
            &[("tests/utils.cpp", source), ("app/tests/utils.cpp", source)],
            &xml,
            CONFIG,
        );
        assert_eq!(
            verdicts(&evaluation),
            [
                (Some("Utils.Parses"), Verdict::Passed, 1),
                (Some("Utils.Parses"), Verdict::Failed, 1),
            ]
        );
    }

    #[test]
    fn matches_libtest_junit_output() {
        let source = rust_test("#[test]", "works");
        let xml = suite(&[
            testcase("tests", "works", ""),
            testcase("m::tests", "works", "<failure/>"),
        ]);
        let evaluation = evaluate_files(
            &[("c/src/lib.rs", &source), ("c/src/m.rs", &source)],
            &xml,
            CONFIG,
        );
        assert_eq!(
            verdicts(&evaluation),
            [
                (Some("tests::works"), Verdict::Passed, 1),
                (Some("m::tests::works"), Verdict::Failed, 1),
            ]
        );
    }

    #[test]
    fn gtest_results_of_another_file_never_count() {
        let source = "// [utest->dsn~a~1]\nTEST(Utils, Parses) {}\n";
        let xml = suite(&["<testcase classname=\"Utils\" name=\"Parses\" \
             file=\"/ci/proj/app/tests/utils.cpp\"><failure/></testcase>\n"
            .to_owned()]);
        let evaluation = evaluate_files(
            &[("tests/utils.cpp", source), ("app/tests/utils.cpp", source)],
            &xml,
            CONFIG,
        );
        assert_eq!(
            verdicts(&evaluation),
            [
                (Some("Utils.Parses"), Verdict::NotExecuted, 0),
                (Some("Utils.Parses"), Verdict::Failed, 1),
            ]
        );
    }

    #[test]
    fn results_of_untagged_files_do_not_join_those_of_the_tag_file() {
        let source = "// [utest->dsn~a~1]\nTEST(Utils, Parses) {}\n";
        let case = |file: &str, body: &str| {
            format!(
                "<testcase classname=\"Utils\" name=\"Parses\" file=\"{file}\">{body}</testcase>\n"
            )
        };
        let xml = suite(&[
            case("/ci/tests/utils.cpp", ""),
            case("/ci/other/utils_test.cpp", "<failure/>"),
        ]);
        let evaluation = evaluate_files(&[("tests/utils.cpp", source)], &xml, CONFIG);
        assert_eq!(
            verdicts(&evaluation),
            [(Some("Utils.Parses"), Verdict::Passed, 1)]
        );
    }

    #[test]
    fn nextest_crate_root_tests_are_no_module_tests() {
        let source = "// [utest->dsn~a~1]\n#[test]\nfn works() {}\n";
        let xml = "<testsuites><testsuite name=\"parser\">\
            <testcase classname=\"parser\" name=\"works\"><failure/></testcase>\
            <testcase classname=\"parser\" name=\"parser::works\"/>\
            </testsuite></testsuites>";
        let evaluation = evaluate_files(
            &[("p/src/lib.rs", source), ("p/src/parser.rs", source)],
            xml,
            CONFIG,
        );
        assert_eq!(
            verdicts(&evaluation),
            [
                (Some("works"), Verdict::Failed, 1),
                (Some("parser::works"), Verdict::Passed, 1),
            ]
        );
    }

    #[test]
    fn matches_libtest_output_for_a_module_named_test() {
        let source = "mod test {\n// [utest->dsn~a~1]\n#[test]\nfn works() {}\n}\n";
        let xml = "<testsuites><testsuite name=\"test\" package=\"test\">\
            <testcase classname=\"test\" name=\"works\"/>\
            </testsuite></testsuites>";
        // nextest for a package named test: no package attribute, so the
        // crate-root test is no test of module test.
        let nextest = "<testsuites><testsuite name=\"test\">\
            <testcase classname=\"test\" name=\"works\"><failure/></testcase>\
            </testsuite></testsuites>";
        let evaluation = evaluate_files(
            &[(
                "test/src/test.rs",
                "// [utest->dsn~a~1]\n#[test]\nfn works() {}\n",
            )],
            nextest,
            CONFIG,
        );
        assert_eq!(
            verdicts(&evaluation),
            [(Some("test::works"), Verdict::NotExecuted, 0)]
        );
        let evaluation = evaluate_files(&[("c/src/lib.rs", source)], xml, CONFIG);
        assert_eq!(
            verdicts(&evaluation),
            [(Some("test::works"), Verdict::Passed, 1)]
        );
    }

    #[test]
    fn reads_nextest_binary_ids() {
        assert_eq!(nextest_binary("shall-core"), Some(("shall-core", None)));
        assert_eq!(nextest_binary("a::bin/b"), Some(("a", Some("bin/b"))));
        assert_eq!(nextest_binary("a::cli"), Some(("a", Some("cli"))));
        assert_eq!(nextest_binary("a.b"), None);
        assert_eq!(nextest_binary("a::b::c"), None);
    }
}
