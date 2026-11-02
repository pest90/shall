//! Tests for `shall diff`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn shall(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shall"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.org"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

const CONFIG: &str = "[workspace]\nmembers = [\"p\"]\n";

const LEVELS: &str = "[[level]]\ntype = \"swr\"\nname = \"S\"\nneeds = [\"dsn\"]\n\n\
                      [[level]]\ntype = \"dsn\"\nname = \"D\"\n";

/// A repository with a workspace and the member `p`, committed on
/// `master`.
fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("shall.toml"), CONFIG).unwrap();
    fs::create_dir_all(root.join("p/reqs")).unwrap();
    fs::write(root.join("p/shall.toml"), LEVELS).unwrap();
    fs::write(
        root.join("p/reqs/a.md"),
        "# Stop\n`swr~stop~1`\n\n# Gone\n`swr~gone~1`\n\n# Handler\n`dsn~handler~1`\n\
         Covers:\n- swr~stop~1\n",
    )
    .unwrap();
    git(root, &["init", "-q", "-b", "master"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    dir
}

#[test]
fn lists_the_changes_against_a_commit_and_between_commits() {
    let dir = repository();
    let root = dir.path();
    fs::write(
        root.join("p/reqs/a.md"),
        "# Stop now\n`swr~stop~2`\n\n# Handler\n`dsn~handler~1`\nCovers:\n- swr~stop~1\n\n\
         # New\n`swr~new~1`\n",
    )
    .unwrap();
    let expected = "p/reqs/a.md:10: added swr~new~1 (New)\n\
                    p/reqs/a.md:5: removed swr~gone~1 (Gone) (in the base)\n\
                    p/reqs/a.md:2: revised swr~stop~1 -> swr~stop~2 (Stop now)\n\
                    p/reqs/a.md:7: broken link: dsn~handler~1 covers swr~stop~1: outdated, \
                    current revision 2\n";
    // Against the working tree, from anywhere in the project.
    let output = shall(&root.join("p/reqs"), &["diff", "master"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), expected);
    assert!(
        stderr(&output)
            .starts_with("1 added, 1 removed, 1 revised, 0 edited, 1 broken link (base "),
        "{}",
        stderr(&output)
    );
    // Between commits, as a merge request.
    git(root, &["checkout", "-q", "-b", "feature"]);
    git(root, &["commit", "-q", "-am", "change"]);
    for range in ["master..feature", "master...", "master...feature"] {
        let output = shall(root, &["diff", range, "p"]);
        assert_eq!(stdout(&output), expected, "{range}: {}", stderr(&output));
    }
    let output = shall(root, &["diff", "master...", "p", "--format", "markdown"]);
    let markdown = stdout(&output);
    assert!(
        markdown.starts_with("### Requirement changes\n\n1 added, 1 removed, 1 revised, 0 edited, 1 broken link from `"),
        "{markdown}"
    );
    assert!(
        markdown.contains("| swr\\~stop\\~1 -\\> swr\\~stop\\~2 | Stop now | p/reqs/a.md:2 |\n"),
        "{markdown}"
    );
    // The work trees of the states are removed again.
    let list = Command::new("git")
        .current_dir(root)
        .args(["worktree", "list"])
        .output()
        .unwrap();
    assert_eq!(stdout(&list).lines().count(), 1, "{}", stdout(&list));
}

#[test]
fn a_new_project_is_all_added_and_nothing_is_no_change() {
    let dir = repository();
    let root = dir.path();
    let output = shall(root, &["diff", "HEAD", "p"]);
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).starts_with("0 added, 0 removed, 0 revised, 0 edited, 0 broken links"));
    fs::write(
        root.join("shall.toml"),
        "[workspace]\nmembers = [\"p\", \"q\"]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("q")).unwrap();
    fs::write(root.join("q/shall.toml"), LEVELS).unwrap();
    fs::write(root.join("q/r.md"), "# Q\n`dsn~q~1`\n").unwrap();
    let output = shall(root, &["diff", "HEAD", "q"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "q/r.md:2: added dsn~q~1 (Q)\n");
}

#[test]
fn reports_unknown_revisions() {
    let dir = repository();
    let output = shall(dir.path(), &["diff", "nope", "p"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stderr(&output), "Error: unknown revision 'nope'\n");
    let output = shall(dir.path(), &["diff", "--", "p"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn leaves_the_repository_alone_also_in_a_hook() {
    let dir = repository();
    let root = dir.path();
    fs::write(root.join("p/reqs/a.md"), "# Stop\n`swr~stop~2`\n").unwrap();
    git(root, &["add", "-A"]);
    // As inside a pre-commit hook, which names its own index.
    let output = Command::new(env!("CARGO_BIN_EXE_shall"))
        .current_dir(root)
        .env("GIT_INDEX_FILE", root.join(".git/index.hook"))
        .env("GIT_DIR", root.join("nowhere"))
        .args(["diff", "HEAD", "p"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("revised swr~stop~1 -> swr~stop~2"));
    assert!(!root.join(".git/index.hook").exists());
    // The staged change is still staged.
    let staged = Command::new("git")
        .current_dir(root)
        .args(["diff", "--cached", "--name-only"])
        .output()
        .unwrap();
    assert_eq!(stdout(&staged), "p/reqs/a.md\n");
    // A workspace root is no project, before reading any state.
    let output = shall(root, &["diff", "HEAD"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("describes a workspace, not a project"));
    // Any revision syntax that names a commit.
    let output = shall(root, &["diff", ":/base", "p"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let output = shall(root, &["diff", "HEAD^{tree}", "p"]);
    assert_eq!(stderr(&output), "Error: 'HEAD^{tree}' names no commit\n");
}

#[test]
fn reads_a_submodule_member_at_its_recorded_commit() {
    let dir = repository();
    let root = dir.path();
    let library = tempfile::tempdir().unwrap();
    fs::write(library.path().join("shall.toml"), LEVELS).unwrap();
    fs::write(library.path().join("l.md"), "# L\n`dsn~lib~1`\n").unwrap();
    git(library.path(), &["init", "-q", "-b", "master"]);
    git(library.path(), &["add", "-A"]);
    git(library.path(), &["commit", "-q", "-m", "lib"]);
    git(
        root,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            library.path().to_str().unwrap(),
            "lib",
        ],
    );
    fs::write(
        root.join("shall.toml"),
        "[workspace]\nmembers = [\"p\", \"lib\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("p/shall.toml"),
        format!("{LEVELS}\n[[import]]\nname = \"lib\"\npath = \"../lib\"\n"),
    )
    .unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "submodule"]);
    fs::write(root.join("lib/l.md"), "# L\n`dsn~lib~2`\n").unwrap();
    let output = shall(root, &["diff", "HEAD", "p"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "lib/l.md:2: revised dsn~lib~1 -> dsn~lib~2 (L)\n"
    );
}

#[test]
fn reads_a_submodule_without_its_commit_as_checked_out() {
    let dir = repository();
    let root = dir.path();
    let library = tempfile::tempdir().unwrap();
    fs::write(library.path().join("shall.toml"), LEVELS).unwrap();
    fs::write(library.path().join("l.md"), "# L\n`dsn~lib~1`\n").unwrap();
    git(library.path(), &["init", "-q", "-b", "master"]);
    git(library.path(), &["add", "-A"]);
    git(library.path(), &["commit", "-q", "-m", "lib"]);
    git(
        root,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            library.path().to_str().unwrap(),
            "lib",
        ],
    );
    // A workspace member, at a commit that its clone does not have.
    fs::write(
        root.join("shall.toml"),
        "[workspace]\nmembers = [\"p\", \"lib\"]\n",
    )
    .unwrap();
    git(root, &["add", "shall.toml"]);
    git(
        root,
        &[
            "update-index",
            "--cacheinfo",
            "160000,1111111111111111111111111111111111111111,lib",
        ],
    );
    git(root, &["commit", "-q", "-m", "unfetched"]);
    let output = shall(root, &["diff", "HEAD", "p"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stderr(&output).starts_with("warning: submodule lib does not have commit 111111111111 of "),
        "{}",
        stderr(&output)
    );
}

#[cfg(unix)]
#[test]
fn skips_paths_that_are_no_utf8_in_both_states() {
    use std::os::unix::ffi::OsStrExt;
    let dir = repository();
    let root = dir.path();
    let name = std::ffi::OsStr::from_bytes(b"p/reqs/n\xffx.md");
    fs::write(root.join(name), "# U\n`swr~u~1`\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "odd name"]);
    let output = shall(root, &["diff", "HEAD", "p"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "");
}
