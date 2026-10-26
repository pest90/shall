//! Tests for `shall new`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use shall_core::{ItemId, create, markdown};

const CONFIG: &str = r#"
[[level]]
type  = "swr"
name  = "Software"
needs = ["dsn"]

[[level]]
type  = "dsn"
name  = "Design"
needs = ["impl", "utest"]
"#;

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("shall.toml"), CONFIG).unwrap();
    fs::create_dir(dir.path().join("reqs")).unwrap();
    dir
}

fn shall(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shall"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn assert_generated(id: &ItemId, typ: &str, base: &str) {
    assert_eq!(id.typ().as_str(), typ);
    assert_eq!(id.revision(), 1);
    let random = id
        .name()
        .strip_prefix(&format!("{base}-"))
        .unwrap_or_else(|| panic!("{id} does not start with {base}-"));
    assert_eq!(random.len(), 4, "{id}");
    assert!(
        random.bytes().all(|b| create::ALPHABET.contains(&b)),
        "{id}"
    );
}

#[test]
fn prints_id_without_file() {
    let dir = project();
    let output = shall(
        dir.path(),
        &["new", "swr", "Overtemp shutdown", "--component", "motor"],
    );
    let id: ItemId = stdout(&output).trim().parse().unwrap();
    assert_generated(&id, "swr", "motor.overtemp-shutdown");
}

#[test]
fn appends_skeletons_to_file() {
    let dir = project();
    let first = stdout(&shall(
        dir.path(),
        &["new", "dsn", "Temp monitor", "--file", "reqs/design.md"],
    ));
    let second = stdout(&shall(
        dir.path(),
        &["new", "dsn", "Temp monitor", "--file", "reqs/design.md"],
    ));

    let text = fs::read_to_string(dir.path().join("reqs/design.md")).unwrap();
    let parsed = markdown::parse(Path::new("reqs/design.md"), &text).unwrap();
    let [a, b] = &parsed.items[..] else {
        panic!("expected two items in:\n{text}")
    };
    assert_ne!(a.id, b.id);
    assert_generated(&a.id, "dsn", "temp-monitor");
    assert_eq!(first, format!("{}: created {}\n", a.location, a.id));
    assert_eq!(second, format!("{}: created {}\n", b.location, b.id));
    assert_eq!(a.location.line(), 2);
    assert_eq!(a.title, "Temp monitor");
    let needs: Vec<_> = a.needs.iter().map(|t| t.as_str()).collect();
    assert_eq!(needs, ["impl", "utest"]);
}

#[test]
fn appends_after_text_without_final_newline() {
    let dir = project();
    fs::write(dir.path().join("reqs/software.md"), "# Software").unwrap();
    let output = stdout(&shall(
        dir.path(),
        &["new", "swr", "X", "--file", "reqs/software.md"],
    ));
    let text = fs::read_to_string(dir.path().join("reqs/software.md")).unwrap();
    assert!(text.starts_with("# Software\n\n## X\n`swr~x-"), "{text}");
    assert!(
        output.starts_with("reqs/software.md:4: created swr~x-"),
        "{output}"
    );
    // Top level: no parent to cover.
    assert!(!text.contains("Covers:"), "{text}");
}

#[test]
fn finds_project_from_subdirectory() {
    let dir = project();
    let output = stdout(&shall(
        &dir.path().join("reqs"),
        &["new", "swr", "X", "--file", "software.md"],
    ));
    assert!(
        output.starts_with("reqs/software.md:2: created swr~x-"),
        "{output}"
    );
}

#[test]
fn accepts_parent_paths_and_rejects_files_outside() {
    let dir = project();
    fs::create_dir(dir.path().join("src")).unwrap();
    let output = stdout(&shall(
        &dir.path().join("src"),
        &["new", "swr", "X", "--file", "../reqs/./software.md"],
    ));
    assert!(
        output.starts_with("reqs/software.md:2: created swr~x-"),
        "{output}"
    );

    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("x.md");
    let output = shall(
        dir.path(),
        &["new", "swr", "X", "--file", target.to_str().unwrap()],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("is outside of the project"), "{stderr}");
    assert!(
        !target.exists(),
        "nothing may be written outside the project"
    );
}

#[test]
fn reports_unknown_type_and_missing_config() {
    let dir = project();
    let output = shall(dir.path(), &["new", "req", "X"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unknown type 'req'"), "{stderr}");
    assert!(stderr.contains("swr, dsn"), "{stderr}");

    let empty = tempfile::tempdir().unwrap();
    let output = shall(empty.path(), &["new", "swr", "X"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("no shall.toml found"), "{stderr}");
}
