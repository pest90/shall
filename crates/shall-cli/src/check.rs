//! `shall check`: read the project, trace it and report its defects.
//!
//! The files come from `[sources]` in `shall.toml`; patterns are globs
//! relative to the project root, where `*` stays within a directory and
//! `**` spans directories. Symbolic links are followed; a file selected
//! through several paths is read once. Files matched by `.gitignore` are skipped:
//! in a Git work tree with the `.gitignore` files up to its top, outside
//! of one with those of the project; global excludes and
//! `.git/info/exclude` do not count, so that the result does not depend
//! on the machine. JUnit patterns that start with a directory, such as
//! `build/**/*.xml`, also find ignored files below it, since test results
//! usually are in an ignored build directory.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context;
use clap::ValueEnum;
use globset::{GlobBuilder, GlobMatcher};
use shall_core::config::{self, Config};
use shall_core::results::{self, Packages};
use shall_core::{Diagnostic, Item, Location, Origin, Severity, junit, markdown, tags, trace};

use crate::project::{find_root, is_markdown};

/// When `shall check` fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Mode {
    /// Never fail because of defects, only report them.
    Report,
    /// Fail on every defect, errors and warnings.
    Strict,
}

/// Arguments of `shall check`.
pub struct CheckArgs {
    pub path: PathBuf,
    pub mode: Mode,
}

/// Exit code when defects fail the check.
const DEFECTS: u8 = 1;

/// Runs the check, prints the defects to stdout and a summary to stderr.
pub fn run(args: &CheckArgs) -> anyhow::Result<ExitCode> {
    let report = check(&args.path)?;
    match print(&report) {
        // The reader went away, e.g. `shall check | head`; stop quietly.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
        result => {
            result?;
            eprintln!("{}", report.summary());
        }
    }
    if args.mode == Mode::Strict && !report.diagnostics.is_empty() {
        Ok(ExitCode::from(DEFECTS))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn print(report: &Report) -> io::Result<()> {
    let mut out = io::stdout().lock();
    for diagnostic in &report.diagnostics {
        writeln!(out, "{}", line(diagnostic))?;
    }
    out.flush()
}

/// A diagnostic as printed: `path:line: message`, warnings with
/// `warning: ` before the message.
fn line(diagnostic: &Diagnostic) -> String {
    match diagnostic.severity {
        Severity::Error => diagnostic.to_string(),
        Severity::Warning => format!("{}: warning: {}", diagnostic.location, diagnostic.message),
    }
}

/// What a check found.
#[derive(Debug)]
pub struct Report {
    /// All diagnostics, sorted by path and line.
    pub diagnostics: Vec<Diagnostic>,
    requirements: usize,
    tags: usize,
    results: Option<usize>,
}

impl Report {
    fn count(&self, severity: Severity) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == severity)
            .count()
    }

    /// One line such as `not ok: 4 errors, 1 warning (8 requirements, 7
    /// tags, 3 test results)`; a tag with several targets counts once.
    fn summary(&self) -> String {
        let errors = self.count(Severity::Error);
        let warnings = self.count(Severity::Warning);
        let verdict = if errors > 0 { "not ok" } else { "ok" };
        let mut read = vec![
            plural(self.requirements, "requirement"),
            plural(self.tags, "tag"),
        ];
        read.extend(self.results.map(|results| plural(results, "test result")));
        format!(
            "{verdict}: {}, {} ({})",
            plural(errors, "error"),
            plural(warnings, "warning"),
            read.join(", ")
        )
    }
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Reads and traces the project at or above `start`.
pub fn check(start: &Path) -> anyhow::Result<Report> {
    let root = find_root(start)?;
    let config_path = root.join(config::FILE_NAME);
    let config_text = fs::read_to_string(&config_path)
        .with_context(|| format!("cannot read {}", config_path.display()))?;
    let config = Config::parse(&config_text)?;
    let mut diagnostics = Vec::new();
    let sources = Sources { text: &config_text };

    for key in config.sources.invalid_keys() {
        diagnostics.push(Diagnostic::warning(
            sources.location(key),
            format!("[sources] key '{key}' is not a list of patterns and is ignored"),
        ));
    }
    let code = config.sources.code();
    if !config.levels.is_empty() {
        let known: HashSet<&str> = config
            .levels
            .iter()
            .flat_map(|level| std::iter::once(&level.typ).chain(&level.needs))
            .chain(&config.verification.types)
            .map(|typ| typ.as_str())
            .collect();
        for key in code.keys().filter(|key| !known.contains(*key)) {
            diagnostics.push(Diagnostic::warning(
                sources.location(key),
                format!(
                    "[sources] key '{key}' is not a known type; \
                     the type of an item comes from its tag"
                ),
            ));
        }
    }

    let files = project_files(&root, &mut diagnostics);
    let requirement_files = match &config.sources.requirements {
        Some(patterns) => sources.select(&files, "requirements", patterns, &mut diagnostics),
        None => files
            .iter()
            .filter(|file| is_markdown(Path::new(file)))
            .cloned()
            .collect(),
    };
    let requirement_files = unique(&root, requirement_files);
    let mut code_files = BTreeSet::new();
    for (key, patterns) in &code {
        code_files.extend(sources.select(&files, key, patterns, &mut diagnostics));
    }
    let code_files = unique(&root, code_files);

    let mut items: Vec<Item> = Vec::new();
    for file in &requirement_files {
        if let Some(text) = read(&root, file, &mut diagnostics)? {
            let parsed = markdown::parse(Path::new(file), &text)?;
            items.extend(parsed.items);
            diagnostics.extend(parsed.diagnostics);
        }
    }
    let mut tags = BTreeSet::new();
    for file in &code_files {
        if let Some(text) = read(&root, file, &mut diagnostics)? {
            let scanned = tags::scan(Path::new(file), &text)?;
            for item in &scanned.items {
                if let Origin::Tag { text, .. } = &item.origin {
                    tags.insert((item.location.clone(), text.clone()));
                }
            }
            items.extend(scanned.items);
            diagnostics.extend(scanned.diagnostics);
        }
    }
    let trace = trace::trace(&items, &config);
    diagnostics.extend(trace.defects);

    let mut result_count = None;
    if !config.sources.junit.is_empty() {
        let candidates = junit_files(&root, &config.sources.junit, &files, &mut diagnostics);
        let junit_files = unique(
            &root,
            sources.select(
                &candidates,
                "junit",
                &config.sources.junit,
                &mut diagnostics,
            ),
        );
        let mut test_results = Vec::new();
        for file in &junit_files {
            if let Some(text) = read(&root, file, &mut diagnostics)? {
                let parsed = junit::parse(Path::new(file), &text)?;
                test_results.extend(parsed.results);
                diagnostics.extend(parsed.diagnostics);
            }
        }
        let packages = packages(&root, &code_files, &files);
        let evaluation = results::evaluate(&items, &test_results, &config, &packages);
        diagnostics.extend(evaluation.defects);
        result_count = Some(test_results.len());
    }

    diagnostics.sort_by(|a, b| {
        (a.location.path(), a.location.line()).cmp(&(b.location.path(), b.location.line()))
    });
    let requirements = items
        .iter()
        .filter(|item| item.origin == Origin::Requirement)
        .count();
    Ok(Report {
        diagnostics,
        requirements,
        tags: tags.len(),
        results: result_count,
    })
}

/// Package names of the crates that contain the Rust files among
/// `code_files`, from the closest `Cargo.toml` above each file.
fn packages(root: &Path, code_files: &BTreeSet<String>, files: &[String]) -> Packages {
    let manifests: HashSet<&str> = files
        .iter()
        .filter_map(|file| file.strip_suffix("Cargo.toml"))
        .filter(|dir| dir.is_empty() || dir.ends_with('/'))
        .map(|dir| dir.trim_end_matches('/'))
        .collect();
    let mut packages = Packages::default();
    let mut done = HashSet::new();
    for file in code_files.iter().filter(|file| file.ends_with(".rs")) {
        let Some(dir) = manifest_dir(file, &manifests) else {
            continue;
        };
        if done.insert(dir) {
            let manifest = root.join(dir).join("Cargo.toml");
            if let Ok(text) = fs::read_to_string(manifest) {
                packages.add(dir, &text);
            }
        }
    }
    packages
}

/// The closest directory above `file` with a `Cargo.toml`.
fn manifest_dir<'a>(file: &'a str, manifests: &HashSet<&str>) -> Option<&'a str> {
    let mut dir = file;
    while let Some((parent, _)) = dir.rsplit_once('/') {
        if manifests.contains(parent) {
            return Some(parent);
        }
        dir = parent;
    }
    manifests.contains("").then_some("")
}

/// The `[sources]` table of `shall.toml`, for patterns and locations.
struct Sources<'a> {
    text: &'a str,
}

impl Sources<'_> {
    /// Location of a key of `[sources]` in `shall.toml`: its line, the
    /// line of a table `[sources.<key>]`, the `[sources]` table itself, or
    /// the first line.
    fn location(&self, key: &str) -> Location {
        let mut table = None;
        let mut found = None;
        let mut current = String::new();
        let dotted = format!("sources.{key}");
        for (index, line) in self.text.lines().enumerate() {
            let line = line.split_once('#').map_or(line, |(code, _)| code).trim();
            if line.starts_with('[') {
                current = line.trim_matches(['[', ']']).trim().to_owned();
                if current == "sources" {
                    table.get_or_insert(index + 1);
                } else if current == dotted {
                    found = Some(index + 1);
                    break;
                }
            } else if (current == "sources" && is_key(line, key))
                || (current.is_empty() && is_key(line, &dotted))
            {
                found = Some(index + 1);
                break;
            }
        }
        let line = found.or(table).unwrap_or(1);
        Location::new(
            Path::new(config::FILE_NAME),
            u32::try_from(line).unwrap_or(1),
        )
        .expect("the file name is a valid location")
    }

    /// The files matching any of the patterns of `key`. A pattern that
    /// matches no file gets a warning; an invalid one is an error and is
    /// skipped.
    fn select(
        &self,
        files: &[String],
        key: &str,
        patterns: &[impl AsRef<str>],
        diagnostics: &mut Vec<Diagnostic>,
    ) -> BTreeSet<String> {
        let mut selected = BTreeSet::new();
        for pattern in patterns {
            let pattern = pattern.as_ref();
            let matcher = match glob(pattern) {
                Ok(matcher) => matcher,
                Err(error) => {
                    diagnostics.push(Diagnostic::error(
                        self.location(key),
                        format!(
                            "[sources] invalid pattern '{pattern}' of '{key}': {}",
                            error.kind()
                        ),
                    ));
                    continue;
                }
            };
            let mut matched = false;
            for file in files.iter().filter(|file| matcher.is_match(file.as_str())) {
                matched = true;
                selected.insert(file.clone());
            }
            if !matched {
                diagnostics.push(Diagnostic::warning(
                    self.location(key),
                    format!("[sources] pattern '{pattern}' of '{key}' matches no file"),
                ));
            }
        }
        selected
    }
}

/// Whether a TOML line assigns `key`, also as a quoted key.
fn is_key(line: &str, key: &str) -> bool {
    [key.to_owned(), format!("\"{key}\""), format!("'{key}'")]
        .iter()
        .any(|name| {
            line.strip_prefix(name.as_str())
                .is_some_and(|rest| rest.trim_start().starts_with('='))
        })
}

fn glob(pattern: &str) -> Result<GlobMatcher, globset::Error> {
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    Ok(GlobBuilder::new(pattern)
        .literal_separator(true)
        // The same escapes on every platform.
        .backslash_escape(true)
        .build()?
        .compile_matcher())
}

/// A walk over `dir` that follows symbolic links and skips `.git`.
fn walker(dir: &Path) -> ignore::WalkBuilder {
    let mut builder = ignore::WalkBuilder::new(dir);
    builder
        .follow_links(true)
        .filter_entry(|entry| entry.file_name() != ".git");
    builder
}

/// All files of the project, relative to the root with `/` separators and
/// sorted, without those matched by `.gitignore` files and without `.git`.
fn project_files(root: &Path, diagnostics: &mut Vec<Diagnostic>) -> Vec<String> {
    // In a Git work tree, Git's own rules: .gitignore files up to the top
    // of the work tree. Outside, only those of the project itself.
    let in_work_tree = root.ancestors().any(|dir| dir.join(".git").exists());
    let mut walker = walker(root);
    walker
        .hidden(false)
        .ignore(false)
        .git_global(false)
        .git_exclude(false);
    if !in_work_tree {
        walker.require_git(false).parents(false);
    }
    // The walk reports a broken link before applying the ignore rules, so
    // match it against the .gitignore files on its way up: up to the top
    // of the work tree, or to the project root outside of one.
    let top = if in_work_tree {
        root.ancestors()
            .find(|dir| dir.join(".git").exists())
            .unwrap_or(root)
    } else {
        root
    };
    let ignored = |link: &Path| {
        for dir in link.ancestors().skip(1) {
            let (gitignore, _) = ignore::gitignore::Gitignore::new(dir.join(".gitignore"));
            let matched = gitignore.matched_path_or_any_parents(link, false);
            if matched.is_ignore() {
                return true;
            }
            // In a work tree, a nested repository has rules of its own, as
            // for the walk.
            if matched.is_whitelist() || dir == top || (in_work_tree && dir.join(".git").exists()) {
                return false;
            }
        }
        false
    };
    collect(root, walker.build(), &ignored, diagnostics)
}

/// Candidate JUnit files. Patterns that start with a directory
/// (`build/` for `build/**/*.xml`) take everything below it, including
/// files ignored by Git; other patterns take the project files.
fn junit_files(
    root: &Path,
    patterns: &[String],
    project_files: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<String> {
    let mut files = BTreeSet::new();
    for pattern in patterns {
        let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
        let parts: Vec<&str> = pattern.split('/').collect();
        let fixed: Vec<&str> = parts
            .iter()
            .copied()
            .take_while(|part| !part.contains(['*', '?', '[', '{', '\\']))
            .collect();
        // The last part is a file name unless the pattern goes on.
        let fixed = if fixed.len() == parts.len() {
            &fixed[..fixed.len() - 1]
        } else {
            &fixed[..]
        };
        // Patterns are relative to the root and cannot leave it.
        if fixed.is_empty() || fixed.contains(&"..") {
            files.extend(project_files.iter().cloned());
            continue;
        }
        let base = fixed
            .iter()
            .fold(root.to_path_buf(), |path, part| path.join(part));
        if !base.is_dir() {
            continue;
        }
        let walk = walker(&base).standard_filters(false).build();
        files.extend(collect(root, walk, &|_| false, diagnostics));
    }
    files.into_iter().collect()
}

/// The files of a walk, relative to the root and sorted, including
/// broken symbolic links that are not `ignored`, which [`read`] reports
/// if they are selected.
/// Paths that cannot be walked are errors, since their files are missing;
/// a link loop is a warning.
fn collect(
    root: &Path,
    walk: ignore::Walk,
    ignored: &dyn Fn(&Path) -> bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    for entry in walk {
        let path = match entry {
            Ok(entry) if entry.file_type().is_some_and(|kind| kind.is_file()) => entry.into_path(),
            Ok(_) => continue,
            Err(error) => match broken_link(&error) {
                Some(path) if ignored(path) => continue,
                Some(path) => path.to_path_buf(),
                None => {
                    walk_error(root, &error, diagnostics);
                    continue;
                }
            },
        };
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let parts: Option<Vec<&str>> = relative
            .components()
            .map(|component| component.as_os_str().to_str())
            .collect();
        match parts {
            Some(parts) => files.push(parts.join("/")),
            None => diagnostics.push(Diagnostic::warning(
                location_of(root, &path),
                "the path is not valid UTF-8; skipped",
            )),
        }
    }
    files.sort();
    files.dedup();
    files
}

/// The selected files without those reached again through symbolic or
/// hard links: of several paths to one file, the first that is the file's
/// own path (no symbolic link on the way), otherwise the first.
fn unique(root: &Path, files: BTreeSet<String>) -> BTreeSet<String> {
    // The path kept per file, and whether it is the file's own path, once
    // that was needed: only for a file with several paths.
    let mut by_identity: HashMap<FileIdentity, (String, Option<bool>)> = HashMap::new();
    let mut unique = BTreeSet::new();
    let own = |file: &str| {
        let path = root.join(file);
        dunce::canonicalize(&path).is_ok_and(|real| real == path)
    };
    for file in files {
        let Some(identity) = identity(&root.join(&file)) else {
            unique.insert(file);
            continue;
        };
        match by_identity.get_mut(&identity) {
            None => {
                by_identity.insert(identity, (file, None));
            }
            Some((kept, kept_own)) => {
                if !*kept_own.get_or_insert_with(|| own(kept)) && own(&file) {
                    *kept = file;
                    *kept_own = Some(true);
                }
            }
        }
    }
    unique.extend(by_identity.into_values().map(|(file, _)| file));
    unique
}

#[cfg(unix)]
type FileIdentity = (u64, u64);
#[cfg(not(unix))]
type FileIdentity = PathBuf;

/// What makes a file the same file through any path: device and inode,
/// one `stat` per file; elsewhere its canonical path.
#[cfg(unix)]
fn identity(path: &Path) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn identity(path: &Path) -> Option<FileIdentity> {
    dunce::canonicalize(path).ok()
}

/// The path of a symbolic link whose target does not exist.
fn broken_link(error: &ignore::Error) -> Option<&Path> {
    fn not_found(error: &ignore::Error) -> bool {
        match error {
            ignore::Error::Io(io) => io.kind() == io::ErrorKind::NotFound,
            ignore::Error::WithPath { err, .. }
            | ignore::Error::WithDepth { err, .. }
            | ignore::Error::WithLineNumber { err, .. } => not_found(err),
            _ => false,
        }
    }
    fn path(error: &ignore::Error) -> Option<&Path> {
        match error {
            ignore::Error::WithPath { path, .. } => Some(path),
            ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
                path(err)
            }
            _ => None,
        }
    }
    let path = path(error)?;
    (not_found(error) && path.is_symlink()).then_some(path)
}

/// Reports a path that the walk could not read.
fn walk_error(root: &Path, error: &ignore::Error, diagnostics: &mut Vec<Diagnostic>) {
    fn path(error: &ignore::Error) -> Option<&Path> {
        match error {
            ignore::Error::WithPath { path, .. } => Some(path),
            ignore::Error::Loop { child, .. } => Some(child),
            ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
                path(err)
            }
            ignore::Error::Partial(errors) => errors.iter().find_map(path),
            _ => None,
        }
    }
    /// The innermost error, without the path that the location names.
    fn cause(error: &ignore::Error) -> &ignore::Error {
        match error {
            ignore::Error::WithPath { err, .. }
            | ignore::Error::WithDepth { err, .. }
            | ignore::Error::WithLineNumber { err, .. } => cause(err),
            _ => error,
        }
    }
    let location = path(error).map_or_else(
        || Location::new(Path::new(config::FILE_NAME), 1).expect("valid location"),
        |path| location_of(root, path),
    );
    match cause(error) {
        ignore::Error::Loop { .. } => {
            diagnostics.push(Diagnostic::warning(location, "symbolic link loop; skipped"));
        }
        // walkdir's I/O errors repeat the absolute path.
        ignore::Error::Io(io) => diagnostics.push(Diagnostic::error(
            location,
            format!("cannot read: {}", io.kind()),
        )),
        cause => diagnostics.push(Diagnostic::error(location, format!("cannot read: {cause}"))),
    }
}

/// Location of a path below the root, with lossy `/` separators.
fn location_of(root: &Path, path: &Path) -> Location {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let lossy = relative.to_string_lossy().replace('\\', "/");
    Location::new(Path::new(&lossy), 1)
        .or_else(|_| Location::new(Path::new(config::FILE_NAME), 1))
        .expect("valid location")
}

/// Reads a project file as UTF-8; an unreadable file is an error, since
/// its contents are missing, and is skipped.
fn read(
    root: &Path,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> anyhow::Result<Option<String>> {
    let location = Location::new(Path::new(file), 1)?;
    let path = root.join(file);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound && path.is_symlink() => {
            diagnostics.push(Diagnostic::warning(
                location,
                "broken symbolic link; skipped",
            ));
            return Ok(None);
        }
        Err(error) => {
            diagnostics.push(Diagnostic::error(
                location,
                format!("cannot read the file: {error}"),
            ));
            return Ok(None);
        }
    };
    match String::from_utf8(bytes) {
        Ok(text) => Ok(Some(text)),
        Err(_) => {
            diagnostics.push(Diagnostic::error(location, "not UTF-8, skipped"));
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_keys_of_the_sources_table() {
        let sources = Sources {
            text: "[[level]]\nimpl = 1\n[sources] # code\n# impl\nutest = []\n\"impl\" = [] # x\n[sources.x]\n",
        };
        assert_eq!(sources.location("impl").line(), 6);
        assert_eq!(sources.location("utest").line(), 5);
        assert_eq!(sources.location("x").line(), 7);
        assert_eq!(sources.location("missing").line(), 3);
        assert_eq!(Sources { text: "" }.location("x").line(), 1);
        let dotted = Sources {
            text: "sources.impl = []\n[sources]\njunit = []\n",
        };
        assert_eq!(dotted.location("impl").line(), 1);
        assert_eq!(dotted.location("junit").line(), 3);
    }

    #[test]
    fn finds_the_closest_manifest() {
        let manifests: HashSet<&str> = ["", "crates/a"].into_iter().collect();
        assert_eq!(
            manifest_dir("crates/a/src/lib.rs", &manifests),
            Some("crates/a")
        );
        assert_eq!(manifest_dir("crates/b/src/lib.rs", &manifests), Some(""));
        assert_eq!(manifest_dir("build.rs", &manifests), Some(""));
        assert_eq!(manifest_dir("x/y.rs", &HashSet::new()), None);
    }

    #[test]
    fn globs_keep_directories_apart() {
        let matcher = glob("src/*.cpp").unwrap();
        assert!(matcher.is_match("src/a.cpp"));
        assert!(!matcher.is_match("src/sub/a.cpp"));
        let matcher = glob("./src/**/*.{cpp,hpp}").unwrap();
        assert!(matcher.is_match("src/a.hpp"));
        assert!(matcher.is_match("src/sub/a.cpp"));
        assert!(glob("src/[").is_err());
    }
}
