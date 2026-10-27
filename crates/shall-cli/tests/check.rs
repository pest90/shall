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
