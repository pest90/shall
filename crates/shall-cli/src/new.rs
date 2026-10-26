//! `shall new`: create an item with a generated ID.
//!
//! An ID is never handed out again once it was used: neither while it
//! exists in a Markdown file of the project, nor after it was deleted, as
//! long as it can be found in the Git history (all branches, tags, stashes
//! and the reflog of this clone).

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use shall_core::config::{self, Config};
use shall_core::create;
use shall_core::{ItemId, ItemType, Location, markdown};

/// Arguments of `shall new`.
pub struct NewArgs {
    pub typ: String,
    pub title: String,
    pub file: Option<PathBuf>,
    pub component: Option<String>,
    pub no_history: bool,
}

/// Creates the ID and, with `--file`, appends the item skeleton to the file.
/// Prints the ID, or `path:line: created <id>` when a file was written.
pub fn run(args: &NewArgs) -> anyhow::Result<()> {
    let cwd = std::env::current_dir().context("cannot read the current directory")?;
    let output = create(args, &cwd, || rand::random_range(0..create::ALPHABET.len()))?;
    println!("{output}");
    Ok(())
}

/// Does the work of [`run`] with an injectable random source and returns
/// the line to print.
fn create(
    args: &NewArgs,
    cwd: &Path,
    random_index: impl FnMut() -> usize,
) -> anyhow::Result<String> {
    let root = find_root(cwd)?;
    let config_path = root.join(config::FILE_NAME);
    let text = fs::read_to_string(&config_path)
        .with_context(|| format!("cannot read {}", config_path.display()))?;
    let config = Config::parse(&text)?;

    let typ: ItemType = args.typ.parse()?;
    let Some(level) = config.level(&typ) else {
        if config.levels.is_empty() {
            bail!("{} defines no [[level]]", config_path.display());
        }
        let known: Vec<&str> = config.levels.iter().map(|l| l.typ.as_str()).collect();
        bail!(
            "unknown type '{typ}'; the levels in {} are: {}",
            config_path.display(),
            known.join(", ")
        );
    };
    // Resolve the target before doing any work, so that a wrong path fails
    // early and nothing is written.
    let target = args
        .file
        .as_deref()
        .map(|file| Target::resolve(&root, cwd, file))
        .transpose()?;
    let base = create::base_name(&args.title, args.component.as_deref())?;

    let known = known_ids(&root);
    let history = if args.no_history {
        HashSet::new()
    } else {
        history_names(&root, &typ, &base)?
    };
    let id = create::generate_id(&typ, &base, random_index, |id| {
        known.contains(&key(id)) || history.contains(id.name())
    })?;

    let Some(target) = target else {
        return Ok(id.to_string());
    };
    let skeleton = create::skeleton(&id, &args.title, &level.needs, config.has_parents(&typ));
    let line = target.append(&id, &skeleton)?;
    Ok(format!("{}:{line}: created {id}", target.relative))
}

/// Returns the closest directory at or above `start` with a `shall.toml`,
/// with symbolic links resolved.
fn find_root(start: &Path) -> anyhow::Result<PathBuf> {
    let root = start
        .ancestors()
        .find(|dir| dir.join(config::FILE_NAME).is_file())
        .with_context(|| {
            format!(
                "no {} found in {} or its parents",
                config::FILE_NAME,
                start.display()
            )
        })?;
    // dunce avoids Windows `\\?\` paths, which Git does not accept.
    dunce::canonicalize(root).with_context(|| format!("cannot resolve {}", root.display()))
}

/// Type and name of an ID; IDs are taken regardless of their revision.
fn key(id: &ItemId) -> (String, String) {
    (id.typ().to_string(), id.name().to_owned())
}

/// Whether the path names a Markdown file, by its extension.
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

/// Collects the IDs of all items in Markdown files of the project,
/// including hidden directories but skipping files ignored by Git.
/// Unreadable files are skipped with a warning.
fn known_ids(root: &Path) -> HashSet<(String, String)> {
    let mut known = HashSet::new();
    // Only Git's ignore rules apply, the same that `Target::resolve` checks
    // with `git check-ignore`.
    let walk = ignore::WalkBuilder::new(root)
        .hidden(false)
        .ignore(false)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build();
    for entry in walk {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                eprintln!("warning: skipping a path: {error}");
                continue;
            }
        };
        let path = entry.path();
        if !is_markdown(path) || !path.is_file() {
            continue;
        }
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("warning: skipping {}: {error}", path.display());
                continue;
            }
        };
        let relative = path.strip_prefix(root).unwrap_or(path);
        // Problems in other files do not matter for a new ID.
        if let Ok(parsed) = markdown::parse(relative, &text) {
            known.extend(parsed.items.iter().map(|item| key(&item.id)));
        }
    }
    known
}

fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(root);
    command
}

/// Returns the names `<base>-<random>` of IDs of type `typ` that occur
/// anywhere in the Git history.
///
/// Outside of a Git repository there is no history, which only warrants a
/// warning. Inside one, an unreadable or shallow history is an error,
/// because deleted IDs could then be handed out again.
fn history_names(root: &Path, typ: &ItemType, base: &str) -> anyhow::Result<HashSet<String>> {
    // `--git-path shallow` names a file that exists only in shallow clones;
    // unlike `--is-shallow-repository`, it works with Git before 2.15.
    let probe = git(root)
        .args(["rev-parse", "--git-path", "shallow"])
        .output();
    match probe {
        Ok(output) if output.status.success() => {
            let shallow = String::from_utf8_lossy(&output.stdout);
            if root.join(shallow.trim()).exists() {
                bail!(
                    "{} is a shallow clone, so IDs deleted earlier cannot be found; \
                     run `git fetch --unshallow` or pass --no-history",
                    root.display()
                );
            }
        }
        result => {
            let in_repository = root.ancestors().any(|dir| dir.join(".git").exists());
            if !in_repository {
                eprintln!(
                    "warning: {} is not in a Git repository; IDs deleted earlier cannot be excluded",
                    root.display()
                );
                return Ok(HashSet::new());
            }
            let reason = match result {
                Ok(output) => String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                Err(error) => error.to_string(),
            };
            bail!(
                "cannot read the Git history of {}, so IDs deleted earlier could be reused \
                 ({reason}); fix Git or pass --no-history",
                root.display()
            );
        }
    }

    // One pass over the history for all candidates: find every change of
    // a line with an ID of this base and collect the names. `-G` matches
    // changed lines, so replacing one such ID by another is found too,
    // which `-S` misses because the number of matches stays the same.
    // The diff options make the output independent of user settings.
    let prefix = format!("{typ}~{base}-");
    let pattern = format!("{}[0-9a-z]+~", prefix.replace('.', "\\."));
    let output = git(root)
        .args([
            "log",
            "--all",
            "--reflog",
            "-p",
            // Stashes are merge commits; without -m their changes are not
            // searched.
            "-m",
            "--format=",
            "--text",
            "--no-textconv",
            "--no-ext-diff",
            "--no-color",
            "-G",
        ])
        .arg(&pattern)
        .output()
        .context("cannot run git")?;
    if !output.status.success() {
        bail!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let log = String::from_utf8_lossy(&output.stdout);
    let mut names = HashSet::new();
    for (start, _) in log.match_indices(&prefix) {
        let rest = &log[start + prefix.len()..];
        let len = rest.bytes().take_while(u8::is_ascii_alphanumeric).count();
        if len > 0 && rest[len..].starts_with('~') {
            names.insert(format!("{base}-{}", &rest[..len]));
        }
    }
    Ok(names)
}

/// The requirements file a new item is appended to.
struct Target {
    /// Path with symbolic links resolved.
    path: PathBuf,
    /// Path relative to the project root, separated by `/`.
    relative: String,
}

impl Target {
    /// Resolves `file`, given relative to `cwd`, and checks that it is a
    /// Markdown file inside the project, also after resolving symbolic
    /// links.
    fn resolve(root: &Path, cwd: &Path, file: &Path) -> anyhow::Result<Self> {
        let joined = cwd.join(file);
        if !is_markdown(&joined) {
            bail!(
                "{} is not a Markdown file (.md or .markdown)",
                file.display()
            );
        }
        let name = joined
            .file_name()
            .with_context(|| format!("{} is not a file", file.display()))?;
        let parent = joined.parent().unwrap_or(cwd);
        let parent = dunce::canonicalize(parent)
            .with_context(|| format!("directory {} does not exist", parent.display()))?;
        let mut path = parent.join(name);
        let metadata = fs::symlink_metadata(&path);
        if metadata.as_ref().is_ok_and(|m| m.file_type().is_symlink()) {
            path = dunce::canonicalize(&path)
                .with_context(|| format!("{} is a link to a missing file", file.display()))?;
            if !is_markdown(&path) {
                bail!(
                    "{} links to {}, which is not a Markdown file",
                    file.display(),
                    path.display()
                );
            }
        }
        if path.is_dir() {
            bail!("{} is a directory", file.display());
        }
        let relative = path
            .strip_prefix(root)
            .ok()
            .and_then(|relative| Location::new(relative, 1).ok())
            .with_context(|| {
                format!(
                    "{} is outside of the project in {}",
                    file.display(),
                    root.display()
                )
            })?;
        let relative = relative.path().to_owned();
        // IDs in these files are not seen by `known_ids`, so an ID written
        // there could be handed out again.
        if relative.split('/').any(|part| part == ".git") {
            bail!("{} is inside the .git directory", file.display());
        }
        let ignored = git(root)
            .args(["check-ignore", "-q", "--"])
            .arg(&relative)
            .status()
            .is_ok_and(|status| status.code() == Some(0));
        if ignored {
            bail!("{} is ignored by Git", file.display());
        }
        Ok(Self { path, relative })
    }

    /// Appends the skeleton of `id`, creating the file if needed, and
    /// returns the line of the ID. The file keeps its line endings. Fails
    /// without writing if the new item would not be recognised, e.g.
    /// because the file ends inside an unclosed code block.
    fn append(&self, id: &ItemId, skeleton: &str) -> anyhow::Result<u32> {
        let existing = match fs::read_to_string(&self.path) {
            Ok(existing) => existing,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(error).with_context(|| format!("cannot read {}", self.relative));
            }
        };
        let empty = existing.trim_start_matches('\u{feff}').is_empty();
        let mut addition = String::new();
        if !empty && !existing.ends_with('\n') {
            addition.push('\n');
        }
        // The skeleton starts with a blank line, which an empty file does
        // not need.
        addition.push_str(if empty {
            skeleton.trim_start_matches('\n')
        } else {
            skeleton
        });
        if existing.contains("\r\n") {
            addition = addition.replace('\n', "\r\n");
        }

        let combined = format!("{existing}{addition}");
        let parsed = markdown::parse(Path::new(&self.relative), &combined)?;
        let item = parsed
            .items
            .iter()
            .find(|item| &item.id == id)
            .with_context(|| {
                format!(
                    "the new item would not be recognised at the end of {}; \
                     is a code block left open?",
                    self.relative
                )
            })?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut file| file.write_all(addition.as_bytes()))
            .with_context(|| format!("cannot write {}", self.relative))?;
        Ok(item.location.line())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "[[level]]\ntype = \"swr\"\nname = \"Software\"\nneeds = [\"dsn\"]\n";

    fn git_ok(dir: &Path, args: &[&str]) {
        let status = git(dir)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.org"])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    /// A project with `shall.toml` in a fresh Git repository.
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("shall.toml"), CONFIG).unwrap();
        git_ok(dir.path(), &["init", "-q"]);
        dir
    }

    fn commit_all(dir: &Path, message: &str) {
        git_ok(dir, &["add", "-A"]);
        git_ok(dir, &["commit", "-q", "--allow-empty", "-m", message]);
    }

    /// Random source that yields the indices of the given characters.
    fn sequence(chars: &str) -> impl FnMut() -> usize {
        let indices: Vec<usize> = chars
            .bytes()
            .map(|b| create::ALPHABET.iter().position(|&a| a == b).unwrap())
            .collect();
        let mut next = 0;
        move || {
            let index = indices[next % indices.len()];
            next += 1;
            index
        }
    }

    fn args(file: Option<&str>) -> NewArgs {
        NewArgs {
            typ: "swr".to_owned(),
            title: "x".to_owned(),
            file: file.map(PathBuf::from),
            component: None,
            no_history: false,
        }
    }

    fn new_id(dir: &Path) -> anyhow::Result<String> {
        create(&args(None), dir, sequence("aaaabbbb"))
    }

    #[test]
    fn rerolls_ids_in_working_tree() {
        let dir = project();
        fs::write(dir.path().join("a.md"), "`swr~x-aaaa~1`\n").unwrap();
        assert_eq!(new_id(dir.path()).unwrap(), "swr~x-bbbb~1");
    }

    #[test]
    fn rerolls_ids_in_hidden_directories() {
        let dir = project();
        fs::create_dir(dir.path().join(".docs")).unwrap();
        fs::write(dir.path().join(".docs/a.MD"), "`swr~x-aaaa~1`\n").unwrap();
        assert_eq!(new_id(dir.path()).unwrap(), "swr~x-bbbb~1");
    }

    #[test]
    fn rerolls_deleted_ids() {
        let dir = project();
        fs::write(dir.path().join("a.md"), "`swr~x-aaaa~2`\n").unwrap();
        commit_all(dir.path(), "add");
        fs::remove_file(dir.path().join("a.md")).unwrap();
        commit_all(dir.path(), "delete");
        assert_eq!(new_id(dir.path()).unwrap(), "swr~x-bbbb~1");
    }

    #[test]
    fn rerolls_ids_only_in_reflog_or_old_stashes() {
        let dir = project();
        commit_all(dir.path(), "base");
        fs::write(dir.path().join("a.md"), "`swr~x-aaaa~1`\n").unwrap();
        commit_all(dir.path(), "add");
        git_ok(dir.path(), &["reset", "-q", "--hard", "HEAD~1"]);
        assert_eq!(new_id(dir.path()).unwrap(), "swr~x-bbbb~1");

        let dir = project();
        fs::write(dir.path().join("a.md"), "").unwrap();
        commit_all(dir.path(), "base");
        fs::write(dir.path().join("a.md"), "`swr~x-aaaa~1`\n").unwrap();
        git_ok(dir.path(), &["stash", "-q"]);
        fs::write(dir.path().join("a.md"), "other\n").unwrap();
        git_ok(dir.path(), &["stash", "-q"]);
        assert_eq!(new_id(dir.path()).unwrap(), "swr~x-bbbb~1");
    }

    #[test]
    fn rerolls_ids_that_replaced_another_on_other_branch() {
        let dir = project();
        fs::write(dir.path().join("a.md"), "`swr~x-aaaa~1`\n").unwrap();
        commit_all(dir.path(), "add");
        git_ok(dir.path(), &["checkout", "-q", "-b", "other"]);
        fs::write(dir.path().join("a.md"), "`swr~x-bbbb~1`\n").unwrap();
        commit_all(dir.path(), "swap");
        git_ok(dir.path(), &["checkout", "-q", "-"]);
        let id = create(&args(None), dir.path(), sequence("bbbbcccc")).unwrap();
        assert_eq!(id, "swr~x-cccc~1");
    }

    #[test]
    fn history_ignores_diff_settings() {
        let dir = project();
        fs::write(dir.path().join(".gitattributes"), "*.md -diff diff=hide\n").unwrap();
        git_ok(dir.path(), &["config", "diff.hide.textconv", "sed s/~/_/g"]);
        fs::write(dir.path().join("a.md"), "`swr~x-aaaa~1`\n").unwrap();
        commit_all(dir.path(), "add");
        fs::write(dir.path().join("a.md"), "").unwrap();
        commit_all(dir.path(), "delete");
        assert_eq!(new_id(dir.path()).unwrap(), "swr~x-bbbb~1");
    }

    #[test]
    fn same_name_with_other_type_or_base_is_free() {
        let dir = project();
        fs::write(dir.path().join("a.md"), "`dsn~x-aaaa~1`\n`swr~xy-aaaa~1`\n").unwrap();
        commit_all(dir.path(), "add");
        assert_eq!(new_id(dir.path()).unwrap(), "swr~x-aaaa~1");
    }

    #[test]
    fn shallow_clone_is_an_error_unless_history_is_skipped() {
        let source = project();
        fs::write(source.path().join("a.md"), "`swr~x-aaaa~1`\n").unwrap();
        commit_all(source.path(), "add");
        fs::write(source.path().join("a.md"), "").unwrap();
        commit_all(source.path(), "delete");
        let clones = tempfile::tempdir().unwrap();
        let url = format!("file://{}", source.path().display());
        git_ok(
            clones.path(),
            &["clone", "-q", "--depth", "1", &url, "clone"],
        );
        let clone = clones.path().join("clone");

        let error = new_id(&clone).unwrap_err().to_string();
        assert!(error.contains("shallow clone"), "{error}");
        let mut skip = args(None);
        skip.no_history = true;
        assert_eq!(
            create(&skip, &clone, sequence("aaaa")).unwrap(),
            "swr~x-aaaa~1"
        );
    }

    #[test]
    fn broken_git_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("shall.toml"), CONFIG).unwrap();
        fs::write(dir.path().join(".git"), "gitdir: /nonexistent\n").unwrap();
        let error = new_id(dir.path()).unwrap_err().to_string();
        assert!(error.contains("cannot read the Git history"), "{error}");
    }

    #[test]
    fn works_without_git_repository() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("shall.toml"), CONFIG).unwrap();
        assert_eq!(new_id(dir.path()).unwrap(), "swr~x-aaaa~1");
    }

    #[test]
    fn appends_with_line_endings_of_the_file() {
        let dir = project();
        fs::write(dir.path().join("a.md"), "# A\r\n").unwrap();
        let output = create(&args(Some("a.md")), dir.path(), sequence("aaaa")).unwrap();
        assert_eq!(output, "a.md:4: created swr~x-aaaa~1");
        let text = fs::read_to_string(dir.path().join("a.md")).unwrap();
        assert!(!text.replace("\r\n", "").contains('\n'), "{text:?}");

        fs::write(dir.path().join("b.md"), "\u{feff}").unwrap();
        let output = create(&args(Some("b.md")), dir.path(), sequence("bbbb")).unwrap();
        assert_eq!(output, "b.md:2: created swr~x-bbbb~1");
    }

    #[test]
    fn refuses_to_append_into_open_code_block() {
        let dir = project();
        let original = "# A\n\n```\ncode\n";
        fs::write(dir.path().join("a.md"), original).unwrap();
        let error = create(&args(Some("a.md")), dir.path(), sequence("aaaa"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("code block left open"), "{error}");
        assert_eq!(
            fs::read_to_string(dir.path().join("a.md")).unwrap(),
            original
        );
    }

    #[test]
    fn refuses_other_targets() {
        let dir = project();
        fs::create_dir(dir.path().join("dir.md")).unwrap();
        fs::create_dir(dir.path().join("build")).unwrap();
        fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();
        for (file, message) in [
            ("shall.toml", "is not a Markdown file"),
            ("missing/a.md", "does not exist"),
            ("dir.md", "is a directory"),
            (".git/notes.md", "inside the .git directory"),
            ("build/a.md", "is ignored by Git"),
        ] {
            let error = create(&args(Some(file)), dir.path(), sequence("aaaa"))
                .unwrap_err()
                .to_string();
            assert!(error.contains(message), "{file}: {error}");
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("shall.toml")).unwrap(),
            CONFIG
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_out_of_the_project_are_rejected() {
        let dir = project();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("out")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("b.md"), dir.path().join("b.md")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("shall.toml"), dir.path().join("c.md")).unwrap();
        for (file, message) in [
            ("out/a.md", "outside of the project"),
            ("b.md", "link to a missing file"),
            ("c.md", "which is not a Markdown file"),
        ] {
            let error = create(&args(Some(file)), dir.path(), sequence("aaaa"))
                .unwrap_err()
                .to_string();
            assert!(error.contains(message), "{file}: {error}");
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("shall.toml")).unwrap(),
            CONFIG
        );
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn accepts_paths_through_a_linked_project() {
        let dir = project();
        let links = tempfile::tempdir().unwrap();
        let link = links.path().join("project");
        std::os::unix::fs::symlink(dir.path(), &link).unwrap();
        let file = link.join("a.md");
        let output = create(&args(Some(file.to_str().unwrap())), &link, sequence("aaaa")).unwrap();
        assert_eq!(output, "a.md:2: created swr~x-aaaa~1");
    }
}
