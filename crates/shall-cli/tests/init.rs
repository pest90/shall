//! Tests for `shall init`.

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

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn every_template_checks_with_the_gaps_of_its_example() {
    for (template, files, summary) in [
        (
            "simple",
            &["shall.toml", "reqs/requirements.md", "reqs/design.md"][..],
            "not ok: 2 errors, 2 warnings (2 requirements, 0 tags)\n",
        ),
        (
            "automotive",
            &[
                "shall.toml",
                "reqs/hlr.md",
                "reqs/sys.md",
                "reqs/swr.md",
                "reqs/design.md",
            ],
            "not ok: 4 errors, 2 warnings (8 requirements, 0 tags)\n",
        ),
        (
            "platform",
            &[
                "shall.toml",
                "reqs/capabilities.md",
                "reqs/software.md",
                "reqs/design.md",
            ],
            "not ok: 2 errors, 2 warnings (3 requirements, 0 tags)\n",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let output = shall(dir.path(), &["init", "new", "--template", template]);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        assert_eq!(stderr(&output), "");
        let created: Vec<String> = files
            .iter()
            .map(|file| format!("created new/{file}\n"))
            .collect();
        assert_eq!(stdout(&output), created.concat());
        // Only the design decisions miss code and tests.
        let output = shall(&dir.path().join("new"), &["check"]);
        assert_eq!(stderr(&output), summary, "{template}");
        let defects = stdout(&output);
        let errors: Vec<&str> = defects
            .lines()
            .filter(|line| !line.contains("warning:"))
            .map(|line| line.rsplit_once(": ").unwrap().1)
            .collect();
        assert!(
            errors
                .iter()
                .all(|error| error.starts_with("not covered by required type ")),
            "{template}: {errors:?}"
        );
    }
}

#[test]
fn a_project_imports_the_platform_of_its_workspace() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("shall.toml"),
        "[workspace]\nmembers = [\"platform\", \"projects/*\"]\n",
    )
    .unwrap();
    // Until a project exists, `shall check` cannot run in the workspace.
    let output = shall(dir.path(), &["init", "platform", "--template", "platform"]);
    assert!(
        stderr(&output).starts_with("warning: member 'projects/*' of "),
        "{}",
        stderr(&output)
    );
    let output = shall(dir.path(), &["init", "projects/a", "--template", "project"]);
    assert_eq!(stderr(&output), "");
    assert!(
        fs::read_to_string(dir.path().join("projects/a/shall.toml"))
            .unwrap()
            .contains("path = \"../../platform\"")
    );
    let output = shall(dir.path(), &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stderr(&output).ends_with("workspace: not ok: 3 errors, 2 warnings (2 projects)\n"),
        "{}",
        stderr(&output)
    );
    // Elsewhere, the paths are wrong.
    let root = dunce::canonicalize(dir.path()).unwrap();
    let output = shall(
        dir.path(),
        &[
            "init",
            "other",
            "--template",
            "project",
            "--platform",
            "../plat",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stderr(&output),
        format!(
            "warning: {} is not a member of the workspace {}; add it to [workspace] members\n\
             warning: no shall.toml at ../plat relative to other; create the platform there \
             (`shall init --template platform`) or fix the [[import]] path in shall.toml\n",
            root.join("other").display(),
            root.join("shall.toml").display()
        )
    );
}

#[test]
fn a_project_outside_of_a_workspace_gets_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let output = shall(dir.path(), &["init", "--template", "project"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stderr(&output),
        "warning: . is in no workspace, but a project imports only members of its own \
         workspace; list both in the [workspace] members of a shall.toml above\n\
         warning: no shall.toml at ../../platform relative to .; create the platform there \
         (`shall init --template platform`) or fix the [[import]] path in shall.toml\n"
    );
    let output = shall(
        dir.path(),
        &["init", "b", "--template", "project", "--platform", "."],
    );
    assert_eq!(
        stderr(&output),
        "Error: --platform must name another directory than the project\n"
    );
    let output = shall(
        dir.path(),
        &[
            "init",
            "d",
            "--template",
            "project",
            "--platform",
            "reqs/..",
        ],
    );
    assert!(
        stderr(&output).ends_with(
            "warning: --platform reqs/.. is the new project itself, which cannot import \
             itself; fix the [[import]] path in shall.toml\n"
        ),
        "{}",
        stderr(&output)
    );
    // The path is a TOML string.
    let output = shall(
        dir.path(),
        &[
            "init",
            "c",
            "--template",
            "project",
            "--platform",
            "..\\\"p",
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let config = fs::read_to_string(dir.path().join("c/shall.toml")).unwrap();
    assert!(config.contains("path = \"..\\\\\\\"p\"\n"), "{config}");
}

#[cfg(unix)]
#[test]
fn never_writes_through_a_link_or_half_a_project() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("a/reqs")).unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("outside"),
        dir.path().join("a/reqs/design.md"),
    )
    .unwrap();
    let output = shall(dir.path(), &["init", "a"]);
    assert_eq!(stderr(&output), "Error: a/reqs/design.md exists already\n");
    assert!(!dir.path().join("outside").exists());
    assert!(!dir.path().join("a/shall.toml").exists());
    // A file where a directory belongs.
    fs::create_dir(dir.path().join("b")).unwrap();
    fs::write(dir.path().join("b/reqs"), "").unwrap();
    let output = shall(dir.path(), &["init", "b"]);
    assert_eq!(stderr(&output), "Error: b/reqs is not a directory\n");
    assert!(!dir.path().join("b/shall.toml").exists());
    // An error while writing removes what was written.
    fs::create_dir_all(dir.path().join("c/reqs")).unwrap();
    let mut permissions = fs::metadata(dir.path().join("c/reqs"))
        .unwrap()
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o555);
    fs::set_permissions(dir.path().join("c/reqs"), permissions).unwrap();
    let output = shall(dir.path(), &["init", "c"]);
    if output.status.code() == Some(2) {
        assert!(!dir.path().join("c/shall.toml").exists());
    } // else running as root, which may write anyway
}

#[test]
fn never_overwrites_a_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("reqs")).unwrap();
    fs::write(dir.path().join("reqs/design.md"), "mine").unwrap();
    let output = shall(dir.path(), &["init"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stderr(&output), "Error: ./reqs/design.md exists already\n");
    // Nothing was written.
    assert!(!dir.path().join("shall.toml").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("reqs/design.md")).unwrap(),
        "mine"
    );
    let output = shall(dir.path(), &["init", "--platform", "p"]);
    assert_eq!(
        stderr(&output),
        "Error: --platform is only for --template project\n"
    );
}
