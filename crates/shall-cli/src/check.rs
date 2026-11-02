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

use std::rc::Rc;

use anyhow::{Context, bail};
use globset::{GlobBuilder, GlobMatcher};
use shall_core::config::Mode;
use shall_core::config::{self, Config};
use shall_core::gate::{self, Baseline};
use shall_core::junit::{self, TestResult};
use shall_core::results::{self, Evaluation, Packages};
use shall_core::trace::{self, Trace};
use shall_core::{Diagnostic, Item, Location, Origin, Severity, markdown, tags, variant};

use crate::output::{self, Format};
use crate::project::{find_root, is_markdown};

/// Arguments of `shall check`.
pub struct CheckArgs {
    pub path: PathBuf,
    /// Replaces the modes of `shall.toml`.
    pub mode: Option<Mode>,
    /// The `trace.json` of an earlier run, for mode `new`.
    pub compare_to: Option<PathBuf>,
    /// How the defects are printed.
    pub format: Format,
    pub variant: Option<String>,
    pub workspace: bool,
}

/// Exit code when defects fail the check.
const DEFECTS: u8 = 1;

/// Runs the check, prints the defects to stdout and a summary to stderr.
/// In a workspace root, or with `--workspace`, checks every member.
pub fn run(args: &CheckArgs) -> anyhow::Result<ExitCode> {
    let root = find_root(&args.path)?;
    let baseline = match &args.compare_to {
        Some(path) => {
            let json = fs::read_to_string(path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            let baseline = Baseline::from_trace_json(&json, args.variant.as_deref())
                .map_err(|error| anyhow::anyhow!("invalid {}: {error}", path.display()))?;
            Some(baseline)
        }
        None => None,
    };
    let workspace = args.workspace || is_workspace(&root)?;
    // The root that the paths of the diagnostics are relative to.
    let display_root = if is_workspace(&root)? {
        root.clone()
    } else {
        place(&root)?.root
    };
    let (diagnostics, mut summaries, gate) = if workspace {
        if args.variant.is_some() {
            bail!("--variant checks one project, not a workspace");
        }
        if baseline.is_some() {
            bail!("--compare-to checks one project, not a workspace");
        }
        check_workspace(&root, args.mode)?
    } else {
        let project = Reader::new(place(&root)?)
            .project(&root)?
            .trace(args.variant.as_deref())?;
        let summary = project.summary();
        let gate = project.gate(args.mode, baseline.as_ref())?;
        (project.diagnostics, vec![summary], gate)
    };
    summaries.extend(gate.verdict());
    if baseline.is_some() && !gate.modes.contains("new") {
        summaries.push("warning: --compare-to is used only in mode new".to_owned());
    }
    let prefix = match args.format {
        Format::Text => String::new(),
        Format::Github | Format::Sarif => repository_prefix(&display_root),
    };
    match print(&diagnostics, args.format, &prefix) {
        // The reader went away, e.g. `shall check | head`; stop quietly.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
        result => {
            result?;
            for summary in &summaries {
                eprintln!("{summary}");
            }
        }
    }
    if args.format == Format::Github
        && let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY").filter(|path| !path.is_empty())
    {
        let summary = output::step_summary(&diagnostics, &summaries, &prefix);
        let written = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| file.write_all(summary.as_bytes()));
        // The defects are printed; a summary that cannot be written does
        // not change the exit code of the check.
        if let Err(error) = written {
            eprintln!(
                "warning: cannot write the step summary to {}: {error}",
                Path::new(&path).display()
            );
        }
    }
    if gate.failing() > 0 {
        Ok(ExitCode::from(DEFECTS))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

/// The path of `root`, where the paths of diagnostics start, below the
/// repository: below the top of its Git work tree, or else, as in a
/// checkout without `.git`, below `$GITHUB_WORKSPACE` if it is inside;
/// empty if `root` is that top or neither exists.
fn repository_prefix(root: &Path) -> String {
    let base = root
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
        .or_else(|| {
            std::env::var_os("GITHUB_WORKSPACE")
                .filter(|path| !path.is_empty())
                .and_then(|path| dunce::canonicalize(path).ok())
                .filter(|path| root.starts_with(path))
        });
    base.and_then(|base| {
        root.strip_prefix(base)
            .ok()
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
    })
    .unwrap_or_default()
}

/// Checks every member of the workspace at or above `root`: the
/// diagnostics of all members, each once, sorted by path and line, and a
/// summary per member plus one for the workspace.
fn check_workspace(
    root: &Path,
    mode: Option<Mode>,
) -> anyhow::Result<(Vec<Diagnostic>, Vec<String>, Gate)> {
    let place = if is_workspace(root)? {
        Place {
            root: root.to_path_buf(),
            members: Some(members(root)?),
        }
    } else {
        place(root)?
    };
    let Some(members) = place.members.clone() else {
        bail!(
            "--workspace: {} is in no workspace within its Git work tree",
            root.display()
        );
    };
    let workspace_root = place.root.clone();
    let mut reader = Reader::new(place);
    let mut per_member: Vec<Vec<Diagnostic>> = Vec::new();
    let mut summaries = Vec::new();
    let mut gate = Gate::default();
    for member in &members {
        let mut project = reader.project(member)?.trace(None)?;
        let dir = member
            .strip_prefix(&workspace_root)
            .expect("members are inside the workspace")
            .to_string_lossy()
            .replace('\\', "/");
        project.member = Some(dir);
        summaries.push(project.summary());
        // Defects of a shared project fail the check of every project
        // that imports it; count them once per project.
        gate.add(project.gate(mode, None)?);
        per_member.push(project.diagnostics);
    }
    // Shared projects are traced with every project that imports them;
    // their defects count once, as often as in the member with most.
    let mut most: HashMap<String, usize> = HashMap::new();
    for project_diagnostics in &per_member {
        let mut count: HashMap<String, usize> = HashMap::new();
        for diagnostic in project_diagnostics {
            *count.entry(output::text(diagnostic)).or_default() += 1;
        }
        for (text, count) in count {
            let entry = most.entry(text).or_default();
            *entry = (*entry).max(count);
        }
    }
    let mut diagnostics: Vec<Diagnostic> = per_member.into_iter().flatten().collect();
    diagnostics.sort_by(|a, b| {
        (a.location.path(), a.location.line()).cmp(&(b.location.path(), b.location.line()))
    });
    diagnostics.retain(|diagnostic| {
        let left = most.get_mut(&output::text(diagnostic)).expect("counted");
        let keep = *left > 0;
        *left = left.saturating_sub(1);
        keep
    });
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .count();
    summaries.push(format!(
        "workspace: {}: {}, {} ({})",
        if errors > 0 { "not ok" } else { "ok" },
        plural(errors, "error"),
        plural(diagnostics.len() - errors, "warning"),
        plural(members.len(), "project")
    ));
    Ok((diagnostics, summaries, gate))
}

/// Which defects fail the check, and in which modes.
#[derive(Debug, Default)]
pub struct Gate {
    /// The defects that fail the check, as printed, with how often each
    /// is printed.
    failing: HashMap<String, usize>,
    /// The modes other than `report` that applied, sorted; none means the
    /// check cannot fail.
    modes: BTreeSet<&'static str>,
}

impl Gate {
    /// The number of defects that fail the check.
    pub fn failing(&self) -> usize {
        self.failing.values().sum()
    }

    /// The line after the summaries: whether the check failed, unless it
    /// only reports.
    fn verdict(&self) -> Option<String> {
        if self.modes.is_empty() {
            return None;
        }
        let failing = self.failing();
        let modes: Vec<&str> = self.modes.iter().copied().collect();
        let modes = format!(
            "mode{} {}",
            if modes.len() == 1 { "" } else { "s" },
            modes.join(", ")
        );
        Some(if failing == 1 {
            format!("check failed: 1 defect fails {modes}")
        } else if failing > 1 {
            format!("check failed: {failing} defects fail {modes}")
        } else {
            format!("check passed: no defect fails {modes}")
        })
    }

    /// Adds the gate of another project of a workspace; a defect of a
    /// shared project counts once, as it is printed once.
    fn add(&mut self, other: Gate) {
        for (text, count) in other.failing {
            let entry = self.failing.entry(text).or_default();
            *entry = (*entry).max(count);
        }
        self.modes.extend(other.modes);
    }
}

fn print(diagnostics: &[Diagnostic], format: Format, prefix: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    match format {
        Format::Text => {
            for diagnostic in diagnostics {
                writeln!(out, "{}", output::text(diagnostic))?;
            }
        }
        Format::Github => {
            for diagnostic in diagnostics {
                writeln!(out, "{}", output::github(diagnostic, prefix))?;
            }
        }
        Format::Sarif => out.write_all(output::sarif(diagnostics, prefix).as_bytes())?,
    }
    out.flush()
}

/// A project, read and traced.
#[derive(Debug)]
pub struct Project {
    /// The parsed `shall.toml`.
    pub config: Config,
    /// Name of the traced variant; none when all items are traced.
    pub variant: Option<String>,
    /// Directory of the project below the workspace root, when a whole
    /// workspace is checked.
    pub member: Option<String>,
    /// Items of requirement files and tags, in the order they were read;
    /// only those of the variant, if one is traced.
    pub items: Vec<Item>,
    /// Items outside the traced variant; none without a variant.
    outside: Vec<Item>,
    /// Coverage and links of `items`.
    pub trace: Trace,
    /// The test results and their evaluation; none without `junit`
    /// sources.
    pub tests: Option<(Vec<TestResult>, Evaluation)>,
    /// All diagnostics, sorted by path and line.
    pub diagnostics: Vec<Diagnostic>,
    /// Number of tags; a tag with several targets counts once.
    tags: usize,
}

impl Project {
    /// Which defects fail the check: in `mode`, or else in the modes of
    /// `shall.toml`; `baseline` holds the defects of the earlier run that
    /// mode `new` needs.
    pub fn gate(&self, mode: Option<Mode>, baseline: Option<&Baseline>) -> anyhow::Result<Gate> {
        let modes: BTreeSet<Mode> = match mode {
            Some(mode) => [mode].into(),
            None => std::iter::once(self.config.check.mode)
                .chain(self.config.check.levels.values().map(|level| level.mode))
                .collect(),
        };
        if modes.contains(&Mode::New) && baseline.is_none() {
            let which = self
                .member
                .as_ref()
                .map(|member| format!(" of {member}"))
                .unwrap_or_default();
            bail!(
                "mode new{which} needs --compare-to with the trace.json of the run to compare \
                 with, e.g. on the target branch; a workspace check cannot compare yet, so \
                 check that project alone or pass --mode"
            );
        }
        let mut all = self.items.clone();
        all.extend(self.outside.iter().cloned());
        let mut failing: HashMap<String, usize> = HashMap::new();
        for diagnostic in gate::failing(&self.diagnostics, &all, &self.config, mode, baseline) {
            *failing.entry(output::text(diagnostic)).or_default() += 1;
        }
        Ok(Gate {
            failing,
            modes: modes
                .into_iter()
                .filter(|mode| *mode != Mode::Report)
                .map(Mode::as_str)
                .collect(),
        })
    }

    fn count(&self, severity: Severity) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == severity)
            .count()
    }

    /// One line such as `not ok: 4 errors, 1 warning (8 requirements, 7
    /// tags, 3 test results)`, after `variant <name>: ` for a variant; a
    /// tag with several targets counts once.
    pub fn summary(&self) -> String {
        let errors = self.count(Severity::Error);
        let warnings = self.count(Severity::Warning);
        let verdict = if errors > 0 { "not ok" } else { "ok" };
        let requirements = self
            .items
            .iter()
            .filter(|item| item.origin == Origin::Requirement)
            .count();
        let mut read = vec![
            plural(requirements, "requirement"),
            plural(self.tags, "tag"),
        ];
        read.extend(
            self.tests
                .as_ref()
                .map(|(results, _)| plural(results.len(), "test result")),
        );
        let member = self
            .member
            .as_ref()
            .map(|dir| format!("{dir}: "))
            .unwrap_or_default();
        let variant = self
            .variant
            .as_ref()
            .map(|name| format!("variant {name}: "))
            .unwrap_or_default();
        format!(
            "{member}{variant}{verdict}: {}, {} ({})",
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

/// Reads and traces the project at or above `start`, or only the items of
/// the named variant.
pub fn analyze(start: &Path, variant: Option<&str>) -> anyhow::Result<Project> {
    read_project(start)?.trace(variant)
}

/// Where the project at `root` lives: the root that paths in diagnostics
/// are relative to and that imports must stay in, and its workspace.
pub(crate) struct Place {
    /// The workspace root if the project is a member of one, otherwise the
    /// project root.
    root: PathBuf,
    /// The member directories, if `root` is a workspace root.
    pub(crate) members: Option<Vec<PathBuf>>,
}

/// Finds the workspace above the project at `root`, if any: the closest
/// directory above, up to the top of the Git work tree, whose `shall.toml`
/// has a `[workspace]`. Such a file that cannot be read or parsed is an
/// error; other `shall.toml` files above are not read further.
pub(crate) fn place(root: &Path) -> anyhow::Result<Place> {
    let standalone = Place {
        root: root.to_path_buf(),
        members: None,
    };
    // `.git` is a directory, or a file in a submodule or linked work tree.
    if root.join(".git").exists() {
        return Ok(standalone);
    }
    for dir in root.ancestors().skip(1) {
        if workspace_of(dir)?.is_some() {
            let members = members(dir)?;
            if !members.iter().any(|member| member == root) {
                bail!(
                    "{} is not a member of the workspace {}; add it to [workspace] members",
                    root.display(),
                    dir.join(config::FILE_NAME).display()
                );
            }
            return Ok(Place {
                root: dir.to_path_buf(),
                members: Some(members),
            });
        }
        if dir.join(".git").exists() {
            break;
        }
    }
    Ok(standalone)
}

/// The `[workspace]` of the `shall.toml` in `dir`, if it has one. Only a
/// file with a `workspace` table is parsed as a configuration.
fn workspace_of(dir: &Path) -> anyhow::Result<Option<config::Workspace>> {
    let path = dir.join(config::FILE_NAME);
    if !path.is_file() {
        return Ok(None);
    }
    let text =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    // Parse only what may be a workspace; a false hit costs one parse.
    if !text.contains("workspace") {
        return Ok(None);
    }
    Ok(Config::parse(&text)
        .with_context(|| format!("in {}", path.display()))?
        .workspace)
}

/// Whether the `shall.toml` in `dir` describes a workspace.
fn is_workspace(dir: &Path) -> anyhow::Result<bool> {
    Ok(workspace_of(dir)?.is_some())
}

fn member_glob(root: &Path, pattern: &str) -> anyhow::Result<GlobMatcher> {
    glob(pattern.trim_end_matches('/')).with_context(|| {
        format!(
            "invalid member pattern '{pattern}' in {}",
            root.join(config::FILE_NAME).display()
        )
    })
}

/// The member directories of the workspace at `root`, in the order of
/// `[workspace] members` and sorted within a pattern. A pattern that
/// matches no directory with a `shall.toml` is an error, and so is a
/// member that resolves to a directory outside the workspace.
fn members(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let path = root.join(config::FILE_NAME);
    let workspace = workspace_of(root)?.expect("the root is a workspace");
    if workspace.members.is_empty() {
        bail!("{} lists no [workspace] members", path.display());
    }
    let mut ignored = Vec::new();
    let projects: Vec<String> = project_files(root, &mut ignored)
        .into_iter()
        .filter_map(|file| {
            file.strip_suffix(config::FILE_NAME)
                .filter(|dir| dir.ends_with('/'))
                .map(|dir| dir.trim_end_matches('/').to_owned())
        })
        .collect();
    let mut members = Vec::new();
    for pattern in &workspace.members {
        let matcher = member_glob(root, pattern)?;
        let matched: Vec<&String> = projects
            .iter()
            .filter(|dir| matcher.is_match(dir.as_str()))
            .collect();
        if matched.is_empty() {
            bail!(
                "member '{pattern}' of {} matches no directory with a {} \
                 (directories ignored by .gitignore do not count)",
                path.display(),
                config::FILE_NAME
            );
        }
        for dir in matched {
            let member = dunce::canonicalize(root.join(dir))
                .with_context(|| format!("cannot resolve {dir}"))?;
            if !member.starts_with(root) {
                bail!(
                    "member {dir} of {} is {}, outside the workspace",
                    path.display(),
                    member.display()
                );
            }
            if !members.contains(&member) {
                members.push(member);
            }
        }
    }
    Ok(members)
}

/// A project read, with its own items only: its files, items and the
/// diagnostics of reading them. Locations are relative to the root of the
/// [`Place`].
struct Member {
    /// Absolute directory of the project.
    root: PathBuf,
    /// Directory of the project below the root of its [`Place`], e.g.
    /// `platform`; empty for that root itself.
    prefix: String,
    config: Config,
    config_text: String,
    /// All files of the project, relative to its root and sorted, without
    /// those of projects nested in it.
    files: Vec<String>,
    /// The code and test files that were scanned for tags, relative to
    /// the project root.
    code_files: BTreeSet<String>,
    items: Vec<Item>,
    diagnostics: Vec<Diagnostic>,
}

/// Reads projects once each, also when several projects import them.
struct Reader {
    place: Place,
    read: HashMap<PathBuf, Rc<Member>>,
}

impl Reader {
    fn new(place: Place) -> Self {
        Self {
            place,
            read: HashMap::new(),
        }
    }

    /// Reads the project at the absolute, canonical `root`, or returns it
    /// if it was read before.
    fn member(&mut self, root: &Path) -> anyhow::Result<Rc<Member>> {
        if let Some(member) = self.read.get(root) {
            return Ok(Rc::clone(member));
        }
        let prefix = root
            .strip_prefix(&self.place.root)
            .expect("projects are inside the root of their place")
            .to_string_lossy()
            .replace('\\', "/");
        let member = Rc::new(read_member(root, prefix)?);
        self.read.insert(root.to_path_buf(), Rc::clone(&member));
        Ok(member)
    }

    /// Reads the project at `root` and its imports.
    fn project(&mut self, root: &Path) -> anyhow::Result<ReadProject> {
        let member = self.member(root)?;
        if member.config.workspace.is_some() {
            bail!(
                "{} describes a workspace, not a project; run this in a member",
                root.join(config::FILE_NAME).display()
            );
        }
        let mut imports: Vec<(String, Rc<Member>)> = Vec::new();
        let mut diagnostics = Vec::new();
        for import in &member.config.imports {
            let path = import.path.as_deref().expect("the configuration checks it");
            let what = format!("import '{}' of {}", import.name, member.root.display());
            let dir = dunce::canonicalize(member.root.join(path))
                .with_context(|| format!("{what}: cannot resolve {path}"))?;
            if !dir.starts_with(&self.place.root) {
                if self.place.members.is_some() {
                    bail!(
                        "{what}: {} is outside the workspace {}; imports from other \
                         repositories are planned (M8.3)",
                        dir.display(),
                        self.place.root.display()
                    );
                }
                bail!(
                    "{what}: {} is outside the project; import other projects of the \
                     repository from a [workspace] that lists both",
                    dir.display()
                );
            }
            if !dir.join(config::FILE_NAME).is_file() {
                bail!("{what}: {} has no {}", dir.display(), config::FILE_NAME);
            }
            if dir == member.root {
                bail!("{what}: a project cannot import itself");
            }
            if let Some((other, _)) = imports.iter().find(|(_, read)| read.root == dir) {
                bail!("{what}: {} is imported as '{other}' already", dir.display());
            }
            if self
                .place
                .members
                .as_ref()
                .is_some_and(|members| !members.contains(&dir))
            {
                bail!(
                    "{what}: {} is not a member of the workspace {}; add it to \
                     [workspace] members",
                    dir.display(),
                    self.place.root.join(config::FILE_NAME).display()
                );
            }
            let imported = self.member(&dir)?;
            if imported.config.workspace.is_some() {
                bail!("{what}: {} describes a workspace", dir.display());
            }
            if !imported.config.imports.is_empty() {
                let line = first_import_line(&imported.config_text).unwrap_or(1);
                let location = Location::new(Path::new(config::FILE_NAME), line)
                    .expect("valid location")
                    .prefixed(&imported.prefix);
                diagnostics.push(Diagnostic::warning(
                    location,
                    format!(
                        "the imports of '{}' are not followed; transitive imports are \
                         planned (M8.2)",
                        import.name
                    ),
                ));
            }
            imports.push((import.name.clone(), imported));
        }
        let config = member
            .config
            .with_imports(
                imports
                    .iter()
                    .map(|(name, read)| (name.as_str(), &read.config)),
            )
            .with_context(|| format!("in {}", root.join(config::FILE_NAME).display()))?;
        if !member.config.imports.is_empty() {
            let mut unknown = unknown_source_keys(&member.config, &config, &member.config_text);
            relocate(&mut unknown, &member.prefix);
            diagnostics.extend(unknown);
        }
        let mut unknown = unknown_check_settings(&member.config, &config, &member.config_text);
        relocate(&mut unknown, &member.prefix);
        diagnostics.extend(unknown);
        Ok(ReadProject {
            member,
            imports,
            config,
            diagnostics,
        })
    }
}

/// Line of the first import in a `shall.toml`: an `[[import]]` table or
/// an `import = [...]` key.
fn first_import_line(text: &str) -> Option<u32> {
    text.lines()
        .position(|line| {
            let code = line
                .split_once('#')
                .map_or(line, |(code, _)| code)
                .trim()
                .replace(' ', "");
            code == "[[import]]" || code.starts_with("import=")
        })
        .map(|index| u32::try_from(index + 1).unwrap_or(u32::MAX))
}

/// A project read with its imports, ready to be traced, once or once per
/// variant.
pub struct ReadProject {
    member: Rc<Member>,
    /// Imported projects by import name, in the order of `[[import]]`.
    imports: Vec<(String, Rc<Member>)>,
    /// The project's configuration with the levels of its imports.
    config: Config,
    /// Diagnostics of resolving the imports.
    diagnostics: Vec<Diagnostic>,
}

/// Reads the project at or above `start` and its imports. A project in a
/// workspace reports paths relative to the workspace root.
pub fn read_project(start: &Path) -> anyhow::Result<ReadProject> {
    let root = find_root(start)?;
    let mut reader = Reader::new(place(&root)?);
    reader.project(&root)
}

/// Reads the configuration, the requirement files and the tags of the
/// project at `root`, with locations below `prefix`.
fn read_member(root: &Path, prefix: String) -> anyhow::Result<Member> {
    let config_path = root.join(config::FILE_NAME);
    let config_text = fs::read_to_string(&config_path)
        .with_context(|| format!("cannot read {}", config_path.display()))?;
    let config =
        Config::parse(&config_text).with_context(|| format!("in {}", config_path.display()))?;
    let mut diagnostics = Vec::new();
    if config.workspace.is_some() {
        return Ok(Member {
            root: root.to_path_buf(),
            prefix,
            config,
            config_text,
            files: Vec::new(),
            code_files: BTreeSet::new(),
            items: Vec::new(),
            diagnostics,
        });
    }
    let sources = Sources::new(&config_text, "sources");

    for key in config.sources.invalid_keys() {
        diagnostics.push(Diagnostic::warning(
            sources.location(key),
            format!("[sources] key '{key}' is not a list of patterns and is ignored"),
        ));
    }
    for (name, variant) in &config.variants {
        let table = Sources::new(&config_text, &format!("variant.{name}"));
        for key in variant.unknown_keys() {
            diagnostics.push(Diagnostic::warning(
                table.location(key),
                format!("[variant.{name}] key '{key}' is unknown and is ignored"),
            ));
        }
    }
    let code = config.sources.code();
    // With imports, the types of the imported levels count too; the
    // project checks its keys once it knows them.
    if config.imports.is_empty() {
        diagnostics.extend(unknown_source_keys(&config, &config, &config_text));
    }

    let files = without_nested_projects(project_files(root, &mut diagnostics));
    let requirement_files = match &config.sources.requirements {
        Some(patterns) => sources.select(&files, "requirements", patterns, &mut diagnostics),
        None => files
            .iter()
            .filter(|file| is_markdown(Path::new(file)))
            .cloned()
            .collect(),
    };
    let requirement_files = unique(root, requirement_files);
    let mut code_files = BTreeSet::new();
    for (key, patterns) in &code {
        code_files.extend(sources.select(&files, key, patterns, &mut diagnostics));
    }
    let code_files = unique(root, code_files);

    let mut items: Vec<Item> = Vec::new();
    for file in &requirement_files {
        if let Some(text) = read(root, file, &mut diagnostics)? {
            if text.starts_with(shall_core::oft::MARKER) {
                diagnostics.push(Diagnostic::warning(
                    location_of(root, &root.join(file)),
                    "written by shall export --oft; skipped",
                ));
                continue;
            }
            let parsed = markdown::parse(Path::new(file), &text)?;
            items.extend(parsed.items);
            diagnostics.extend(parsed.diagnostics);
        }
    }
    for file in &code_files {
        if let Some(text) = read(root, file, &mut diagnostics)? {
            let scanned = tags::scan(Path::new(file), &text)?;
            items.extend(scanned.items);
            diagnostics.extend(scanned.diagnostics);
        }
    }
    diagnostics.extend(variant::check_features(&items, &config));
    for item in &mut items {
        item.location = item.location.prefixed(&prefix);
    }
    relocate(&mut diagnostics, &prefix);
    Ok(Member {
        root: root.to_path_buf(),
        prefix,
        config,
        config_text,
        files,
        code_files,
        items,
        diagnostics,
    })
}

/// Warnings for keys of `[check]` that mean nothing, and for
/// `[check.level.<type>]` tables whose type `known`, a configuration with
/// the levels of the imports, does not know as a level, a type needed by
/// a level or a verification type: both are often typos, which would
/// leave defects in mode `report`.
fn unknown_check_settings(config: &Config, known: &Config, text: &str) -> Vec<Diagnostic> {
    let check = Sources::new(text, "check");
    let mut diagnostics: Vec<Diagnostic> = config
        .check
        .unknown_keys()
        .into_iter()
        .map(|key| {
            Diagnostic::warning(
                check.location(key),
                format!("[check] key '{key}' is unknown and is ignored"),
            )
        })
        .collect();
    let types: HashSet<&str> = known
        .levels
        .iter()
        .flat_map(|level| std::iter::once(&level.typ).chain(&level.needs))
        .chain(&known.verification.types)
        .map(|typ| typ.as_str())
        .collect();
    let levels = Sources::new(text, "check.level");
    for typ in config.check.levels.keys() {
        if !types.contains(typ.as_str()) {
            // `[check.level.x]`, or `level.x.mode = …` in `[check]`.
            let mut location = levels.location(typ.as_str());
            if location.line() == 1 {
                location = check.location("level");
            }
            diagnostics.push(Diagnostic::warning(
                location,
                format!(
                    "[check.level.{typ}] names no level, type needed by a level or \
                     verification type"
                ),
            ));
        }
    }
    diagnostics
}

/// Warnings for keys of `[sources]` that are no type known to `known`, a
/// configuration with the levels of the imports.
fn unknown_source_keys(config: &Config, known: &Config, text: &str) -> Vec<Diagnostic> {
    if known.levels.is_empty() {
        return Vec::new();
    }
    let types: HashSet<&str> = known
        .levels
        .iter()
        .flat_map(|level| std::iter::once(&level.typ).chain(&level.needs))
        .chain(&known.verification.types)
        .map(|typ| typ.as_str())
        .collect();
    let sources = Sources::new(text, "sources");
    config
        .sources
        .code()
        .into_keys()
        .filter(|key| !types.contains(key))
        .map(|key| {
            Diagnostic::warning(
                sources.location(key),
                format!(
                    "[sources] key '{key}' is not a known type; \
                     the type of an item comes from its tag"
                ),
            )
        })
        .collect()
}

/// Drops the files of projects nested in this one: those below a
/// directory with a `shall.toml` of its own.
fn without_nested_projects(files: Vec<String>) -> Vec<String> {
    let nested: Vec<String> = files
        .iter()
        .filter_map(|file| file.strip_suffix(config::FILE_NAME))
        .filter(|dir| dir.ends_with('/'))
        .map(str::to_owned)
        .collect();
    files
        .into_iter()
        .filter(|file| !nested.iter().any(|dir| file.starts_with(dir.as_str())))
        .collect()
}

/// Moves diagnostics below the directory `prefix`.
fn relocate(diagnostics: &mut [Diagnostic], prefix: &str) {
    for diagnostic in diagnostics {
        diagnostic.location = diagnostic.location.prefixed(prefix);
    }
}

impl ReadProject {
    /// The configuration, with the levels of the imports.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Traces all items of the project and its imports, or only those of
    /// the named variant, and evaluates the test results: the project's
    /// from the variant's `junit` patterns if it has any, otherwise from
    /// those of `[sources]`, and those of every import from its
    /// `[sources]`.
    pub fn trace(&self, variant: Option<&str>) -> anyhow::Result<Project> {
        let config = &self.config;
        let own = &self.member;
        let mut diagnostics = own.diagnostics.clone();
        let mut all_items = own.items.clone();
        for (name, import) in &self.imports {
            diagnostics.extend(import.diagnostics.iter().cloned());
            all_items.extend(import.items.iter().cloned().map(|mut item| {
                item.import = Some(name.clone());
                item
            }));
        }
        diagnostics.extend(self.diagnostics.iter().cloned());
        let selected = variant
            .map(|name| {
                own.config
                    .variants
                    .get_key_value(name)
                    .ok_or_else(|| unknown_variant(name, &own.config))
            })
            .transpose()?;
        let (items, outside) = match selected {
            Some((_, variant)) => {
                let selection = variant::select(&all_items, variant);
                (selection.inside, selection.outside)
            }
            None => (all_items, Vec::new()),
        };
        let trace = trace::trace_variant(&items, &outside, config);
        diagnostics.extend(trace.defects.iter().cloned());

        let own_junit = match selected {
            Some((name, variant)) if variant.junit.is_some() => (
                variant.junit.as_deref().unwrap_or_default(),
                format!("variant.{name}"),
            ),
            _ => (own.config.sources.junit.as_slice(), "sources".to_owned()),
        };
        let junit: Vec<(&Member, &[String], String)> =
            std::iter::once((own.as_ref(), own_junit.0, own_junit.1))
                .chain(self.imports.iter().map(|(_, import)| {
                    (
                        import.as_ref(),
                        import.config.sources.junit.as_slice(),
                        "sources".to_owned(),
                    )
                }))
                .filter(|(_, patterns, _)| !patterns.is_empty())
                .collect();
        let mut tests = None;
        if !junit.is_empty() {
            let mut test_results = Vec::new();
            let mut packages = Packages::default();
            for (member, patterns, table) in junit {
                let mut found = Vec::new();
                let sources = Sources::new(&member.config_text, &table);
                let candidates = junit_files(&member.root, patterns, &member.files, &mut found);
                let junit_files = unique(
                    &member.root,
                    sources.select(&candidates, "junit", patterns, &mut found),
                );
                for file in &junit_files {
                    if let Some(text) = read(&member.root, file, &mut found)? {
                        let parsed = junit::parse(Path::new(file), &text)?;
                        test_results.extend(parsed.results.into_iter().map(|mut result| {
                            result.location = result.location.prefixed(&member.prefix);
                            result.file = result.file.map(|file| in_member(&file, &member.prefix));
                            result
                        }));
                        found.extend(parsed.diagnostics);
                    }
                }
                relocate(&mut found, &member.prefix);
                diagnostics.extend(found);
            }
            for member in std::iter::once(own).chain(self.imports.iter().map(|(_, m)| m)) {
                packages.add_project(member.prefix.clone());
                add_packages(&mut packages, member);
            }
            let evaluation =
                results::evaluate_variant(&items, &outside, &test_results, config, &packages);
            diagnostics.extend(evaluation.defects.iter().cloned());
            tests = Some((test_results, evaluation));
        }

        diagnostics.sort_by(|a, b| {
            (a.location.path(), a.location.line()).cmp(&(b.location.path(), b.location.line()))
        });
        let tags: BTreeSet<(&Location, &str)> = items
            .iter()
            .filter_map(|item| match &item.origin {
                Origin::Tag { text, .. } => Some((&item.location, text.as_str())),
                Origin::Requirement => None,
            })
            .collect();
        let tags = tags.len();
        Ok(Project {
            config: config.clone(),
            variant: selected.map(|(name, _)| name.clone()),
            member: None,
            items,
            outside,
            trace,
            tests,
            diagnostics,
            tags,
        })
    }
}

/// A `file` attribute of a test result as seen from the root of the
/// [`Place`]: a relative path is relative to the project, with `.` and
/// `..` resolved as far as it stays inside the root, and an absolute one
/// stays.
fn in_member(file: &str, prefix: &str) -> String {
    let absolute = file.starts_with('/') || file.starts_with('\\') || file.get(1..2) == Some(":");
    if absolute || prefix.is_empty() {
        return file.to_owned();
    }
    let mut parts: Vec<&str> = prefix.split('/').collect();
    for part in file.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." if !parts.is_empty() => {
                parts.pop();
            }
            // Above the root: leave the attribute as written.
            ".." => return file.to_owned(),
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// The error for a variant that `shall.toml` does not define.
fn unknown_variant(name: &str, config: &Config) -> anyhow::Error {
    if config.variants.is_empty() {
        anyhow::anyhow!(
            "unknown variant '{name}': {} has no [variant.*]",
            config::FILE_NAME
        )
    } else {
        let names: Vec<&str> = config.variants.keys().map(String::as_str).collect();
        anyhow::anyhow!(
            "unknown variant '{name}'; {} defines {}",
            config::FILE_NAME,
            names.join(", ")
        )
    }
}

/// Adds the package names of the crates that contain the Rust files
/// among the member's code files, from the closest `Cargo.toml` above each
/// file, by their directory below the root of the [`Place`].
fn add_packages(packages: &mut Packages, member: &Member) {
    let manifests: HashSet<&str> = member
        .files
        .iter()
        .filter_map(|file| file.strip_suffix("Cargo.toml"))
        .filter(|dir| dir.is_empty() || dir.ends_with('/'))
        .map(|dir| dir.trim_end_matches('/'))
        .collect();
    let mut done = HashSet::new();
    for file in member
        .code_files
        .iter()
        .filter(|file| file.ends_with(".rs"))
    {
        let Some(dir) = manifest_dir(file, &manifests) else {
            continue;
        };
        if done.insert(dir) {
            let manifest = member.root.join(dir).join("Cargo.toml");
            if let Ok(text) = fs::read_to_string(manifest) {
                let dir = match (member.prefix.as_str(), dir) {
                    (prefix, "") => prefix.to_owned(),
                    ("", dir) => dir.to_owned(),
                    (prefix, dir) => format!("{prefix}/{dir}"),
                };
                packages.add(dir, &text);
            }
        }
    }
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

/// A table of `shall.toml` with patterns, such as `[sources]` or
/// `[variant.premium]`, for patterns and locations.
struct Sources<'a> {
    text: &'a str,
    /// Name of the table, e.g. `sources`.
    table: String,
}

impl<'a> Sources<'a> {
    fn new(text: &'a str, table: &str) -> Self {
        Self {
            text,
            table: table.to_owned(),
        }
    }

    /// Location of a key of the table in `shall.toml`: its line, the line
    /// of a table `[<table>.<key>]`, the table itself, or the first line.
    fn location(&self, key: &str) -> Location {
        let mut table = None;
        let mut found = None;
        let mut current = String::new();
        let dotted = format!("{}.{key}", self.table);
        for (index, line) in self.text.lines().enumerate() {
            let line = line.split_once('#').map_or(line, |(code, _)| code).trim();
            if line.starts_with('[') {
                current = line.trim_matches(['[', ']']).trim().to_owned();
                if current == self.table {
                    table.get_or_insert(index + 1);
                } else if current == dotted {
                    found = Some(index + 1);
                    break;
                }
            } else if (current == self.table && is_key(line, key))
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
                            "[{}] invalid pattern '{pattern}' of '{key}': {}",
                            self.table,
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
                    format!(
                        "[{}] pattern '{pattern}' of '{key}' matches no file",
                        self.table
                    ),
                ));
            }
        }
        selected
    }
}

/// Whether a TOML line assigns `key`, also as a quoted key or as the
/// start of a dotted key such as `key.mode = …`.
fn is_key(line: &str, key: &str) -> bool {
    [key.to_owned(), format!("\"{key}\""), format!("'{key}'")]
        .iter()
        .any(|name| {
            line.strip_prefix(name.as_str()).is_some_and(|rest| {
                let rest = rest.trim_start();
                rest.starts_with('=') || rest.starts_with('.')
            })
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
        let sources = Sources::new(
            "[[level]]\nimpl = 1\n[sources] # code\n# impl\nutest = []\n\"impl\" = [] # x\n[sources.x]\n",
            "sources",
        );
        assert_eq!(sources.location("impl").line(), 6);
        assert_eq!(sources.location("utest").line(), 5);
        assert_eq!(sources.location("x").line(), 7);
        assert_eq!(sources.location("missing").line(), 3);
        assert_eq!(Sources::new("", "sources").location("x").line(), 1);
        let dotted = Sources::new("sources.impl = []\n[sources]\njunit = []\n", "sources");
        assert_eq!(dotted.location("impl").line(), 1);
        assert_eq!(dotted.location("junit").line(), 3);
        let variant = Sources::new(
            "[sources]\njunit = []\n[variant.a]\nfeatures = []\njunit = []\n",
            "variant.a",
        );
        assert_eq!(variant.location("junit").line(), 5);
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
