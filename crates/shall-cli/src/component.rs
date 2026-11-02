//! External components: `[[import]]` with `git` and `rev`.
//!
//! The repository of a component is mirrored as a bare repository in
//! `.shall/cache/<name>-<hash>/repo.git` below the workspace root, or the
//! project root without a workspace, where the hash stands for the
//! repository, and the commit that `rev` names is written from its
//! objects into `.shall/cache/<name>-<hash>/<commit>/`, as `shall diff`
//! writes a state, without symbolic links, so that a component cannot
//! make `shall` read files outside of it. The component is read from
//! there like a project of its own, and its files are named by that path
//! in diagnostics.
//!
//! `rev` is a tag, a branch or a commit ID. A tag or a full commit ID
//! that the mirror has is not fetched again, since tags do not move; a
//! branch is fetched on every run, and `--offline` never fetches, but
//! uses what the mirror has. A lock file per component lets several runs
//! share the cache. `.shall/` gets a `.gitignore` that ignores it.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, bail};

use crate::diff::{self, git, output};

/// Whether components may be fetched (`--offline` clears it).
static OFFLINE: AtomicBool = AtomicBool::new(false);

/// The cache to use instead of the one below the root being read, e.g.
/// the working tree's while `shall diff` reads an earlier state from a
/// temporary directory.
static CACHE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Temporary trees and the work trees they stand for, so that a relative
/// `git` in an earlier state names the same repository as today.
static TREES: Mutex<Vec<(PathBuf, PathBuf)>> = Mutex::new(Vec::new());

/// Never fetches components from now on.
pub fn set_offline(offline: bool) {
    OFFLINE.store(offline, Ordering::Relaxed);
}

/// Uses the cache below `root` for every later read, also of temporary
/// trees; keeps an earlier choice.
pub fn share_cache(root: &Path) {
    let mut cache = CACHE.lock().expect("no panic while holding it");
    cache.get_or_insert_with(|| root.join(".shall").join("cache"));
}

/// Records that the temporary tree `tree` holds the files of the work
/// tree `real`.
pub fn map_tree(tree: &Path, real: &Path) {
    TREES
        .lock()
        .expect("no panic while holding it")
        .push((tree.to_path_buf(), real.to_path_buf()));
}

/// The path that messages and `trace.json` use for `dir`, a directory of
/// a component in the cache: `.shall/cache/<name>-<hash>/<commit>`, the
/// same for every state that reads it; none for other directories.
pub fn display(dir: &Path) -> Option<String> {
    let cache = CACHE.lock().expect("no panic while holding it").clone()?;
    let cache = dunce::canonicalize(cache).ok()?;
    let inside = dir.strip_prefix(cache).ok()?;
    Some(format!(
        ".shall/cache/{}",
        inside.to_string_lossy().replace('\\', "/")
    ))
}

/// A component as an import names it.
pub struct Request<'a> {
    /// The root of the workspace, or of the project without one.
    pub root: &'a Path,
    /// The importing project.
    pub from: &'a Path,
    /// Whether the importing project is itself an external component.
    pub from_component: bool,
    pub name: &'a str,
    pub url: &'a str,
    pub rev: &'a str,
}

/// A component in the cache.
pub struct Fetched {
    /// The directory of its files at the commit.
    pub dir: PathBuf,
    /// The repository as compared: a local path made absolute, without a
    /// trailing `/` or `.git`.
    pub url: String,
    /// The commit that `rev` names.
    pub commit: String,
}

/// Fetches the component into the cache if needed and returns where it
/// is. A `git` without a scheme is a path, relative to the importing
/// project, which must then not be a component itself.
pub fn fetch(request: &Request<'_>) -> anyhow::Result<Fetched> {
    let Request {
        root,
        from,
        from_component,
        name,
        url,
        rev,
    } = *request;
    let what = format!("import '{name}' of {}", from.display());
    if url.starts_with('-') {
        bail!("{what}: invalid 'git' '{url}'");
    }
    if !is_rev(rev) {
        bail!("{what}: invalid 'rev' '{rev}': expected a tag, a branch or a commit ID");
    }
    if from_component && is_local(url) && Path::new(url).is_relative() {
        bail!(
            "{what}: an external component names other components by URL, not by the \
             relative path '{url}'"
        );
    }
    let (source, url) = normalize(from, url).with_context(|| what.clone())?;
    let cache = CACHE
        .lock()
        .expect("no panic while holding it")
        .clone()
        .unwrap_or_else(|| root.join(".shall").join("cache"));
    let home = cache.join(format!("{name}-{:08x}", hash(&url)));
    fs::create_dir_all(&home).with_context(|| format!("cannot create {}", home.display()))?;
    ignore_cache(&cache)?;
    // One run at a time per component; the lock ends with the file.
    let lock = File::create(home.join(".lock"))
        .with_context(|| format!("cannot create {}", home.join(".lock").display()))?;
    lock.lock()
        .with_context(|| format!("cannot lock {}", home.display()))?;
    let repo = home.join("repo.git");
    let offline = OFFLINE.load(Ordering::Relaxed);
    if !repo.join("shall-ready").is_file() {
        if offline {
            bail!(
                "{what}: {url} is not in the cache {}; run once without --offline",
                cache.display()
            );
        }
        if repo.exists() {
            fs::remove_dir_all(&repo)
                .with_context(|| format!("cannot remove {}", repo.display()))?;
        }
        output(git(&home).args(["init", "--bare", "--quiet", "repo.git"]))
            .with_context(|| format!("{what}: cannot create {}", repo.display()))?;
        fs::write(repo.join("shall-ready"), "")?;
    }
    let known = resolve(&repo, rev);
    let commit = match known {
        Some(commit) if offline || is_fixed(&repo, rev) => commit,
        None if offline => bail!(
            "{what}: rev '{rev}' is not in the cache {}; run once without --offline",
            cache.display()
        ),
        _ => {
            output(
                git(&repo)
                    .args(["fetch", "--quiet", "--prune", "--no-tags", "--"])
                    .arg(&source)
                    .args(["+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*"]),
            )
            .with_context(|| format!("{what}: cannot fetch {source}"))?;
            resolve(&repo, rev).with_context(|| format!("{what}: {source} has no rev '{rev}'"))?
        }
    };
    let dir = home.join(&commit);
    if !dir.join(".shall-complete").is_file() {
        // Written aside and moved, so that an interrupted run leaves no
        // half tree behind that looks complete.
        let partial = home.join(format!("{commit}.{}.partial", std::process::id()));
        if partial.exists() {
            fs::remove_dir_all(&partial)
                .with_context(|| format!("cannot remove {}", partial.display()))?;
        }
        diff::write_tree(&repo, &commit, &partial, Path::new(""), false)
            .with_context(|| format!("{what}: cannot write {}", diff::short(&commit)))?;
        // Ends the search for a workspace above, as at the top of a work
        // tree.
        fs::write(partial.join(".git"), "")?;
        fs::write(partial.join(".shall-complete"), format!("{url} {rev}\n"))?;
        if dir.exists() {
            fs::remove_dir_all(&dir).with_context(|| format!("cannot remove {}", dir.display()))?;
        }
        fs::rename(&partial, &dir).with_context(|| format!("cannot write {}", dir.display()))?;
    }
    drop(lock);
    let dir =
        dunce::canonicalize(&dir).with_context(|| format!("cannot resolve {}", dir.display()))?;
    Ok(Fetched { dir, url, commit })
}

/// Whether `rev` can name a tag, a branch or a commit ID, and nothing
/// that Git would evaluate, such as `HEAD~1`.
fn is_rev(rev: &str) -> bool {
    !rev.is_empty()
        && !rev.starts_with(['-', '/', '.'])
        && !rev.ends_with(['/', '.'])
        && !rev.contains("..")
        && !rev.contains("@{")
        && rev
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '+'))
}

/// Whether a `git` value is a path on this machine rather than a URL.
fn is_local(url: &str) -> bool {
    let scheme = url.contains("://");
    // `host:path` as scp writes it, but not a Windows drive like `C:\`.
    let scp = url
        .split_once(':')
        .is_some_and(|(host, _)| host.len() > 1 && !host.contains(['/', '\\']));
    !scheme && !scp
}

/// The repository to fetch from and the key to compare it by. A local
/// path, also as a `file://` URL, which needs an absolute path, is made
/// absolute and resolved, standing for the work tree of a temporary tree;
/// its key drops a last `/.git`, the Git directory of a work tree. The
/// key of a URL drops a trailing `/` and `.git`.
fn normalize(from: &Path, url: &str) -> anyhow::Result<(String, String)> {
    let local = |path: PathBuf| {
        let path = dunce::canonicalize(&path).unwrap_or(path);
        let key = if path.file_name().is_some_and(|name| name == ".git") {
            path.parent().map_or(path.clone(), Path::to_path_buf)
        } else {
            path.clone()
        };
        (
            path.to_string_lossy().into_owned(),
            key.to_string_lossy().into_owned(),
        )
    };
    if let Some(path) = url.strip_prefix("file://") {
        if !Path::new(path).is_absolute() {
            bail!("'{url}' needs an absolute path, as in file:///srv/can");
        }
        return Ok(local(PathBuf::from(path)));
    }
    if !is_local(url) {
        let key = url.trim_end_matches('/');
        let key = key
            .strip_suffix(".git")
            .unwrap_or(key)
            .trim_end_matches('/');
        return Ok((url.to_owned(), key.to_owned()));
    }
    let mut from = from.to_path_buf();
    for (tree, real) in TREES.lock().expect("no panic while holding it").iter() {
        if let Ok(inside) = from.strip_prefix(tree) {
            from = real.join(inside);
            break;
        }
    }
    Ok(local(from.join(url)))
}

/// FNV-1a, stable across Rust versions, for the name of the cache.
fn hash(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// The commit `rev` names in `repo`, if any: a tag, a branch or a commit
/// ID, also a shortened one.
fn resolve(repo: &Path, rev: &str) -> Option<String> {
    let commit_id = rev.len() >= 7 && rev.bytes().all(|b| b.is_ascii_hexdigit());
    let candidates = [
        Some(format!("refs/tags/{rev}^{{commit}}")),
        Some(format!("refs/heads/{rev}^{{commit}}")),
        commit_id.then(|| format!("{rev}^{{commit}}")),
    ];
    candidates.into_iter().flatten().find_map(|candidate| {
        output(git(repo).args(["rev-parse", "--verify", "--quiet", &candidate])).ok()
    })
}

/// Whether `rev` is a tag or a full commit ID in `repo`, which never
/// change; a branch moves and is fetched again.
fn is_fixed(repo: &Path, rev: &str) -> bool {
    let tag = output(git(repo).args([
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("refs/tags/{rev}"),
    ]))
    .is_ok();
    let commit = matches!(rev.len(), 40 | 64) && rev.bytes().all(|b| b.is_ascii_hexdigit());
    tag || commit
}

/// Writes `.shall/.gitignore`, so that the cache is not committed.
fn ignore_cache(cache: &Path) -> anyhow::Result<()> {
    let Some(shall) = cache.parent() else {
        return Ok(());
    };
    let ignore = shall.join(".gitignore");
    if !ignore.exists() {
        fs::write(&ignore, "# The cache of shall; never commit it.\n*\n")
            .with_context(|| format!("cannot write {}", ignore.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revs_are_tags_branches_or_commits() {
        for rev in ["v1.2.0", "master", "release/1.x", "6e0d36e", "a+b"] {
            assert!(is_rev(rev), "{rev}");
        }
        for rev in [
            "", "HEAD~1", "HEAD^", "a..b", "-x", "x@{1}", ":/text", "a b",
        ] {
            assert!(!is_rev(rev), "{rev}");
        }
    }

    #[test]
    fn tells_paths_from_urls() {
        for url in ["../can", "/srv/can", "C:\\repos\\can", "can.git"] {
            assert!(is_local(url), "{url}");
        }
        for url in [
            "https://example.org/can.git",
            "git@example.org:team/can.git",
            "file:///srv/can",
        ] {
            assert!(!is_local(url), "{url}");
        }
    }

    #[test]
    fn the_hash_is_stable() {
        assert_eq!(hash(""), 0x811c_9dc5);
        assert_eq!(hash("a"), 0xe40c_292c);
    }
}
