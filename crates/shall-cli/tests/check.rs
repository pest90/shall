//! Tests for `shall check`.

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

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/motor")
}

#[test]
fn reports_exactly_the_defects_of_the_fixture() {
    let expected = fs::read_to_string(fixture().join("expected-defects.txt")).unwrap();
    // From the project root, from a directory inside it, and with a path.
    for (dir, args) in [
        (fixture(), &["check"][..]),
        (fixture().join("src"), &["check"]),
        (fixture().join(".."), &["check", "motor"]),
    ] {
        let output = shall(&dir, args);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        assert_eq!(stdout(&output), expected);
        assert_eq!(
            stderr(&output),
            "not ok: 4 errors, 0 warnings (9 requirements, 7 tags, 3 test results)\n"
        );
    }
    let output = shall(&fixture(), &["check", "--mode", "strict"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), expected);
}

fn project(config: &str, files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("shall.toml"), config).unwrap();
    for (path, content) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    dir
}

const LEVELS: &str = r#"
[[level]]
type  = "dsn"
name  = "Design"
needs = ["impl", "utest"]

[verification]
types = ["utest"]
"#;

const DESIGN: &[u8] = b"# Design\n\n## A\n`dsn~a~1`\n\nText.\n";

#[test]
fn reports_problems_with_the_sources() {
    let config = format!(
        "{LEVELS}
[sources]
impl  = [\"src/**/*.c\", \"lib/**\"]
utset = [\"tests/*.c\", \"src/a.c\"]
exclude = \"build/**\"
junit = [\"build/*.xml\"]
"
    );
    // Not a Git repository: .gitignore applies anyway.
    let dir = project(
        &config,
        &[
            ("reqs/design.md", DESIGN),
            (
                "README.md",
                b"Not a requirement, but read: all Markdown files are.\n",
            ),
            ("src/a.c", b"// [impl->dsn~a~1]\nvoid a() {}\n"),
            ("src/latin1.c", b"// caf\xe9\n"),
            ("tests/a.c", b"// [utest->dsn~a~1]\nTEST(A, Works) {}\n"),
            (
                "tests/b.c",
                b"// [utest->dsn~a~1]\nTEST(A, Missing) {}\n// [utest->dsn~a~1]\nint helper() {}\n",
            ),
            (
                "build/out.xml",
                b"<testsuite><testcase classname=\"A\" name=\"Works\"/></testsuite>",
            ),
            ("src/gen/x.c", b"// [impl->dsn~ignored~1]\n"),
            (".gitignore", b"build/\nsrc/gen/\n"),
        ],
    );
    let output = shall(dir.path(), &["check"]);
    // src/a.c, matched by two keys, is read once: no duplicate IDs.
    // Ignored by Git: src/gen/x.c is not read, build/out.xml is.
    assert_eq!(
        stdout(&output),
        "\
shall.toml:11: warning: [sources] pattern 'lib/**' of 'impl' matches no file
shall.toml:12: warning: [sources] key 'utset' is not a known type; the type of an item comes from its tag
shall.toml:13: warning: [sources] key 'exclude' is not a list of patterns and is ignored
src/latin1.c:1: not UTF-8, skipped
tests/b.c:1: test A.Missing was not executed: no result in the JUnit files
tests/b.c:3: warning: tag [utest->dsn~a~1]: no test definition follows the tag
"
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        stderr(&output),
        "not ok: 2 errors, 4 warnings (1 requirement, 4 tags, 1 test result)\n"
    );

    // Without JUnit files configured, tests are not matched; warnings
    // alone fail in strict mode.
    let config = config.replace("junit = [\"build/*.xml\"]", "");
    fs::write(dir.path().join("shall.toml"), config).unwrap();
    fs::remove_file(dir.path().join("src/latin1.c")).unwrap();
    let output = shall(dir.path(), &["check", "--mode", "strict"]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert_eq!(
        stdout(&output),
        "\
shall.toml:11: warning: [sources] pattern 'lib/**' of 'impl' matches no file
shall.toml:12: warning: [sources] key 'utset' is not a known type; the type of an item comes from its tag
shall.toml:13: warning: [sources] key 'exclude' is not a list of patterns and is ignored
"
    );
    assert_eq!(
        stderr(&output),
        "ok: 0 errors, 3 warnings (1 requirement, 4 tags)\n"
    );
}

#[test]
fn reports_invalid_patterns_and_reads_the_others() {
    let dir = project(
        "[sources]\nimpl = [\"src/[\", \"src/*.c\"]\n",
        &[("src/a.c", b"// [impl->dsn~a~1]\n")],
    );
    let output = shall(dir.path(), &["check"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = stdout(&output);
    assert!(
        stdout.starts_with("shall.toml:2: [sources] invalid pattern 'src/[' of 'impl': "),
        "{stdout}"
    );
    assert!(
        stdout.contains("src/a.c:1: tag [impl->dsn~a~1]: unknown target dsn~a~1"),
        "{stdout}"
    );
}

#[cfg(unix)]
#[test]
fn follows_symbolic_links() {
    use std::os::unix::fs::symlink;

    let config = format!(
        "{LEVELS}\n[sources]\nrequirements = [\"reqs/*.md\"]\nimpl = [\"src/*.c\"]\njunit = [\"**/*.xml\"]\n"
    );
    let dir = project(
        &config,
        &[
            ("real/design.md", DESIGN),
            ("real/a.c", b"// [impl->dsn~a~1]\n// [utest->dsn~a~1]\nTEST(A, Works) {}\n"),
            (
                "real/out.xml",
                b"<testsuite><testcase classname=\"A\" name=\"Works\"><failure/></testcase></testsuite>",
            ),
            (".gitignore", b"real/\n"),
        ],
    );
    fs::create_dir(dir.path().join("reqs")).unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    symlink("../real/design.md", dir.path().join("reqs/design.md")).unwrap();
    symlink("../real/a.c", dir.path().join("src/a.c")).unwrap();
    symlink("real/out.xml", dir.path().join("out.xml")).unwrap();
    let output = shall(dir.path(), &["check"]);
    // The JUnit pattern without a directory follows .gitignore, so
    // real/out.xml is not read twice.
    assert_eq!(stdout(&output), "src/a.c:2: test A.Works failed\n");
    assert_eq!(
        stderr(&output),
        "not ok: 1 error, 0 warnings (1 requirement, 2 tags, 1 test result)\n"
    );
}

#[test]
fn matches_rust_tests_of_their_own_package() {
    let config = format!(
        "{LEVELS}\n[sources]\nutest = [\"crates/*/src/**/*.rs\"]\njunit = [\"target/nextest/*.xml\"]\n"
    );
    let test = b"#[cfg(test)]\nmod tests {\n    // [utest->dsn~a~1]\n    #[test]\n    fn it_works() {}\n}\n";
    let dir = project(
        &config,
        &[
            ("reqs/design.md", DESIGN),
            ("Cargo.toml", b"[workspace]\nmembers = [\"crates/*\"]\n"),
            ("crates/a/Cargo.toml", b"[package]\nname = \"proj-a\"\n"),
            ("crates/a/src/lib.rs", test),
            ("crates/b/Cargo.toml", b"[package]\nname = \"proj-b\"\n"),
            ("crates/b/src/lib.rs", test),
            (
                "target/nextest/junit.xml",
                b"<testsuites><testsuite name=\"proj-a\">\
                  <testcase classname=\"proj-a\" name=\"tests::it_works\"><failure/></testcase>\
                  </testsuite></testsuites>",
            ),
            (".gitignore", b"target/\n"),
        ],
    );
    // Without the package names, crate b would take the result of proj-a.
    let output = shall(dir.path(), &["check"]);
    assert_eq!(
        stdout(&output),
        "\
crates/a/src/lib.rs:3: test tests::it_works failed
crates/b/src/lib.rs:3: test tests::it_works was not executed: no result in the JUnit files
reqs/design.md:4: dsn~a~1: not covered by required type 'impl'
"
    );
}

#[test]
fn fails_without_project_or_with_invalid_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let output = shall(dir.path(), &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("no shall.toml found"),
        "{}",
        stderr(&output)
    );

    let dir = project("[[level]]\ntype = 1\n", &[]);
    let output = shall(dir.path(), &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("invalid shall.toml"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn ignore_rules_above_the_work_tree_do_not_apply() {
    let outer = tempfile::tempdir().unwrap();
    fs::write(outer.path().join(".gitignore"), "*\n").unwrap();
    let dir = outer.path().join("proj");
    fs::create_dir_all(dir.join("reqs")).unwrap();
    fs::write(dir.join("shall.toml"), LEVELS).unwrap();
    fs::write(dir.join("reqs/design.md"), DESIGN).unwrap();
    fs::write(dir.join(".gitignore"), "reqs/draft.md\n").unwrap();
    fs::write(dir.join("reqs/draft.md"), "`dsn~b~1`\n").unwrap();
    let expected = "not ok: 2 errors, 0 warnings (1 requirement, 0 tags)\n";
    // Outside of a work tree, and in a work tree at the project root.
    let output = shall(&dir, &["check"]);
    assert_eq!(stderr(&output), expected, "{}", stdout(&output));
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&dir)
        .status()
        .unwrap();
    assert!(git.success());
    let output = shall(&dir, &["check"]);
    assert_eq!(stderr(&output), expected, "{}", stdout(&output));
}

#[cfg(unix)]
#[test]
fn reads_linked_directories_once_and_reports_loops() {
    use std::os::unix::fs::symlink;

    let config = format!("{LEVELS}\n[sources]\nrequirements = [\"**/*.md\"]\n");
    let dir = project(&config, &[("reqs/design.md", DESIGN)]);
    fs::create_dir(dir.path().join("docs")).unwrap();
    symlink("../reqs", dir.path().join("docs/current")).unwrap();
    symlink("..", dir.path().join("reqs/loop")).unwrap();
    // reqs/design.md once, not again as docs/current/design.md; the loop
    // on each way it is reached.
    let output = shall(dir.path(), &["check"]);
    assert_eq!(
        stdout(&output),
        "\
docs/current/loop:1: warning: symbolic link loop; skipped
reqs/design.md:4: dsn~a~1: not covered by required type 'impl'
reqs/design.md:4: dsn~a~1: not covered by required type 'utest'
reqs/loop:1: warning: symbolic link loop; skipped
"
    );
}

#[cfg(unix)]
#[test]
fn selects_before_dropping_linked_copies() {
    use std::os::unix::fs::symlink;

    let config = format!(
        "{LEVELS}\n[sources]\nrequirements = [\"docs/**/*.md\"]\nimpl = [\"src/*.c\"]\n\
         junit = [\"build/*.xml\", \"results/*.xml\"]\n"
    );
    let dir = project(
        &config,
        &[
            ("reqs/design.md", DESIGN),
            ("src/a.c", b"// [impl->dsn~a~1]\n"),
            (
                "build/out.xml",
                b"<testsuite><testcase classname=\"A\" name=\"B\"/></testsuite>",
            ),
            (".gitignore", b"src/ignored.c\n"),
        ],
    );
    fs::create_dir(dir.path().join("docs")).unwrap();
    // A hard link is the same file: read once, under the first path.
    fs::hard_link(dir.path().join("src/a.c"), dir.path().join("src/b.c")).unwrap();
    symlink("../reqs", dir.path().join("docs/current")).unwrap();
    symlink("build", dir.path().join("results")).unwrap();
    // Broken links: reported only when selected.
    symlink("missing.c", dir.path().join("src/gone.c")).unwrap();
    symlink("missing.c", dir.path().join("src/ignored.c")).unwrap();
    symlink(
        "build/compile_commands.json",
        dir.path().join("compile_commands.json"),
    )
    .unwrap();
    let output = shall(dir.path(), &["check"]);
    // The requirement is read through the link, the JUnit file once.
    assert_eq!(
        stdout(&output),
        "\
docs/current/design.md:4: dsn~a~1: not covered by required type 'utest'
src/gone.c:1: warning: broken symbolic link; skipped
"
    );
    assert_eq!(
        stderr(&output),
        "not ok: 1 error, 1 warning (1 requirement, 1 tag, 1 test result)\n"
    );
}

#[cfg(unix)]
#[test]
fn nested_repositories_keep_their_own_ignore_rules() {
    use std::os::unix::fs::symlink;

    let config = format!("{LEVELS}\n[sources]\nimpl = [\"src/**/*.c\"]\n");
    let dir = project(
        &config,
        &[("reqs/design.md", DESIGN), (".gitignore", b"gone.c\n")],
    );
    for repo in [dir.path().to_path_buf(), dir.path().join("src/vendor")] {
        fs::create_dir_all(&repo).unwrap();
        let git = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(git.success());
    }
    symlink("missing.c", dir.path().join("src/gone.c")).unwrap();
    symlink("missing.c", dir.path().join("src/vendor/gone.c")).unwrap();
    // Outside of a work tree, the project's rules apply everywhere.
    let outside = project(
        &config,
        &[("reqs/design.md", DESIGN), (".gitignore", b"gone.c\n")],
    );
    fs::create_dir_all(outside.path().join("src/vendor/.git")).unwrap();
    symlink("missing.c", outside.path().join("src/vendor/gone.c")).unwrap();
    let output = shall(outside.path(), &["check"]);
    assert!(!stdout(&output).contains("gone.c"), "{}", stdout(&output));

    let output = shall(dir.path(), &["check"]);
    // Git ignores src/gone.c, but not the link in the nested repository.
    assert!(
        !stdout(&output).contains("src/gone.c"),
        "{}",
        stdout(&output)
    );
    assert!(
        stdout(&output).contains("src/vendor/gone.c:1: warning: broken symbolic link; skipped"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn checks_one_variant() {
    let expected = fs::read_to_string(fixture().join("expected-defects.txt")).unwrap();
    for (variant, summary) in [
        (
            "basis",
            "variant basis: not ok: 4 errors, 0 warnings (5 requirements, 5 tags, 3 test results)\n",
        ),
        (
            "safety",
            "variant safety: not ok: 4 errors, 0 warnings (9 requirements, 7 tags, 3 test results)\n",
        ),
    ] {
        let output = shall(&fixture(), &["check", "--variant", variant]);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        assert_eq!(stdout(&output), expected);
        assert_eq!(stderr(&output), summary);
    }
    let output = shall(&fixture(), &["check", "--variant", "nope"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "Error: unknown variant 'nope'; shall.toml defines basis, safety\n"
    );
}

#[test]
fn variants_take_their_own_test_results() {
    let config = format!(
        "{LEVELS}
[sources]
requirements = [\"reqs/*.md\"]
utest = [\"tests/*.c\"]
junit = [\"build/all/*.xml\"]

[variant.a]
features = [\"x\"]
junit = [\"build/a/*.xml\"]

[variant.b]
features = [\"x\", \"y\"]
junits = []
"
    );
    let failed = b"<testsuite name=\"S\"><testcase classname=\"S\" name=\"T\"><failure/></testcase></testsuite>";
    let passed = b"<testsuite name=\"S\"><testcase classname=\"S\" name=\"T\"/></testsuite>";
    let dir = project(
        &config,
        &[
            (
                "reqs/design.md",
                b"# A\n`dsn~a~1`\nNeeds: utest\n\n# B\n`dsn~b~1`\nNeeds: utest\nFeatures: y, z\n",
            ),
            ("tests/a.c", b"// [utest->dsn~a~1]\nTEST(S, T) {}\n"),
            ("build/all/r.xml", failed),
            ("build/a/r.xml", passed),
        ],
    );
    let output = shall(dir.path(), &["check", "--variant", "a"]);
    assert_eq!(
        stdout(&output),
        "reqs/design.md:6: warning: dsn~b~1: feature 'z' is selected by no variant\n\
         shall.toml:21: warning: [variant.b] key 'junits' is unknown and is ignored\n"
    );
    let output = shall(dir.path(), &["check", "--variant", "b"]);
    assert_eq!(
        stdout(&output),
        "reqs/design.md:6: warning: dsn~b~1: feature 'z' is selected by no variant\n\
         shall.toml:21: warning: [variant.b] key 'junits' is unknown and is ignored\n\
         tests/a.c:1: test S.T failed\n"
    );
    let output = shall(dir.path(), &["check"]);
    assert_eq!(
        stdout(&output),
        "reqs/design.md:6: warning: dsn~b~1: feature 'z' is selected by no variant\n\
         reqs/design.md:6: dsn~b~1: not covered by required type 'utest'\n\
         shall.toml:21: warning: [variant.b] key 'junits' is unknown and is ignored\n\
         tests/a.c:1: test S.T failed\n"
    );
}

#[test]
fn results_of_tests_outside_the_variant_never_count() {
    let config = format!(
        "{LEVELS}
[[level]]
type  = \"hlr\"
name  = \"Customer\"
needs = [\"dsn\"]

[satisfied_by]
targets = [\"dsn\"]

[sources]
requirements = [\"reqs/*.md\"]
utest = [\"tests/*.cpp\"]
impl = [\"src/*.c\"]
junit = [\"build/*.xml\"]

[variant.basis]
features = []

[variant.full]
features = [\"x\"]
"
    );
    let dir = project(
        &config,
        &[
            (
                "reqs/design.md",
                b"# A\n`dsn~a~1`\n\n# B\n`dsn~b~1`\nFeatures: x\n\n# H\n`hlr~h~1`\nSatisfied-by:\n- dsn~b~1\n",
            ),
            ("src/a.c", b"// [impl->dsn~a~1, dsn~b~1]\nvoid f();\n"),
            ("tests/a_test.cpp", b"// [utest->dsn~a~1]\nTEST(S, N) {}\n"),
            ("tests/b_test.cpp", b"// [utest->dsn~b~1]\nTEST(S, N) {}\n"),
            (
                "build/r.xml",
                b"<testsuite name=\"S\"><testcase classname=\"S\" name=\"N\" file=\"tests/b_test.cpp\"><failure/></testcase></testsuite>",
            ),
        ],
    );
    let output = shall(dir.path(), &["check", "--variant", "basis"]);
    assert_eq!(
        stdout(&output),
        "reqs/design.md:9: hlr~h~1: not covered by required type 'dsn'\n\
         reqs/design.md:11: hlr~h~1: is satisfied by dsn~b~1, which is not part of the variant\n\
         tests/a_test.cpp:1: test S.N was not executed: no result in the JUnit files\n"
    );
    assert_eq!(
        stderr(&output),
        "variant basis: not ok: 3 errors, 0 warnings (2 requirements, 2 tags, 1 test result)\n"
    );
    let output = shall(
        dir.path(),
        &["check", "--variant", "basis", "--mode", "strict"],
    );
    assert_eq!(output.status.code(), Some(1));
}

/// A workspace with a platform and a customer project that imports it.
fn workspace() -> tempfile::TempDir {
    let platform_config = br#"
[[level]]
type  = "cap"
name  = "Capabilities"
needs = ["impl", "utest"]

[verification]
types = ["utest"]

[sources]
requirements = ["reqs/*.md"]
impl  = ["src/*.cpp"]
utest = ["tests/*.cpp"]
junit = ["build/*.xml"]
"#;
    let project_config = br#"
[[level]]
type  = "hlr"
name  = "Customer"
needs = ["swr"]

[[import]]
name = "plat"
path = "../../platform"

[sources]
requirements = ["reqs/*.md"]
"#;
    project(
        "[workspace]\nmembers = [\"platform\", \"projects/*\"]\n",
        &[
            ("platform/shall.toml", platform_config),
            (
                "platform/reqs/cap.md",
                b"# Caps\n\n## Protect\n`cap~protect~1`\n\n## Log\n`cap~log~1`\n",
            ),
            ("platform/src/protect.cpp", b"// [impl->cap~protect~1]\nvoid p();\n"),
            (
                "platform/tests/protect_test.cpp",
                b"// [utest->cap~protect~1]\nTEST(Protect, Works) {}\n",
            ),
            (
                "platform/build/r.xml",
                b"<testsuite name=\"Protect\"><testcase classname=\"Protect\" name=\"Works\" file=\"tests/protect_test.cpp\"/></testsuite>",
            ),
            ("projects/a/shall.toml", project_config),
            (
                "projects/a/reqs/customer.md",
                b"# Customer\n\n## Protect the motor\n`hlr~a.protect~1`\n\nSatisfied-by:\n- cap~protect~1\n\nParams:\n- limit_c = 85\n",
            ),
        ],
    )
}

#[test]
fn checks_a_project_with_its_imports() {
    let dir = workspace();
    let output = shall(&dir.path().join("projects/a"), &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    // Paths are relative to the workspace root.
    assert_eq!(
        stdout(&output),
        "platform/reqs/cap.md:7: cap~log~1: not covered by required type 'impl'\n\
         platform/reqs/cap.md:7: cap~log~1: not covered by required type 'utest'\n"
    );
    assert_eq!(
        stderr(&output),
        "not ok: 2 errors, 0 warnings (3 requirements, 2 tags, 1 test result)\n"
    );

    // The report marks the imported items.
    let site = dir.path().join("site");
    let output = shall(
        &dir.path().join("projects/a"),
        &["report", "--out", site.to_str().unwrap()],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let json = fs::read_to_string(site.join("trace.json")).unwrap();
    assert!(json.contains("\"import\": \"plat\""), "{json}");
    assert!(
        json.contains("\"path\": \"projects/a/reqs/customer.md\""),
        "{json}"
    );
    let output = shall(dir.path(), &["report", "--out", site.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("describes a workspace, not a project"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn checks_every_member_of_a_workspace() {
    let dir = workspace();
    let expected_defects = "platform/reqs/cap.md:7: cap~log~1: not covered by required type 'impl'\n\
         platform/reqs/cap.md:7: cap~log~1: not covered by required type 'utest'\n";
    let expected_summaries = "platform: not ok: 2 errors, 0 warnings (2 requirements, 2 tags, 1 test result)\n\
         projects/a: not ok: 2 errors, 0 warnings (3 requirements, 2 tags, 1 test result)\n\
         workspace: not ok: 2 errors, 0 warnings (2 projects)\n";
    for (subdir, args) in [
        ("", &["check"][..]),
        ("projects/a", &["check", "--workspace"]),
    ] {
        let output = shall(&dir.path().join(subdir), args);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        // The platform's defects are reported once.
        assert_eq!(stdout(&output), expected_defects);
        assert_eq!(stderr(&output), expected_summaries);
    }
    let output = shall(dir.path(), &["check", "--mode", "strict"]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn reports_problems_with_imports() {
    let dir = workspace();
    let a = dir.path().join("projects/a");
    let config = fs::read_to_string(a.join("shall.toml")).unwrap();

    let elsewhere = tempfile::tempdir().unwrap();
    fs::write(elsewhere.path().join("shall.toml"), "").unwrap();
    fs::write(
        a.join("shall.toml"),
        config.replace(
            "../../platform",
            &elsewhere.path().to_string_lossy().replace('\\', "/"),
        ),
    )
    .unwrap();
    let output = shall(&a, &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("is outside the workspace"),
        "{}",
        stderr(&output)
    );

    fs::write(a.join("shall.toml"), config.replace("../../platform", "..")).unwrap();
    let output = shall(&a, &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("has no shall.toml"),
        "{}",
        stderr(&output)
    );

    fs::write(a.join("shall.toml"), config.replace("../../platform", ".")).unwrap();
    let output = shall(&a, &["check"]);
    assert!(
        stderr(&output).contains("a project cannot import itself"),
        "{}",
        stderr(&output)
    );

    // Imports of an import are not followed yet, also when written as an
    // inline array.
    fs::write(a.join("shall.toml"), &config).unwrap();
    let platform = dir.path().join("platform/shall.toml");
    let platform_config = fs::read_to_string(&platform).unwrap();
    fs::write(
        &platform,
        format!("import = [{{ name = \"a\", path = \"../projects/a\" }}]\n{platform_config}"),
    )
    .unwrap();
    let output = shall(&a, &["check"]);
    assert!(
        stdout(&output).contains("platform/shall.toml:1: warning: the imports of 'plat'"),
        "{}",
        stdout(&output)
    );
    let mut platform_config = platform_config;
    platform_config.push_str("\n[[import]]\nname = \"a\"\npath = \"../projects/a\"\n");
    fs::write(&platform, platform_config).unwrap();
    let output = shall(&a, &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stdout(&output).contains(
            "platform/shall.toml:16: warning: the imports of 'plat' are not followed; \
             transitive imports are planned (M8.2)"
        ),
        "{}",
        stdout(&output)
    );
}

#[test]
fn projects_must_be_members_of_their_workspace() {
    let dir = workspace();
    fs::write(
        dir.path().join("shall.toml"),
        "[workspace]\nmembers = [\"platform\"]\n",
    )
    .unwrap();
    let output = shall(&dir.path().join("projects/a"), &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("is not a member of the workspace"),
        "{}",
        stderr(&output)
    );
    fs::write(
        dir.path().join("shall.toml"),
        "[workspace]\nmembers = [\"platform\", \"missing\"]\n",
    )
    .unwrap();
    let output = shall(dir.path(), &["check"]);
    assert!(
        stderr(&output).contains("member 'missing' of"),
        "{}",
        stderr(&output)
    );
    let output = shall(dir.path(), &["check", "--variant", "x"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn imports_need_a_workspace_and_a_valid_one() {
    let dir = workspace();
    let a = dir.path().join("projects/a");
    fs::write(dir.path().join("shall.toml"), "[workspace\n").unwrap();
    let output = shall(&a, &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains(&format!("in {}", dir.path().join("shall.toml").display())),
        "{}",
        stderr(&output)
    );
    fs::remove_file(dir.path().join("shall.toml")).unwrap();
    let output = shall(&a, &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("is outside the project; import other projects"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn nested_projects_and_test_files_belong_to_their_own_project() {
    let dir = workspace();
    // A project nested in the platform keeps its Markdown to itself.
    let platform = dir.path().join("platform/shall.toml");
    let config = fs::read_to_string(&platform).unwrap();
    fs::write(&platform, config.replace("\"reqs/*.md\"", "\"**/*.md\"")).unwrap();
    fs::create_dir_all(dir.path().join("platform/vendor/x/reqs")).unwrap();
    fs::write(dir.path().join("platform/vendor/x/shall.toml"), "").unwrap();
    fs::write(
        dir.path().join("platform/vendor/x/reqs/x.md"),
        "# X\n`cap~vendor~1`\n",
    )
    .unwrap();
    // A relative `file` attribute with `..`, as Meson writes it, still
    // names the test file of the platform.
    fs::write(
        dir.path().join("platform/tests/other_test.cpp"),
        "// [utest->cap~log~1]\nTEST(Protect, Works) {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("platform/build/r.xml"),
        "<testsuite name=\"Protect\"><testcase classname=\"Protect\" name=\"Works\" \
         file=\"build/../tests/protect_test.cpp\"><failure/></testcase></testsuite>",
    )
    .unwrap();
    let output = shall(&dir.path().join("projects/a"), &["check"]);
    assert_eq!(
        stdout(&output),
        "platform/reqs/cap.md:7: cap~log~1: not covered by required type 'impl'\n\
         platform/tests/other_test.cpp:1: test Protect.Works was not executed: no result in the JUnit files\n\
         platform/tests/protect_test.cpp:1: test Protect.Works failed\n"
    );
}

#[cfg(unix)]
#[test]
fn members_must_stay_inside_the_workspace() {
    let dir = workspace();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("shall.toml"), "").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("projects/b")).unwrap();
    let output = shall(dir.path(), &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("outside the workspace"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn imported_projects_keep_their_own_configuration() {
    let dir = workspace();
    // The platform satisfies a capability by another one, with the
    // default `[satisfied_by]`, which the project replaces; the project
    // has code of a type only its import knows.
    let a = dir.path().join("projects/a/shall.toml");
    let config = fs::read_to_string(&a).unwrap();
    fs::write(
        &a,
        format!(
            "{}\n[satisfied_by]\ntargets = [\"hlr\"]\n",
            config.replace(
                "requirements = [\"reqs/*.md\"]",
                "requirements = [\"reqs/*.md\"]\nimpl = [\"src/*.cpp\"]",
            )
        ),
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("projects/a/src")).unwrap();
    fs::write(dir.path().join("projects/a/src/a.cpp"), "void a();\n").unwrap();
    fs::write(
        dir.path().join("platform/reqs/more.md"),
        "# More\n\n## Combined\n`cap~combined~1`\nNeeds: impl, utest\nSatisfied-by:\n- cap~protect~1\n",
    )
    .unwrap();
    let output = shall(&dir.path().join("projects/a"), &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "platform/reqs/cap.md:7: cap~log~1: not covered by required type 'impl'\n\
         platform/reqs/cap.md:7: cap~log~1: not covered by required type 'utest'\n\
         platform/reqs/more.md:4: cap~combined~1: not covered by required type 'impl'\n\
         platform/reqs/more.md:4: cap~combined~1: not covered by required type 'utest'\n"
    );
}

#[test]
fn imports_and_workspaces_need_members() {
    let dir = workspace();
    // A project inside the workspace that is no member cannot be imported.
    fs::create_dir_all(dir.path().join("lib")).unwrap();
    fs::write(dir.path().join("lib/shall.toml"), "").unwrap();
    let a = dir.path().join("projects/a/shall.toml");
    let config = fs::read_to_string(&a).unwrap();
    fs::write(&a, config.replace("../../platform", "../../lib")).unwrap();
    let output = shall(&dir.path().join("projects/a"), &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("lib is not a member of the workspace"),
        "{}",
        stderr(&output)
    );
    fs::write(dir.path().join("shall.toml"), "[workspace]\n").unwrap();
    let output = shall(dir.path(), &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("lists no [workspace] members"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_broken_project_above_is_no_workspace() {
    let dir = project("[[level\n", &[("inner/shall.toml", b"")]);
    let output = shall(&dir.path().join("inner"), &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
}

#[test]
fn the_search_for_a_workspace_stops_at_the_work_tree() {
    let dir = project(
        "[workspace]\nmembers = [\"other\"]\n",
        &[("other/shall.toml", b""), ("repo/shall.toml", b"")],
    );
    // A repository of its own, e.g. a submodule with a `.git` file.
    fs::write(
        dir.path().join("repo/.git"),
        "gitdir: ../.git/modules/repo\n",
    )
    .unwrap();
    let output = shall(&dir.path().join("repo"), &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    // A quoted key is a workspace too.
    fs::write(
        dir.path().join("shall.toml"),
        "\"workspace\" = { members = [\"other\"] }\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("third")).unwrap();
    fs::write(dir.path().join("third/shall.toml"), "").unwrap();
    let output = shall(&dir.path().join("third"), &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("is not a member of the workspace"),
        "{}",
        stderr(&output)
    );
}
