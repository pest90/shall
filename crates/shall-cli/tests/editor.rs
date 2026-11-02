//! Keeps the editor files in `contrib/vscode` valid and in line with the
//! output of `shall check`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn contrib(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contrib/vscode")
        .join(file)
}

/// Parses JSON with whole-line `//` comments, as VSCode writes them.
fn jsonc(file: &str) -> serde_json::Value {
    let text = fs::read_to_string(contrib(file)).unwrap();
    let json: String = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    serde_json::from_str(&json).unwrap_or_else(|e| panic!("{file}: {e}"))
}

#[test]
fn the_problem_matcher_reads_every_line_of_shall_check() {
    let tasks = jsonc("tasks.json");
    let check = tasks["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["label"] == "shall: check")
        .unwrap();
    let pattern = &check["problemMatcher"]["pattern"];
    let regex = regex::Regex::new(pattern["regexp"].as_str().unwrap()).unwrap();
    let group = |name: &str| pattern[name].as_u64().unwrap() as usize;

    // The fixture alone and as a member of a workspace with a warning of
    // its own, in a strict mode for the most summary lines.
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/motor");
    let workspace = tempfile::tempdir().unwrap();
    copy(&fixture, &workspace.path().join("motor"));
    fs::write(
        workspace.path().join("shall.toml"),
        "[workspace]\nmembers = [\"motor\"]\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join("motor/reqs/typo.md"),
        "# T\n`sys~typo-k4f2~1`\nNeds: swr\n",
    )
    .unwrap();
    for (dir, root) in [
        (fixture.clone(), fixture.clone()),
        (
            workspace.path().to_path_buf(),
            workspace.path().to_path_buf(),
        ),
        (
            workspace.path().join("motor"),
            workspace.path().to_path_buf(),
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_shall"))
            .current_dir(&dir)
            .args(["check", "--mode", "strict"])
            .output()
            .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.is_empty());
        if root != fixture {
            // The warning of typo.md, so that the warning case is real.
            assert!(text.contains(": warning: "), "{text}");
        }
        for line in text.lines() {
            let captures = regex.captures(line).unwrap_or_else(|| panic!("{line}"));
            let file = &captures[group("file")];
            assert!(root.join(file).is_file(), "{line}");
            let warning = line.contains(": warning: ");
            assert_eq!(captures.get(group("severity")).is_some(), warning, "{line}");
            assert!(
                !captures[group("message")].starts_with("warning:"),
                "{line}"
            );
        }
        // VSCode reads stderr too: no summary line is a problem.
        for line in String::from_utf8(output.stderr).unwrap().lines() {
            assert!(!regex.is_match(line), "{line}");
        }
    }
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
fn the_snippets_are_valid() {
    let snippets = jsonc("shall.code-snippets");
    for (name, snippet) in snippets.as_object().unwrap() {
        assert!(snippet["prefix"].is_string(), "{name}");
        assert!(snippet["body"].is_array(), "{name}");
    }
}
