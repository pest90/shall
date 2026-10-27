//! Keeps the examples in the documentation valid and meaningful:
//! ```` ```markdown ```` blocks must parse without diagnostics and are
//! pinned by a snapshot, ```` ```toml ```` blocks must be valid
//! configurations, and relative links must point to existing files.

use std::fs;
use std::path::{Path, PathBuf};

use shall_core::config::Config;
use shall_core::markdown;

/// Documentation files and the number of Markdown and TOML examples in
/// each, so that a lost or renamed fence is noticed.
const FILES: [(&str, usize, usize); 2] = [("README.md", 1, 1), ("docs/user-guide.md", 2, 1)];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Returns the contents of all code blocks opened with exactly
/// ```` ```<language> ````.
fn blocks(text: &str, language: &str) -> Vec<String> {
    let open = format!("```{language}");
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in text.lines() {
        match &mut current {
            None if line == open => current = Some(Vec::new()),
            Some(lines) if line == "```" => {
                blocks.push(lines.join("\n"));
                current = None;
            }
            Some(lines) => lines.push(line),
            None => {}
        }
    }
    blocks
}

#[test]
fn markdown_examples_parse_as_documented() {
    let mut items = Vec::new();
    for (file, markdown_count, _) in FILES {
        let text = fs::read_to_string(root().join(file)).unwrap();
        let examples = blocks(&text, "markdown");
        assert_eq!(
            examples.len(),
            markdown_count,
            "Markdown examples in {file}"
        );
        for block in examples {
            let parsed = markdown::parse(Path::new(file), &block).unwrap();
            assert_eq!(parsed.diagnostics, [], "{file}:\n{block}");
            for item in &parsed.items {
                assert!(!item.title.is_empty(), "{file}: {} has no title", item.id);
            }
            items.extend(parsed.items);
        }
    }
    insta::assert_yaml_snapshot!(items);
}

#[test]
fn toml_examples_are_valid_configurations() {
    for (file, _, toml_count) in FILES {
        let text = fs::read_to_string(root().join(file)).unwrap();
        let examples = blocks(&text, "toml");
        assert_eq!(examples.len(), toml_count, "TOML examples in {file}");
        for block in examples {
            let config = Config::parse(&block).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert!(!config.levels.is_empty(), "{file}: no levels in\n{block}");
            for level in &config.levels {
                for need in &level.needs {
                    let known = config.level(need).is_some()
                        || ["impl", "utest", "itest"].contains(&need.as_str());
                    assert!(known, "{file}: {} needs unknown type {need}", level.typ);
                }
            }
        }
    }
}

/// Removes code blocks and inline code, where `[x](y)` is an example and
/// not a link.
fn without_code(text: &str) -> String {
    let mut result = String::new();
    let mut in_block = false;
    for line in text.lines() {
        if line.starts_with("```") {
            in_block = !in_block;
            continue;
        }
        if in_block {
            continue;
        }
        let outside: String = line.split('`').step_by(2).collect::<Vec<_>>().join(" ");
        result.push_str(&outside);
        result.push('\n');
    }
    result
}

#[test]
fn relative_links_resolve() {
    for (file, _, _) in FILES {
        let path = root().join(file);
        let text = without_code(&fs::read_to_string(&path).unwrap());
        for (start, _) in text.match_indices("](") {
            let rest = &text[start + 2..];
            let target = &rest[..rest.find(')').unwrap()];
            if target.contains("://") || target.starts_with('#') {
                continue;
            }
            let target = target.split('#').next().unwrap();
            let resolved = path.parent().unwrap().join(target);
            assert!(resolved.exists(), "{file}: broken link {target}");
        }
    }
}
