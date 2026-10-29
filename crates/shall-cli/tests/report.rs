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
    let mut wrote = String::new();
    for file in ["index.html", "trace.json", "defects.txt", "junit.xml"] {
        wrote.push_str(&format!("wrote {}\n", target.join(file).display()));
    }
    assert_eq!(
        stderr(&output),
        format!("{wrote}not ok: 4 errors, 0 warnings (9 requirements, 7 tags, 3 test results)\n")
    );
    // defects.txt is what `shall check` prints.
    assert_eq!(
        fs::read_to_string(target.join("defects.txt")).unwrap(),
        fs::read_to_string(fixture().join("expected-defects.txt")).unwrap()
    );
    insta::assert_snapshot!(
        "junit_xml",
        fs::read_to_string(target.join("junit.xml")).unwrap()
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

#[test]
fn writes_a_report_per_variant_and_an_overview() {
    let out = tempfile::tempdir().unwrap();
    let site = out.path().join("site");
    let output = shall(
        &fixture(),
        &["report", "--out", site.to_str().unwrap(), "--all-variants"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let mut expected = String::new();
    for variant in ["basis", "safety"] {
        for file in ["index.html", "trace.json", "defects.txt", "junit.xml"] {
            expected.push_str(&format!(
                "wrote {}\n",
                site.join(variant).join(file).display()
            ));
        }
    }
    expected.push_str(&format!("wrote {}\n", site.join("index.html").display()));
    expected.push_str(
        "variant basis: not ok: 4 errors, 0 warnings (5 requirements, 5 tags, 3 test results)\n\
         variant safety: not ok: 4 errors, 0 warnings (9 requirements, 7 tags, 3 test results)\n",
    );
    assert_eq!(stderr(&output), expected);
    let overview = fs::read_to_string(site.join("index.html")).unwrap();
    insta::assert_snapshot!(
        "overview_html",
        overview.replace(
            &format!("shall {}", env!("CARGO_PKG_VERSION")),
            "shall [version]"
        )
    );
    let basis = fs::read_to_string(site.join("basis").join("trace.json")).unwrap();
    assert!(
        basis.contains("\"variant\": {\n    \"name\": \"basis\",\n    \"features\": []\n  },"),
        "{basis}"
    );
    assert!(!basis.contains("emergency-stop"), "{basis}");
    let safety = fs::read_to_string(site.join("safety").join("index.html")).unwrap();
    assert!(
        safety.contains(
            "<p class=\"variant\">Variant <strong>safety</strong>: <code>estop</code></p>"
        ),
        "{safety}"
    );

    // One variant writes into the output directory itself.
    let one = out.path().join("one");
    let output = shall(
        &fixture(),
        &[
            "report",
            "--out",
            one.to_str().unwrap(),
            "--variant",
            "basis",
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(fs::read_to_string(one.join("trace.json")).unwrap(), basis);
}

#[test]
fn all_variants_needs_variants() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("shall.toml"), "").unwrap();
    let output = shall(dir.path(), &["report", "--all-variants"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "Error: --all-variants: shall.toml has no [variant.*]\n"
    );
}

#[test]
fn variant_and_all_variants_exclude_each_other() {
    let output = shall(
        &fixture(),
        &["report", "--variant", "basis", "--all-variants"],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("'--variant <VARIANT>' cannot be used with '--all-variants'"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn links_locations_to_the_code_at_the_commit() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("shall.toml"),
        "[[level]]\ntype = \"dsn\"\nname = \"D\"\nneeds = [\"impl\"]\n\n\
         [sources]\nimpl = [\"src/*.c\"]\n\n\
         [report]\ncode_url = \"https://git.example.org/p/src/commit/{sha}/{path}#L{line}\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("reqs")).unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("reqs/a.md"), "# A\n`dsn~a-k4f2~1`\n").unwrap();
    fs::write(
        dir.path().join("src/a.c"),
        "// [impl->dsn~a-k4f2~1]\nvoid f();\n",
    )
    .unwrap();
    let site = dir.path().join("site");
    let output = shall(
        dir.path(),
        &[
            "report",
            "--out",
            site.to_str().unwrap(),
            "--build-info",
            "sha=abc123,date=2026-10-29",
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let html = fs::read_to_string(site.join("index.html")).unwrap();
    assert!(
        html.contains("<a href=\"https:&#x2f;&#x2f;git.example.org&#x2f;p&#x2f;src&#x2f;commit&#x2f;abc123&#x2f;src&#x2f;a.c#L1\"><code>src&#x2f;a.c:1</code></a>"),
        "{html}"
    );
    assert!(
        html.contains("<p class=\"build\">Commit <code>abc123</code>, 2026-10-29</p>"),
        "{html}"
    );
    let json = fs::read_to_string(site.join("trace.json")).unwrap();
    assert!(json.contains("\"sha\": \"abc123\""), "{json}");

    let output = shall(dir.path(), &["report", "--build-info", "commit=x"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("unknown key 'commit'"),
        "{}",
        stderr(&output)
    );
}
