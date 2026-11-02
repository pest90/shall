//! `shall diff` and `shall upgrade-check`: the requirement changes
//! between two Git states, see [`shall_core::diff`].
//!
//! A state that is a commit is written into a temporary directory from
//! the objects of the repository (`git ls-tree`, `git cat-file`), read
//! and traced like the working tree, and removed again; the working tree
//! itself is read as it is. Files are written as Git stores them, without
//! filters such as Git LFS or line-ending conversion, and the repository
//! is left untouched: no index, work tree registration or hook is
//! involved, also not inside a Git hook. A submodule is written from its
//! own repository at the commit the state records, if it is cloned.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use clap::ValueEnum;
use shall_core::diff::{self, Changes};
use shall_core::trace::{Link, LinkKind, Trace};
use shall_core::{Item, Origin, config};

use crate::check::{self, Project};
use crate::output::cell;
use crate::project::find_root;

/// How to print the changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// One line `path:line: change` per change.
    Text,
    /// Markdown tables, e.g. for a comment in a merge request.
    Markdown,
}

/// Arguments of `shall diff`.
pub struct DiffArgs {
    pub range: String,
    pub path: PathBuf,
    pub format: Format,
}

/// Prints the changes from the base to the head of `range`.
pub fn run(args: &DiffArgs) -> anyhow::Result<()> {
    let (root, top, relative) = locate(&args.path)?;
    let (base, head) = range(&top, &args.range)?;
    let base_tree = Tree::write(&top, &base)?;
    let base_project = base_tree.read(&relative)?;
    let (head_project, head_name) = match &head {
        Some(head) => {
            let tree = Tree::write(&top, head)?;
            (tree.read(&relative)?, short(head))
        }
        None => (
            Some(check::analyze(&root, None)?),
            "the working tree".to_owned(),
        ),
    };
    let empty = (Vec::new(), Trace::default());
    let changes = diff::changes(state(&base_project, &empty), state(&head_project, &empty));
    let head_items = head_project
        .as_ref()
        .map_or(&[][..], |p| p.items.as_slice());
    let text = match args.format {
        Format::Text => text(&changes, head_items),
        Format::Markdown => markdown(&changes, head_items, &short(&base), &head_name),
    };
    print!("{text}");
    eprintln!(
        "{} (base {}, head {head_name})",
        summary(&changes),
        short(&base)
    );
    Ok(())
}

/// Arguments of `shall upgrade-check`.
pub struct UpgradeArgs {
    pub to: String,
    pub path: PathBuf,
    /// An external component to move to `to` instead of the whole
    /// repository.
    pub import: Option<String>,
}

/// Prints the valid links of the working tree that merging the commit
/// `to` would make stale: whose targets `to` changed since its merge base
/// with `HEAD`, see [`diff::upgrade`].
pub fn upgrade(args: &UpgradeArgs) -> anyhow::Result<()> {
    let empty = (Vec::new(), Trace::default());
    let (current, before, target, name);
    if let Some(import) = &args.import {
        // The component moves; the current state is also the base, since
        // only the component changes.
        current = check::analyze(&args.path, None)?;
        target = Some(check::analyze_at(&args.path, import, &args.to)?);
        before = None;
        name = format!("{import} {}", args.to);
    } else {
        let (root, top, relative) = locate(&args.path)?;
        let to = commit(&top, &args.to)?;
        let head = commit(&top, "HEAD")?;
        let base = output(git(&top).args(["merge-base", &head, &to]))
            .map_err(|_| anyhow::anyhow!("HEAD and '{}' have no merge base", args.to))?;
        current = check::analyze(&root, None)?;
        let base_tree = Tree::write(&top, &base)?;
        before = base_tree.read(&relative)?;
        let tree = Tree::write(&top, &to)?;
        target = tree.read(&relative)?;
        if target.is_none() {
            eprintln!(
                "warning: the project {} does not exist in {}",
                relative.display(),
                short(&to)
            );
        }
        name = short(&to);
    }
    let base = match &before {
        Some(project) => (project.items.as_slice(), &project.trace),
        None if args.import.is_some() => (current.items.as_slice(), &current.trace),
        None => (empty.0.as_slice(), &empty.1),
    };
    let stale = diff::upgrade(
        base,
        (current.items.as_slice(), &current.trace),
        state(&target, &empty),
    );
    let mut review = 0;
    for entry in &stale {
        let link = entry.link;
        let what = match entry.target {
            Some(target) => format!("revised to {} in {name}", target.id),
            None => format!("removed in {name}"),
        };
        let then = if entry.updated {
            "updated there"
        } else {
            review += 1;
            "to review"
        };
        println!(
            "{}: {} {} {}: {what}; {then}",
            link.location,
            source(link, &current.items),
            verb(link.kind),
            link.target,
        );
    }
    let given = if args.import.is_some() {
        String::new()
    } else {
        format!(" ({})", args.to)
    };
    eprintln!(
        "{} stale {}, {review} to review, {} updated in {name}{given}",
        stale.len(),
        if stale.len() == 1 { "link" } else { "links" },
        stale.len() - review,
    );
    Ok(())
}

/// The project at or above `path`: its root, the top of its Git work
/// tree and the root relative to that top. A workspace root is no
/// project.
fn locate(path: &Path) -> anyhow::Result<(PathBuf, PathBuf, PathBuf)> {
    let root = find_root(path)?;
    // Earlier states use the components of the working tree's cache.
    crate::component::share_cache(&check::workspace_root(&root).unwrap_or_else(|_| root.clone()));
    let toml = std::fs::read_to_string(root.join(config::FILE_NAME))
        .with_context(|| format!("cannot read {}", root.join(config::FILE_NAME).display()))?;
    if config::Config::parse(&toml).is_ok_and(|config| config.workspace.is_some()) {
        bail!(
            "{} describes a workspace, not a project; run this in a member",
            root.join(config::FILE_NAME).display()
        );
    }
    let top = git_top(&root)?;
    let relative = root
        .strip_prefix(&top)
        .with_context(|| {
            format!(
                "{} is not in the Git work tree {}",
                root.display(),
                top.display()
            )
        })?
        .to_path_buf();
    Ok((root, top, relative))
}

/// The items and trace of a state, those of `empty` without the project.
fn state<'a>(
    project: &'a Option<Project>,
    empty: &'a (Vec<Item>, Trace),
) -> (&'a [Item], &'a Trace) {
    match project {
        Some(project) => (project.items.as_slice(), &project.trace),
        None => (empty.0.as_slice(), &empty.1),
    }
}

/// A Git command in `dir`, without the `GIT_*` variables of the
/// environment: inside a Git hook they name the hook's index and
/// repository, which must not be touched.
pub(crate) fn git(dir: &Path) -> Command {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command.arg("-C").arg(dir);
    command
}

/// The output of a Git command, trimmed; its error output on failure.
pub(crate) fn output(command: &mut Command) -> anyhow::Result<String> {
    let output = command.output().context("cannot run git")?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if error.is_empty() {
            bail!("git failed with {}", output.status);
        }
        bail!("{error}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The top of the Git work tree of `dir`.
pub(crate) fn git_top(dir: &Path) -> anyhow::Result<PathBuf> {
    let top = output(git(dir).args(["rev-parse", "--show-toplevel"]))
        .with_context(|| format!("{} is not in a Git work tree", dir.display()))?;
    dunce::canonicalize(&top).with_context(|| format!("cannot resolve {top}"))
}

/// The commit a revision names.
pub(crate) fn commit(top: &Path, revision: &str) -> anyhow::Result<String> {
    if revision.starts_with('-') {
        bail!("invalid revision '{revision}'");
    }
    let unknown = || anyhow::anyhow!("unknown revision '{revision}'");
    let object = output(git(top).args(["rev-parse", "--verify", "--quiet", revision]))
        .map_err(|_| unknown())?;
    output(
        git(top)
            .args(["rev-parse", "--verify", "--quiet"])
            .arg(format!("{object}^{{commit}}")),
    )
    .map_err(|_| anyhow::anyhow!("'{revision}' names no commit"))
}

/// Whether `HEAD` of the repository at `top` names a branch without a
/// commit yet, as before the first commit or on a new orphan branch.
pub(crate) fn is_unborn(top: &Path) -> bool {
    output(git(top).args(["symbolic-ref", "-q", "HEAD"])).is_ok()
        && output(git(top).args(["rev-parse", "--verify", "-q", "HEAD"])).is_err()
}

/// The base commit and the head commit of a range, the head none for the
/// working tree: `base` compares with the working tree, `base..head`
/// with `head`, and `base...head` the merge base of both with `head`, as
/// in a merge request; an empty side is `HEAD`.
fn range(top: &Path, range: &str) -> anyhow::Result<(String, Option<String>)> {
    let or_head = |side: &str| {
        if side.is_empty() {
            "HEAD".to_owned()
        } else {
            side.to_owned()
        }
    };
    if let Some((base, head)) = range.split_once("...") {
        let head = commit(top, &or_head(head))?;
        let base = commit(top, &or_head(base))?;
        let merge_base = output(git(top).args(["merge-base", &base, &head]))
            .map_err(|_| anyhow::anyhow!("'{range}' has no merge base"))?;
        return Ok((merge_base, Some(head)));
    }
    if let Some((base, head)) = range.split_once("..") {
        return Ok((
            commit(top, &or_head(base))?,
            Some(commit(top, &or_head(head))?),
        ));
    }
    Ok((commit(top, range)?, None))
}

/// An abbreviated commit for messages.
pub(crate) fn short(commit: &str) -> String {
    commit.chars().take(12).collect()
}

/// A commit written into a temporary directory, removed on drop.
pub(crate) struct Tree {
    /// The directory of the files, like the top of a work tree.
    pub(crate) dir: PathBuf,
    /// The commit, for messages.
    commit: String,
    /// Removes the files on drop.
    temp: tempfile::TempDir,
}

impl Tree {
    /// Writes the files of `commit` of the repository at `top`.
    pub(crate) fn write(top: &Path, commit: &str) -> anyhow::Result<Self> {
        Self::write_part(top, commit, Path::new(""))
    }

    /// Writes only the files of `commit` below `part`, a directory
    /// relative to `top`, e.g. the root of a workspace.
    pub(crate) fn write_part(top: &Path, commit: &str, part: &Path) -> anyhow::Result<Self> {
        let temp = tempfile::Builder::new()
            .prefix("shall-diff-")
            .tempdir()
            .context("cannot create a temporary directory")?;
        let dir = dunce::canonicalize(temp.path())?.join("tree");
        write_tree(top, commit, &dir, part, true)
            .with_context(|| format!("cannot read {}", short(commit)))?;
        // Paths in the tree stand for those of the work tree at `top`.
        crate::component::map_tree(&dir, top);
        // Ends the search for a workspace above, as at the top of a work
        // tree.
        std::fs::write(dir.join(".git"), "").context("cannot write the temporary tree")?;
        Ok(Self {
            dir,
            commit: commit.to_owned(),
            temp,
        })
    }

    /// Adds `files` of the working tree, absolute paths below `top`, where
    /// this state has no file at their place, e.g. test results.
    pub(crate) fn add_files(&self, top: &Path, files: &[PathBuf]) -> anyhow::Result<()> {
        for file in files {
            let Ok(relative) = file.strip_prefix(top) else {
                continue;
            };
            let target = self.dir.join(relative);
            if target.exists() || !file.is_file() {
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("cannot create {}", parent.display()))?;
            }
            std::fs::copy(file, &target)
                .with_context(|| format!("cannot copy {}", relative.display()))?;
        }
        Ok(())
    }

    /// Reads and traces the project at `relative`; none if the project
    /// does not exist in this state. Messages name the commit instead of
    /// the temporary directory.
    fn read(&self, relative: &Path) -> anyhow::Result<Option<Project>> {
        let dir = self.dir.join(relative);
        if !dir.join(config::FILE_NAME).is_file() {
            return Ok(None);
        }
        check::analyze(&dir, None)
            .map(Some)
            .map_err(|error| self.error(&error))
    }

    /// An error of reading this state, naming the commit instead of the
    /// temporary directory.
    pub(crate) fn error(&self, error: &anyhow::Error) -> anyhow::Error {
        let message = format!("{error:#}").replace(
            &format!("{}{}", self.dir.display(), std::path::MAIN_SEPARATOR),
            &format!("{}:", short(&self.commit)),
        );
        if message.contains(&short(&self.commit)) {
            anyhow::anyhow!(message)
        } else {
            anyhow::anyhow!("in {}: {message}", short(&self.commit))
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(self.temp.path()) {
            eprintln!(
                "warning: cannot remove {}: {error}",
                self.temp.path().display()
            );
        }
    }
}

/// Writes the files of `commit` of the repository at `repo` below `dir`:
/// blobs as files, executable or not, symbolic links as links where the
/// platform has them and as files with the target otherwise, or not at
/// all without `links`, and a submodule from its own repository below
/// `repo`, if it is cloned.
pub(crate) fn write_tree(
    repo: &Path,
    commit: &str,
    dir: &Path,
    part: &Path,
    links: bool,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let mut command = git(repo);
    command.args(["ls-tree", "-r", "-z", "--full-tree", commit]);
    if !part.as_os_str().is_empty() {
        command.arg("--").arg(part);
    }
    let list = command.output().context("cannot run git")?;
    if !list.status.success() {
        bail!("{}", String::from_utf8_lossy(&list.stderr).trim());
    }
    let mut blobs: Vec<(String, String, PathBuf)> = Vec::new();
    for entry in list
        .stdout
        .split(|&b| b == 0)
        .filter(|entry| !entry.is_empty())
    {
        let unexpected = || {
            anyhow::anyhow!(
                "unexpected git ls-tree output '{}'",
                String::from_utf8_lossy(entry)
            )
        };
        let tab = entry
            .iter()
            .position(|&b| b == b'\t')
            .ok_or_else(unexpected)?;
        let meta = std::str::from_utf8(&entry[..tab]).map_err(|_| unexpected())?;
        let mut parts = meta.split(' ');
        let (Some(mode), Some(_), Some(object)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(unexpected());
        };
        // The path as Git stores it, so that a name that is no valid UTF-8
        // is skipped when read, as in the working tree.
        let relative = path_from_bytes(&entry[tab + 1..]);
        // Paths come from the repository: keep them inside `dir`.
        if !relative
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            bail!("invalid path '{}' in the repository", relative.display());
        }
        let target = dir.join(&relative);
        if mode == "160000" {
            // A submodule: its own repository at that commit. A clone
            // without that commit, e.g. before `git submodule update`, is
            // read at the commit it has checked out, as a work tree would
            // show it; a submodule that is not cloned is left out.
            let submodule = repo.join(&relative);
            let has = |commit: &str| {
                git(&submodule)
                    .args(["cat-file", "-e"])
                    .arg(format!("{commit}^{{tree}}"))
                    .output()
                    .is_ok_and(|output| output.status.success())
            };
            if !submodule.join(".git").exists() {
                eprintln!(
                    "warning: submodule {} is not cloned; it is left out of {}",
                    relative.display(),
                    short(commit)
                );
            } else if has(object) {
                write_tree(&submodule, object, &target, Path::new(""), links)?;
            } else {
                let checked_out = output(git(&submodule).args(["rev-parse", "HEAD"]))
                    .with_context(|| format!("cannot read submodule {}", relative.display()))?;
                eprintln!(
                    "warning: submodule {} does not have commit {} of {}; it is read at its \
                     checked-out commit {}",
                    relative.display(),
                    short(object),
                    short(commit),
                    short(&checked_out)
                );
                write_tree(&submodule, &checked_out, &target, Path::new(""), links)?;
            }
            continue;
        }
        if mode == "120000" && !links {
            eprintln!(
                "warning: {} is a symbolic link; it is left out of {}",
                relative.display(),
                short(commit)
            );
            continue;
        }
        blobs.push((mode.to_owned(), object.to_owned(), target));
    }
    write_blobs(repo, &blobs)
}

/// A path from the bytes of a Git path.
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// Writes the blobs `(mode, object, path)` with one `git cat-file
/// --batch`, fed from a thread so that neither side blocks.
fn write_blobs(repo: &Path, blobs: &[(String, String, PathBuf)]) -> anyhow::Result<()> {
    use std::io::{BufRead, BufReader, Read, Write};
    let mut child = git(repo)
        .args(["cat-file", "--batch"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("cannot run git")?;
    let mut stdin = child.stdin.take().expect("piped");
    let objects: Vec<String> = blobs.iter().map(|(_, object, _)| object.clone()).collect();
    let feeder = std::thread::spawn(move || -> std::io::Result<()> {
        for object in objects {
            writeln!(stdin, "{object}")?;
        }
        Ok(())
    });
    let mut stdout = BufReader::new(child.stdout.take().expect("piped"));
    for (mode, object, path) in blobs {
        let mut header = String::new();
        stdout.read_line(&mut header)?;
        let size: usize = header
            .trim_end()
            .rsplit(' ')
            .next()
            .and_then(|size| size.parse().ok())
            .with_context(|| format!("cannot read object {object}: {}", header.trim_end()))?;
        let mut content = vec![0; size];
        stdout.read_exact(&mut content)?;
        let mut newline = [0; 1];
        stdout.read_exact(&mut newline)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        write_blob(mode, &content, path)
            .with_context(|| format!("cannot write {}", path.display()))?;
    }
    drop(stdout);
    let fed = feeder.join().expect("the feeder does not panic");
    let status = child.wait()?;
    fed.context("cannot run git cat-file")?;
    if !status.success() {
        bail!("git cat-file failed with {status}");
    }
    Ok(())
}

/// Writes one blob as a file, or as a symbolic link for mode `120000`.
fn write_blob(mode: &str, content: &[u8], path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    if mode == "120000" {
        use std::os::unix::ffi::OsStrExt;
        return std::os::unix::fs::symlink(std::ffi::OsStr::from_bytes(content), path);
    }
    std::fs::write(path, content)?;
    #[cfg(unix)]
    if mode == "100755" {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// How a change names an item: its ID and title.
fn named(item: &Item) -> String {
    if item.title.is_empty() {
        item.id.to_string()
    } else {
        format!("{} ({})", item.id, item.title)
    }
}

/// How a broken link names its source: the ID, or the tag as written.
fn source<'a>(link: &'a Link, items: &'a [Item]) -> String {
    let item = items
        .iter()
        .find(|item| item.location == link.source_location && item.id == link.source);
    match item.map(|item| &item.origin) {
        Some(Origin::Tag { text, .. }) => format!("tag {text}"),
        _ => link.source.to_string(),
    }
}

fn verb(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::Covers => "covers",
        LinkKind::Depends => "depends on",
        LinkKind::SatisfiedBy => "is satisfied by",
    }
}

/// One line per change; removed items at their location in the base.
fn text(changes: &Changes<'_>, head: &[Item]) -> String {
    let mut out = String::new();
    for item in &changes.added {
        let _ = writeln!(out, "{}: added {}", item.location, named(item));
    }
    for item in &changes.removed {
        let _ = writeln!(
            out,
            "{}: removed {} (in the base)",
            item.location,
            named(item)
        );
    }
    for (old, new) in &changes.revised {
        let _ = writeln!(
            out,
            "{}: revised {} -> {}",
            new.location,
            old.id,
            named(new)
        );
    }
    for (_, new) in &changes.edited {
        let _ = writeln!(
            out,
            "{}: edited {} without a new revision",
            new.location,
            named(new)
        );
    }
    for link in &changes.broken {
        let _ = writeln!(
            out,
            "{}: broken link: {} {} {}: {}",
            link.location,
            source(link, head),
            verb(link.kind),
            link.target,
            link.status.problem().unwrap_or_default()
        );
    }
    out
}

/// Markdown tables of the changes, with a heading naming both states.
fn markdown(changes: &Changes<'_>, head: &[Item], base_name: &str, head_name: &str) -> String {
    let mut out = format!(
        "### Requirement changes\n\n{} from `{base_name}` to {}.\n",
        cell(&summary(changes)),
        if head_name == "the working tree" {
            head_name.to_owned()
        } else {
            format!("`{head_name}`")
        }
    );
    let items = |out: &mut String, heading: &str, rows: Vec<(String, &Item)>| {
        if rows.is_empty() {
            return;
        }
        let _ = write!(
            out,
            "\n**{heading} ({})**\n\n| ID | Title | Location |\n| --- | --- | --- |\n",
            rows.len()
        );
        for (id, item) in rows {
            let _ = writeln!(
                out,
                "| {} | {} | {} |",
                cell(&id),
                cell(&item.title),
                cell(&item.location.to_string())
            );
        }
    };
    items(
        &mut out,
        "Added",
        changes
            .added
            .iter()
            .map(|item| (item.id.to_string(), *item))
            .collect(),
    );
    items(
        &mut out,
        "Removed",
        changes
            .removed
            .iter()
            .map(|item| (item.id.to_string(), *item))
            .collect(),
    );
    items(
        &mut out,
        "Revised",
        changes
            .revised
            .iter()
            .map(|(old, new)| (format!("{} -> {}", old.id, new.id), *new))
            .collect(),
    );
    items(
        &mut out,
        "Edited without a new revision",
        changes
            .edited
            .iter()
            .map(|(_, new)| (new.id.to_string(), *new))
            .collect(),
    );
    if !changes.broken.is_empty() {
        let _ = write!(
            out,
            "\n**Broken links ({})**\n\n| Link | Problem | Location |\n| --- | --- | --- |\n",
            changes.broken.len()
        );
        for link in &changes.broken {
            let _ = writeln!(
                out,
                "| {} | {} | {} |",
                cell(&format!(
                    "{} {} {}",
                    source(link, head),
                    verb(link.kind),
                    link.target
                )),
                cell(&link.status.problem().unwrap_or_default()),
                cell(&link.location.to_string())
            );
        }
    }
    out
}

/// The numbers of changes, e.g. `1 added, 0 removed, ...`.
fn summary(changes: &Changes<'_>) -> String {
    format!(
        "{} added, {} removed, {} revised, {} edited, {} broken {}",
        changes.added.len(),
        changes.removed.len(),
        changes.revised.len(),
        changes.edited.len(),
        changes.broken.len(),
        if changes.broken.len() == 1 {
            "link"
        } else {
            "links"
        }
    )
}
