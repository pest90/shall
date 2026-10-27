//! Tests for `shall report`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn shall(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shall"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/motor")
}

/// `trace.json` with the version of `shall` replaced, so that a release
/// does not change the snapshot.
fn normalized(json: &str) -> String {
    let version = format!("\"version\": \"{}\"", env!("CARGO_PKG_VERSION"));
    assert!(json.contains(&version), "{json}");
    json.replacen(&version, "\"version\": \"[version]\"", 1)
}

#[test]
fn writes_the_report_of_the_fixture() {
    let out = tempfile::tempdir().unwrap();
    let target = out.path().join("site").join("nested");
    let output = shall(&fixture(), &["report", "--out", target.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(output.stdout.is_empty());
    let index_html = target.join("index.html");
    let trace_json = target.join("trace.json");
    assert_eq!(
        stderr(&output),
        format!(
            "wrote {}\nwrote {}\n\
             not ok: 4 errors, 0 warnings (8 requirements, 7 tags, 3 test results)\n",
            index_html.display(),
            trace_json.display()
        )
    );
    let json = fs::read_to_string(&trace_json).unwrap();
    insta::assert_snapshot!("trace_json", normalized(&json));
    let html = fs::read_to_string(&index_html).unwrap();
    insta::assert_snapshot!(
        "index_html",
        html.replace(
            &format!("shall {}", env!("CARGO_PKG_VERSION")),
            "shall [version]"
        )
    );

    // Deterministic: a second run from another directory writes the same.
    let again = out.path().join("again");
    let output = shall(
        &fixture().join("src"),
        &["report", "--out", again.to_str().unwrap()],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(fs::read_to_string(again.join("trace.json")).unwrap(), json);
    assert_eq!(fs::read_to_string(again.join("index.html")).unwrap(), html);
}

#[test]
fn output_directory_is_relative_to_the_current_directory() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    fs::create_dir_all(project.join("reqs")).unwrap();
    fs::write(project.join("shall.toml"), "").unwrap();
    fs::write(
        project.join("reqs").join("a.md"),
        "# A\n\n`swr~a-k4f2~1`\n\nText.\n",
    )
    .unwrap();
    let output = shall(dir.path(), &["report", "project", "--out", "site"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let json = fs::read_to_string(dir.path().join("site").join("trace.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["items"][0]["id"], "swr~a-k4f2~1");
    assert_eq!(value["items"][0]["test"], serde_json::Value::Null);
    // Without `junit` sources there are no test results, not an empty list.
    assert_eq!(value["test_results"], serde_json::Value::Null);
    assert_eq!(value["defects"], serde_json::json!([]));
    assert!(!project.join("site").exists());
}

#[test]
fn fails_if_the_output_cannot_be_written() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("shall.toml"), "").unwrap();
    fs::write(dir.path().join("site"), "a file").unwrap();
    let output = shall(dir.path(), &["report", "--out", "site"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).starts_with("Error: cannot create site"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn fails_without_project() {
    let dir = tempfile::tempdir().unwrap();
    let output = shall(dir.path(), &["report"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!dir.path().join("site").exists());
}
