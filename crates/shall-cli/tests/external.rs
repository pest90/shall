//! Tests for external subcommands `shall-<name>`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn shall(dir: &Path, path: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shall"))
        .current_dir(dir)
        .env("PATH", path)
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

#[cfg(unix)]
#[test]
fn runs_a_program_from_the_path_with_the_project() {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    let script = bin.path().join("shall-hello");
    fs::write(
        &script,
        "#!/bin/sh\necho \"args: $*\"\necho \"project: $SHALL_PROJECT_ROOT\"\n\
         echo \"workspace: $SHALL_WORKSPACE_ROOT\"\necho \"config: $SHALL_CONFIG\"\n\
         echo \"version: $SHALL_VERSION\"\ntest -x \"$SHALL\" || exit 9\nexit 3\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("shall.toml"), "").unwrap();
    fs::create_dir(project.path().join("sub")).unwrap();
    let root = dunce::canonicalize(project.path()).unwrap();
    let output = shall(
        &project.path().join("sub"),
        bin.path(),
        &["hello", "a", "--b"],
    );
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "args: a --b\nproject: {root}\nworkspace: {root}\nconfig: {root}/shall.toml\n\
             version: {}\n",
            env!("CARGO_PKG_VERSION"),
            root = root.display()
        )
    );
    // Outside of a project, without its variables.
    let elsewhere = tempfile::tempdir().unwrap();
    let output = shall(elsewhere.path(), bin.path(), &["hello"]);
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("project: \n"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn an_unknown_command_names_the_program_it_looked_for() {
    let dir = tempfile::tempdir().unwrap();
    let output = shall(dir.path(), dir.path(), &["nothing-here"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "Error: unknown command 'nothing-here': no program shall-nothing-here in PATH; \
         see `shall --help`\n"
    );
}

#[test]
fn suggests_a_built_in_command_for_a_typo() {
    let dir = tempfile::tempdir().unwrap();
    let output = shall(dir.path(), dir.path(), &["chek"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("; did you mean 'check'?"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn rejects_odd_names_and_suggests_help() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["sub/x", "", "-x"] {
        let output = shall(dir.path(), dir.path(), &["--", name]);
        assert_eq!(output.status.code(), Some(2), "{name}");
    }
    let output = shall(dir.path(), dir.path(), &["hel"]);
    assert!(
        stderr(&output).contains("did you mean 'help'?"),
        "{}",
        stderr(&output)
    );
    let output = shall(dir.path(), dir.path(), &["rep"]);
    assert!(
        !stderr(&output).contains("did you mean"),
        "{}",
        stderr(&output)
    );
}

#[cfg(unix)]
#[test]
fn passes_signals_and_the_workspace_on() {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    let script = bin.path().join("shall-env");
    fs::write(
        &script,
        "#!/bin/sh\necho \"workspace: $SHALL_WORKSPACE_ROOT\"\n\
         test -n \"$1\" && kill -TERM $$\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let workspace = tempfile::tempdir().unwrap();
    fs::write(
        workspace.path().join("shall.toml"),
        "[workspace]\nmembers = [\"a\"]\n",
    )
    .unwrap();
    fs::create_dir(workspace.path().join("a")).unwrap();
    fs::write(workspace.path().join("a/shall.toml"), "").unwrap();
    let root = dunce::canonicalize(workspace.path()).unwrap();
    let output = shall(&workspace.path().join("a"), bin.path(), &["env"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stdout(&output), format!("workspace: {}\n", root.display()));
    let output = shall(&workspace.path().join("a"), bin.path(), &["env", "kill"]);
    assert_eq!(output.status.code(), Some(128 + 15));
}
