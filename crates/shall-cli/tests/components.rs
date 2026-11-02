//! Tests for external components: `[[import]]` with `git` and `rev`.

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

/// A component repository with the capability `cap~can.send~1` at tag
/// `v1` and `cap~can.send~2` at tag `v2`, on branch `master`.
fn component() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("shall.toml"),
        "[[level]]\ntype = \"cap\"\nname = \"Capabilities\"\n",
    )
    .unwrap();
    git(root, &["init", "-q", "-b", "master"]);
    for revision in [1, 2] {
        fs::write(
            root.join("caps.md"),
            format!("# Send\n`cap~can.send~{revision}`\n"),
        )
        .unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "caps"]);
        git(root, &["tag", &format!("v{revision}")]);
    }
    dir
}

fn importer(component: &Path, rev: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("shall.toml"),
        format!(
            "[[level]]\ntype = \"hlr\"\nname = \"Customer\"\nneeds = [\"cap\"]\n\n\
             [[import]]\nname = \"can\"\ngit = \"{}\"\nrev = \"{rev}\"\n",
            component.display()
        ),
    )
    .unwrap();
    fs::write(
        dir.path().join("reqs.md"),
        "# Send\n`hlr~send~1`\n\nSatisfied-by:\n- cap~can.send~1\n",
    )
    .unwrap();
    dir
}

#[test]
fn imports_a_component_at_its_rev_from_the_cache() {
    let component = component();
    let project = importer(component.path(), "v1");
    let output = shall(project.path(), &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).starts_with("ok: 0 errors, 0 warnings (2 requirements"),
        "{}",
        stderr(&output)
    );
    assert_eq!(
        fs::read_to_string(project.path().join(".shall/.gitignore")).unwrap(),
        "# The cache of shall; never commit it.\n*\n"
    );
    // Offline, from the cache; v2 came along with the first fetch.
    let output = shall(project.path(), &["check", "--offline"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let at_v2 = importer(component.path(), "v2");
    copy_cache(project.path(), at_v2.path());
    let output = shall(at_v2.path(), &["--offline", "check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("is satisfied by outdated revision cap~can.send~1"),
        "{}",
        stdout(&output)
    );
    // A rev that does not exist, offline and online.
    let missing = importer(component.path(), "v9");
    copy_cache(project.path(), missing.path());
    let output = shall(missing.path(), &["check", "--offline"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("rev 'v9' is not in the cache"),
        "{}",
        stderr(&output)
    );
    let output = shall(missing.path(), &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("has no rev 'v9'"),
        "{}",
        stderr(&output)
    );
}

fn copy_cache(from: &Path, to: &Path) {
    copy(&from.join(".shall"), &to.join(".shall"));
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn one_component_in_two_states_is_an_error() {
    let component = component();
    let project = importer(component.path(), "v1");
    // A second component that imports the first at v2.
    let other = tempfile::tempdir().unwrap();
    fs::write(
        other.path().join("shall.toml"),
        format!(
            "[[import]]\nname = \"can\"\ngit = \"{}\"\nrev = \"v2\"\n",
            component.path().display()
        ),
    )
    .unwrap();
    git(other.path(), &["init", "-q", "-b", "master"]);
    git(other.path(), &["add", "-A"]);
    git(other.path(), &["commit", "-q", "-m", "other"]);
    let mut config = fs::read_to_string(project.path().join("shall.toml")).unwrap();
    config.push_str(&format!(
        "\n[[import]]\nname = \"other\"\ngit = \"{}\"\nrev = \"master\"\n",
        other.path().display()
    ));
    fs::write(project.path().join("shall.toml"), config).unwrap();
    let output = shall(project.path(), &["check"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output)
            .contains(" is imported at two states: as 'can' at v1 and as 'other.can' at v2"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn upgrade_check_moves_one_component() {
    let component = component();
    let project = importer(component.path(), "v1");
    let output = shall(
        project.path(),
        &["upgrade-check", "--import", "can", "--to", "v2"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "reqs.md:5: hlr~send~1 is satisfied by cap~can.send~1: revised to \
         cap~can.send~2 in can v2; to review\n"
    );
    assert_eq!(
        stderr(&output),
        "1 stale link, 1 to review, 0 updated in can v2\n"
    );
    let output = shall(
        project.path(),
        &["upgrade-check", "--import", "nope", "--to", "v2"],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("has no [[import]] named 'nope'"),
        "{}",
        stderr(&output)
    );
}

/// A component repository at `dir` with one capability of `prefix`.
fn small_component(dir: &Path, prefix: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("shall.toml"),
        "[[level]]\ntype = \"cap\"\nname = \"Capabilities\"\n",
    )
    .unwrap();
    fs::write(dir.join("caps.md"), format!("# C\n`cap~{prefix}.send~1`\n")).unwrap();
    git(dir, &["init", "-q", "-b", "master"]);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "caps"]);
    git(dir, &["tag", "v1"]);
}

#[test]
fn git_states_and_baselines_read_components_alike() {
    let component = component();
    let project = importer(component.path(), "v1");
    let root = project.path();
    git(root, &["init", "-q", "-b", "master"]);
    git(root, &["add", "shall.toml", "reqs.md"]);
    git(root, &["commit", "-q", "-m", "base"]);
    for args in [
        &["check", "--mode", "new", "--since", "HEAD"][..],
        &["diff", "HEAD"],
        &["upgrade-check", "--to", "HEAD"],
    ] {
        let output = shall(root, args);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{args:?}: {}",
            stderr(&output)
        );
    }
    // Only the project's own changes are new, with the paths of the
    // component the same in both states.
    fs::write(
        root.join("reqs.md"),
        "# Send\n`hlr~send~1`\n\nSatisfied-by:\n- cap~can.send~9\n",
    )
    .unwrap();
    let output = shall(root, &["check", "--mode", "new", "--since", "HEAD"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output).ends_with("check failed: 2 defects fail mode new\n"),
        "{}",
        stderr(&output)
    );
    assert!(
        stdout(&output)
            .lines()
            .all(|line| line.starts_with("reqs.md:")),
        "{}",
        stdout(&output)
    );
}

#[test]
fn a_branch_works_offline_and_names_share_no_cache() {
    let repos = tempfile::tempdir().unwrap();
    small_component(&repos.path().join("a"), "a");
    small_component(&repos.path().join("b"), "b");
    // One workspace, two members, both import "can" from other repositories.
    let workspace = tempfile::tempdir().unwrap();
    fs::write(
        workspace.path().join("shall.toml"),
        "[workspace]\nmembers = [\"pa\", \"pb\"]\n",
    )
    .unwrap();
    for (member, repo, rev) in [("pa", "a", "master"), ("pb", "b", "v1")] {
        let dir = workspace.path().join(member);
        fs::create_dir(&dir).unwrap();
        fs::write(
            dir.join("shall.toml"),
            format!(
                "[[level]]\ntype = \"hlr\"\nname = \"H\"\nneeds = [\"cap\"]\n\n\
                 [[import]]\nname = \"can\"\ngit = \"{}\"\nrev = \"{rev}\"\n",
                repos.path().join(repo).display()
            ),
        )
        .unwrap();
        fs::write(
            dir.join("r.md"),
            format!("# H\n`hlr~{member}~1`\nSatisfied-by:\n- cap~{repo}.send~1\n"),
        )
        .unwrap();
    }
    let output = shall(workspace.path(), &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "", "{}", stderr(&output));
    let output = shall(workspace.path(), &["check", "--offline"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), "");
    // Several runs at once share the cache.
    fs::remove_dir_all(workspace.path().join(".shall")).unwrap();
    let runs: Vec<_> = (0..3)
        .map(|_| {
            Command::new(env!("CARGO_BIN_EXE_shall"))
                .current_dir(workspace.path())
                .arg("check")
                .spawn()
                .unwrap()
        })
        .collect();
    for mut run in runs {
        assert!(run.wait().unwrap().success());
    }
}

#[cfg(unix)]
#[test]
fn components_name_components_by_url_and_never_link_out() {
    let repos = tempfile::tempdir().unwrap();
    small_component(&repos.path().join("a"), "a");
    // A component that imports another by a relative path, and one with a
    // link to a file outside of it.
    let c = repos.path().join("c");
    fs::create_dir(&c).unwrap();
    fs::write(
        c.join("shall.toml"),
        "[[import]]\nname = \"a\"\ngit = \"../a\"\nrev = \"v1\"\n",
    )
    .unwrap();
    git(&c, &["init", "-q", "-b", "master"]);
    git(&c, &["add", "-A"]);
    git(&c, &["commit", "-q", "-m", "c"]);
    let secret = repos.path().join("secret.md");
    fs::write(&secret, "# S\n`cap~secret~1`\n").unwrap();
    let s = repos.path().join("s");
    small_component(&s, "s");
    std::os::unix::fs::symlink(&secret, s.join("link.md")).unwrap();
    git(&s, &["add", "-A"]);
    git(&s, &["commit", "-q", "-m", "link"]);
    git(&s, &["tag", "v2"]);
    let project = tempfile::tempdir().unwrap();
    let write = |imports: &str| {
        fs::write(
            project.path().join("shall.toml"),
            format!("[[level]]\ntype = \"cap\"\nname = \"C\"\n\n{imports}"),
        )
        .unwrap();
    };
    write(&format!(
        "[[import]]\nname = \"c\"\ngit = \"{}\"\nrev = \"master\"\n",
        c.display()
    ));
    let output = shall(project.path(), &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("not by the relative path '../a'"),
        "{}",
        stderr(&output)
    );
    write(&format!(
        "[[import]]\nname = \"s\"\ngit = \"{}\"\nrev = \"v2\"\n",
        s.display()
    ));
    let output = shall(project.path(), &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("link.md is a symbolic link; it is left out"),
        "{}",
        stderr(&output)
    );
    assert!(!stdout(&output).contains("secret"));
    // One component under two names, and an invalid rev.
    let a = repos.path().join("a");
    write(&format!(
        "[[import]]\nname = \"x\"\ngit = \"{0}\"\nrev = \"v1\"\n\n\
         [[import]]\nname = \"y\"\ngit = \"{0}/\"\nrev = \"v1\"\n",
        a.display()
    ));
    let output = shall(project.path(), &["check"]);
    assert!(
        stderr(&output).contains("is imported twice, as 'x' and as 'y'"),
        "{}",
        stderr(&output)
    );
    write(&format!(
        "[[import]]\nname = \"x\"\ngit = \"file://{}\"\nrev = \"v1\"\n\n\
         [[import]]\nname = \"y\"\ngit = \"{}/.git\"\nrev = \"v1\"\n",
        a.display(),
        a.display()
    ));
    let output = shall(project.path(), &["check"]);
    assert!(
        stderr(&output).contains("is imported twice, as 'x' and as 'y'"),
        "{}",
        stderr(&output)
    );
    write("[[import]]\nname = \"x\"\ngit = \"file://../a\"\nrev = \"v1\"\n");
    let output = shall(project.path(), &["check"]);
    assert!(
        stderr(&output).contains("needs an absolute path"),
        "{}",
        stderr(&output)
    );
    write(&format!(
        "[[import]]\nname = \"x\"\ngit = \"{}\"\nrev = \"HEAD~1\"\n",
        a.display()
    ));
    let output = shall(project.path(), &["check"]);
    assert!(
        stderr(&output).contains("invalid 'rev' 'HEAD~1'"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_mirror_follows_branches_that_were_renamed_upstream() {
    let repos = tempfile::tempdir().unwrap();
    let a = repos.path().join("a");
    small_component(&a, "a");
    git(&a, &["branch", "release"]);
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("shall.toml"),
        format!(
            "[[level]]\ntype = \"cap\"\nname = \"C\"\n\n\
             [[import]]\nname = \"a\"\ngit = \"{}.git/\"\nrev = \"master\"\n",
            a.display()
        ),
    )
    .unwrap();
    // `a.git/` names the repository `a` for the comparison only; it is
    // fetched from as written.
    fs::rename(&a, repos.path().join("a.git")).unwrap();
    let output = shall(project.path(), &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let a = repos.path().join("a.git");
    git(&a, &["branch", "-D", "release"]);
    git(&a, &["branch", "release/1.0"]);
    let output = shall(project.path(), &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
}
