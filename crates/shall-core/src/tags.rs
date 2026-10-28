//! Tags in code and tests that link them to the items they implement or
//! verify, in OpenFastTrace (OFT) syntax:
//!
//! ```cpp
//! // [impl->dsn~motor.temp-monitor-r5t1~1]
//! void TempMonitor::run() { /* ... */ }
//!
//! // [utest->dsn~motor.temp-monitor-r5t1~1]
//! TEST(TempMonitor, RaisesFlagAbove85C) { /* ... */ }
//! ```
//!
//! Supported forms, as in OFT:
//!
//! - `[type->target]` creates an item of that type with revision 0 that
//!   covers the target. Its name is the target's name and a hash, e.g.
//!   `impl~motor.temp-monitor-r5t1-3fa2c1d0~0`.
//! - `[type->a, b]` creates one such item per target.
//! - `[type->target>>needed, types]` creates items named like the target,
//!   without hash, that need the given types.
//! - `[type~name~revision->targets]` creates one item with that ID that
//!   covers all targets; `[type~~revision->targets]` only sets the revision.
//!
//! The hash covers the file path, the type, the target's type and name
//! (not its revision) and how often the same type and target were tagged
//! before in the file. Inserting other tags and updating the target's
//! revision keep the ID; renaming the file or reordering tags with the same
//! type and target change it. Two `>>` tags with the same target name give
//! the same ID, which is reported as a duplicate. Short tags `[[name:1]]`
//! are not supported.
//!
//! A text is only a tag if its first target starts like an ID
//! (`type~name`), so that code such as `values[p->len]` or `mask[r->x&~1]`
//! is not mistaken for one. A malformed tag that does not start like that,
//! e.g. `[impl->dsn ~x~1]`, is therefore ignored without a diagnostic; it
//! shows as missing coverage. Tag items are approved.
//!
//! After a tag, the directly following test definition is recorded, so
//! that test results can be matched: gtest's `TEST`, `TEST_F`, `TEST_P`,
//! `TYPED_TEST`, `TYPED_TEST_P` and `GTEST_TEST`, or a Rust function with a
//! test attribute (`#[test]`, `#[…::test]`, `#[rstest]`, `#[test_case]`).
//! If code precedes the tag on its line, as in a trailing comment, that
//! line is the definition. Otherwise the code after the tag is searched,
//! on its own line after a closing `*/` and then on the following lines;
//! comments of any form, attributes (also across lines), preprocessor
//! lines, `namespace` openings and further tags are skipped. Comments and
//! literals are recognised with a small lexer for C, C++ and Rust, which
//! also tracks the inline Rust modules around a test.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::diagnostic::Diagnostic;
use crate::id::{ItemId, ItemType};
use crate::item::{Item, Location, LocationError, Origin, Reference, Status, TestKind, TestName};

/// Items and diagnostics found in one file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Scanned {
    /// Items created from tags, in the order of the tags.
    pub items: Vec<Item>,
    /// Problems with tags, in the order of their lines.
    pub diagnostics: Vec<Diagnostic>,
}

/// `[type(~name~revision)? -> targets (>> needs)?]`
static TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\[\s*(?<type>\p{Alpha}\w*)(?:~(?<name>[^\]\s~]*)~(?<revision>\w+))?\s*->\s*(?<targets>[^\]>]*?)\s*(?:>>\s*(?<needs>[^\]]*?)\s*)?\]",
    )
    .expect("valid regex")
});

/// The start of an ID: `type~name`.
static ID_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^`?\p{Alpha}\w*~\p{Alpha}").expect("valid regex"));

static GTEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*(?<macro>TEST|TEST_F|TEST_P|TYPED_TEST|TYPED_TEST_P|GTEST_TEST)\s*\(\s*(?<suite>\w+)\s*,\s*(?<name>\w+)\s*\)",
    )
    .expect("valid regex")
});

static GTEST_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:TEST|TEST_F|TEST_P|TYPED_TEST|TYPED_TEST_P|GTEST_TEST)\s*\(")
        .expect("valid regex")
});

static RUST_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+(?<name>\w+)",
    )
    .expect("valid regex")
});

static RUST_MOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(?<name>\w+)\s*\{").expect("valid regex")
});

static NAMESPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:inline\s+)?namespace\b[^;]*\{\s*$").expect("valid regex"));

/// Scans the `text` of the file at `path`, relative to the project root,
/// for tags.
pub fn scan(path: &Path, text: &str) -> Result<Scanned, LocationError> {
    let file = Location::new(path, 1)?;
    let lines: Vec<&str> = text.lines().collect();
    let lexed = lex(&lines);
    let mut scanned = Scanned::default();
    let mut occurrences: HashMap<String, u32> = HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        for captures in TAG.captures_iter(line) {
            let targets = &captures["targets"];
            if !ID_START.is_match(targets.trim()) {
                continue;
            }
            let whole = captures.get(0).expect("group 0 exists");
            let text = whole.as_str().to_owned();
            let location = file.with_line(line_number(index));
            match tag_items(&file, &captures, &location, &mut occurrences) {
                Ok(mut items) => {
                    let test = test_after(&lines, &lexed, index, whole.range());
                    for item in &mut items {
                        item.origin = Origin::Tag {
                            text: text.clone(),
                            test: test.clone(),
                        };
                    }
                    scanned.items.extend(items);
                }
                Err(message) => scanned.diagnostics.push(Diagnostic::error(
                    location,
                    format!("tag {text}: {message}"),
                )),
            }
        }
    }
    Ok(scanned)
}

fn line_number(index: usize) -> u32 {
    u32::try_from(index + 1).expect("files have fewer than 2^32 lines")
}

/// Builds the items of one tag, or returns why the tag is invalid.
fn tag_items(
    file: &Location,
    captures: &regex::Captures<'_>,
    location: &Location,
    occurrences: &mut HashMap<String, u32>,
) -> Result<Vec<Item>, String> {
    let typ: ItemType = captures["type"].parse().map_err(|e| format!("{e}"))?;
    let targets = captures["targets"]
        .split(',')
        .map(|target| {
            let target = target.trim().trim_matches('`');
            target
                .parse::<ItemId>()
                .map_err(|e| format!("invalid target: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let needs = match captures.name("needs") {
        Some(needs) => needs
            .as_str()
            .split(',')
            .map(|typ| {
                typ.trim()
                    .parse::<ItemType>()
                    .map_err(|e| format!("invalid needed type: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    let revision = match captures.name("revision") {
        Some(revision) => {
            crate::id::parse_revision(revision.as_str()).map_err(|e| e.to_string())?
        }
        None => 0,
    };
    let covers = |targets: &[ItemId]| -> Vec<Reference> {
        targets
            .iter()
            .map(|id| Reference {
                id: id.clone(),
                line: location.line(),
            })
            .collect()
    };
    let item = |id: ItemId, covers: Vec<Reference>| Item {
        id,
        title: String::new(),
        description: String::new(),
        rationale: None,
        comment: None,
        status: Status::Approved,
        needs: needs.clone(),
        covers,
        depends: Vec::new(),
        satisfied_by: Vec::new(),
        params: Vec::new(),
        tags: Vec::new(),
        features: Vec::new(),
        source: None,
        location: location.clone(),
        end_line: location.line(),
        origin: Origin::Requirement,
        import: None,
    };

    if let Some(name) = captures
        .name("name")
        .filter(|name| !name.as_str().is_empty())
    {
        let id = ItemId::new(typ, name.as_str(), revision)
            .map_err(|e| format!("invalid own ID: {e}"))?;
        return Ok(vec![item(id, covers(&targets))]);
    }
    targets
        .iter()
        .map(|target| {
            let name = if needs.is_empty() {
                // Without the target's revision, so that updating it after
                // the target changed keeps the ID.
                let key = format!("{typ}->{}~{}", target.typ(), target.name());
                let count = occurrences.entry(key.clone()).or_insert(0);
                *count += 1;
                let hash = fnv1a(&format!("{}:{key}:{count}", file.path()));
                format!("{}-{hash:08x}", target.name())
            } else {
                target.name().to_owned()
            };
            let id = ItemId::new(typ.clone(), &name, revision).map_err(|e| e.to_string())?;
            Ok(item(id, covers(std::slice::from_ref(target))))
        })
        .collect()
}

/// 32-bit FNV-1a: small, stable across platforms and Rust versions.
fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// Lexer state carried from one line to the next.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LexState {
    block_comment: bool,
    string: Option<Quote>,
}

/// An open string literal: `"..."` or a Rust raw string with that many `#`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quote {
    Plain,
    Raw(usize),
}

/// A line as code: comments and the contents of string and char literals
/// are replaced by spaces, so byte positions stay the same. This is enough
/// for C, C++ and Rust to tell code from comments and to count braces.
fn code_view(line: &str, mut state: LexState) -> (String, LexState) {
    let bytes = line.as_bytes();
    let mut code = String::with_capacity(line.len());
    let blank = |code: &mut String, text: &str| code.extend(text.bytes().map(|_| ' '));
    let mut i = 0;
    while i < bytes.len() {
        let rest = &line[i..];
        let char_len = rest.chars().next().map_or(1, char::len_utf8);
        if state.block_comment {
            if rest.starts_with("*/") {
                state.block_comment = false;
                blank(&mut code, "*/");
                i += 2;
            } else {
                blank(&mut code, &rest[..char_len]);
                i += char_len;
            }
            continue;
        }
        match state.string {
            Some(Quote::Plain) => {
                let len = if bytes[i] == b'\\' {
                    (1 + rest[1..].chars().next().map_or(0, char::len_utf8)).min(rest.len())
                } else {
                    if bytes[i] == b'"' {
                        state.string = None;
                    }
                    char_len
                };
                blank(&mut code, &rest[..len]);
                i += len;
                continue;
            }
            Some(Quote::Raw(hashes)) => {
                let end = format!("\"{}", "#".repeat(hashes));
                let len = if rest.starts_with(&end) {
                    state.string = None;
                    end.len()
                } else {
                    char_len
                };
                blank(&mut code, &rest[..len]);
                i += len;
                continue;
            }
            None => {}
        }
        if rest.starts_with("//") {
            blank(&mut code, rest);
            break;
        }
        if rest.starts_with("/*") {
            state.block_comment = true;
            blank(&mut code, "/*");
            i += 2;
            continue;
        }
        let after_identifier =
            i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if bytes[i] == b'r' && !after_identifier {
            let hashes = rest[1..].bytes().take_while(|&b| b == b'#').count();
            if rest[1 + hashes..].starts_with('"') {
                state.string = Some(Quote::Raw(hashes));
                blank(&mut code, &rest[..2 + hashes]);
                i += 2 + hashes;
                continue;
            }
        }
        if bytes[i] == b'"' {
            state.string = Some(Quote::Plain);
            code.push(' ');
            i += 1;
            continue;
        }
        if bytes[i] == b'\''
            && let Some(len) = char_literal(rest)
        {
            blank(&mut code, &rest[..len]);
            i += len;
            continue;
        }
        code.push_str(&rest[..char_len]);
        i += char_len;
    }
    (code, state)
}

/// Length of a char literal such as `'{'` or `'\''` at the start of
/// `text`, or none for a Rust lifetime such as `'a`.
fn char_literal(text: &str) -> Option<usize> {
    let rest = &text[1..];
    if let Some(escaped) = rest.strip_prefix('\\') {
        let end = escaped.find('\'')?;
        return Some(2 + end + 1);
    }
    let first = rest.chars().next()?;
    rest[first.len_utf8()..]
        .starts_with('\'')
        .then(|| 1 + first.len_utf8() + 1)
}

/// What is known about every line: its code view, whether it starts inside
/// a block comment, and the inline Rust modules around it.
struct Lexed {
    code: Vec<String>,
    starts_in_comment: Vec<bool>,
    modules: Vec<Vec<String>>,
}

fn lex(lines: &[&str]) -> Lexed {
    let mut state = LexState::default();
    let mut depth = 0usize;
    let mut stack: Vec<(String, usize)> = Vec::new();
    let mut lexed = Lexed {
        code: Vec::with_capacity(lines.len()),
        starts_in_comment: Vec::with_capacity(lines.len()),
        modules: Vec::with_capacity(lines.len()),
    };
    for line in lines {
        lexed.starts_in_comment.push(state.block_comment);
        let (code, next) = code_view(line, state);
        state = next;
        let mut ignored = false;
        if let Some(captures) = RUST_MOD.captures(strip_attributes(&code, &mut ignored)) {
            stack.push((captures["name"].to_owned(), depth + 1));
        }
        lexed
            .modules
            .push(stack.iter().map(|(name, _)| name.clone()).collect());
        for c in code.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        while stack.last().is_some_and(|&(_, open)| depth < open) {
            stack.pop();
        }
        lexed.code.push(code);
    }
    lexed
}

/// Finds the test a tag at `tag` (a byte range) in line `index` belongs to:
/// defined by code before the tag on its line, or else by the code that
/// follows, on the same line after the tag or on the next lines.
fn test_after(
    lines: &[&str],
    lexed: &Lexed,
    index: usize,
    tag: std::ops::Range<usize>,
) -> Option<TestName> {
    let code = &lexed.code[index];
    let mut search = TestSearch {
        lines,
        lexed,
        rust_test: rust_attribute_above(lexed, index),
        attribute: None,
    };
    let before = TAG.replace_all(&code[..tag.start], "");
    if let Some(found) = search.code(index, &before) {
        return found;
    }
    let after = TAG.replace_all(&code[tag.end..], "");
    if let Some(found) = search.code(index, &after) {
        return found;
    }
    for next in index + 1..lines.len() {
        if let Some(found) = search.code(next, &lexed.code[next]) {
            return found;
        }
    }
    None
}

/// Whether the code lines directly above line `index` are attributes that
/// mark a Rust test, as with a tag between `#[test]` and `fn`.
fn rust_attribute_above(lexed: &Lexed, index: usize) -> bool {
    let mut rust_test = false;
    for code in lexed.code[..index].iter().rev() {
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        if !code.starts_with("#[") || !strip_attributes(code, &mut rust_test).trim().is_empty() {
            break;
        }
    }
    rust_test
}

/// The search for a test definition through the code after a tag.
struct TestSearch<'a> {
    lines: &'a [&'a str],
    lexed: &'a Lexed,
    rust_test: bool,
    /// An attribute that spans lines, from after its `#[`.
    attribute: Option<String>,
}

impl TestSearch<'_> {
    /// Looks at `code` from line `index`. Returns `Some` once the search
    /// ends, with the test if `code` defines one, and `None` to go on.
    fn code(&mut self, index: usize, code: &str) -> Option<Option<TestName>> {
        let mut code = TAG.replace_all(code, "").into_owned();
        if let Some(mut attribute) = self.attribute.take() {
            attribute.push(' ');
            attribute.push_str(&code);
            match closing_bracket(&attribute) {
                Some(end) => {
                    self.rust_test |= is_test_attribute(&attribute[..end]);
                    code = attribute[end + 1..].to_owned();
                }
                None => {
                    self.attribute = Some(attribute);
                    return None;
                }
            }
        }
        let rest = strip_attributes(&code, &mut self.rust_test).trim();
        if rest.is_empty() {
            return None;
        }
        if let Some(open) = rest.strip_prefix("#[") {
            // The attribute closes on a later line.
            self.attribute = Some(open.to_owned());
            return None;
        }
        if rest.starts_with('#') || NAMESPACE.is_match(rest) {
            // Preprocessor lines and namespaces may stand between tag and
            // test.
            return None;
        }
        let start = code.len() - code.trim_start().len();
        let rest_start = code.find(rest).unwrap_or(start);
        Some(test_definition(
            self.lines,
            self.lexed,
            index,
            &code[rest_start..],
            self.rust_test,
        ))
    }
}

/// Returns the test defined by `code`, the code of line `index` from where
/// the definition may start.
fn test_definition(
    lines: &[&str],
    lexed: &Lexed,
    index: usize,
    code: &str,
    rust_test: bool,
) -> Option<TestName> {
    let line = line_number(index);
    if GTEST_START.is_match(code) {
        // The macro arguments may continue on the next lines.
        let mut joined = code.to_owned();
        for next in lines.iter().skip(index + 1).take(3) {
            if GTEST.is_match(&joined) {
                break;
            }
            joined.push(' ');
            joined.push_str(next.trim());
        }
        let captures = GTEST.captures(&joined)?;
        let kind = match &captures["macro"] {
            "TEST_P" => TestKind::GtestParameterized,
            "TYPED_TEST" | "TYPED_TEST_P" => TestKind::GtestTyped,
            _ => TestKind::Gtest,
        };
        return Some(TestName {
            kind,
            suite: Some(captures["suite"].to_owned()),
            name: captures["name"].to_owned(),
            module: Vec::new(),
            line,
        });
    }
    let captures = RUST_FN.captures(code).filter(|_| rust_test)?;
    Some(TestName {
        kind: TestKind::Rust,
        suite: None,
        name: captures["name"].to_owned(),
        module: lexed.modules[index].clone(),
        line,
    })
}

/// Strips leading Rust attributes such as `#[test]` and notes whether one
/// of them marks a test. An attribute that does not close on this line is
/// left in place.
fn strip_attributes<'a>(mut code: &'a str, rust_test: &mut bool) -> &'a str {
    while let Some(rest) = code.trim_start().strip_prefix("#[") {
        let Some(end) = closing_bracket(rest) else {
            break;
        };
        *rust_test |= is_test_attribute(&rest[..end]);
        code = &rest[end + 1..];
    }
    code
}

/// Position of the `]` that closes an attribute, skipping nested brackets.
fn closing_bracket(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (position, c) in text.char_indices() {
        match c {
            '[' => depth += 1,
            ']' if depth == 0 => return Some(position),
            ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Whether an attribute (without `#[`) marks a test, e.g. `test`,
/// `tokio::test(flavor = "multi_thread")`, `rstest` or `test_case(1)`.
fn is_test_attribute(attribute: &str) -> bool {
    let path = attribute
        .trim()
        .split(['(', ']', ' ', '\n'])
        .next()
        .unwrap_or("")
        .trim();
    path == "test"
        || path.ends_with("::test")
        || path == "rstest"
        || path == "test_case"
        || path.ends_with("::test_case")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_at(path: &str, text: &str) -> Scanned {
        scan(Path::new(path), text).unwrap()
    }

    fn scan_str(text: &str) -> Scanned {
        scan_at("src/x.cpp", text)
    }

    fn ids(scanned: &Scanned) -> Vec<String> {
        scanned.items.iter().map(|i| i.id.to_string()).collect()
    }

    fn tag_text(item: &Item) -> &str {
        match &item.origin {
            Origin::Tag { text, .. } => text,
            Origin::Requirement => panic!("not a tag"),
        }
    }

    fn test(item: &Item) -> Option<&TestName> {
        match &item.origin {
            Origin::Tag { test, .. } => test.as_ref(),
            Origin::Requirement => panic!("not a tag"),
        }
    }

    fn test_names(scanned: &Scanned) -> Vec<Option<String>> {
        scanned
            .items
            .iter()
            .map(|item| test(item).map(ToString::to_string))
            .collect()
    }

    #[test]
    fn creates_items_from_tags() {
        let scanned = scan_str(
            "\
#include <x>

// [impl->dsn~motor.temp-r5t1~1]
void run() {}
/* [ impl -> dsn~motor.other-b6j3~2 ] [utest~own-name~3->dsn~motor.temp-r5t1~1] */
",
        );
        assert_eq!(scanned.diagnostics, []);
        let ids = ids(&scanned);
        assert_eq!(ids.len(), 3);
        assert!(ids[0].starts_with("impl~motor.temp-r5t1-"), "{ids:?}");
        assert!(ids[0].ends_with("~0"), "{ids:?}");
        assert!(ids[1].starts_with("impl~motor.other-b6j3-"), "{ids:?}");
        assert_eq!(ids[2], "utest~own-name~3");
        let first = &scanned.items[0];
        assert_eq!(first.location.to_string(), "src/x.cpp:3");
        assert_eq!(first.covers[0].id.to_string(), "dsn~motor.temp-r5t1~1");
        assert_eq!(first.covers[0].line, 3);
        assert_eq!(first.status, Status::Approved);
        assert_eq!(tag_text(first), "[impl->dsn~motor.temp-r5t1~1]");
        assert_eq!(
            tag_text(&scanned.items[1]),
            "[ impl -> dsn~motor.other-b6j3~2 ]"
        );
    }

    #[test]
    fn supports_oft_forms() {
        let scanned = scan_str(
            "\
// [impl->dsn~a~1, `dsn~b~2`]
// [dsn->req~x~1 >> impl, utest]
// [impl~~2->dsn~a~1]
// [impl~both~1->dsn~a~1,dsn~b~2]
",
        );
        assert_eq!(scanned.diagnostics, []);
        let ids = ids(&scanned);
        assert!(
            ids[0].starts_with("impl~a-") && ids[0].ends_with("~0"),
            "{ids:?}"
        );
        assert!(ids[1].starts_with("impl~b-"), "{ids:?}");
        assert_eq!(ids[2], "dsn~x~0");
        assert!(
            ids[3].starts_with("impl~a-") && ids[3].ends_with("~2"),
            "{ids:?}"
        );
        assert_eq!(ids[4], "impl~both~1");
        let needs: Vec<_> = scanned.items[2]
            .needs
            .iter()
            .map(ItemType::as_str)
            .collect();
        assert_eq!(needs, ["impl", "utest"]);
        assert_eq!(scanned.items[4].covers.len(), 2);
    }

    #[test]
    fn generated_ids_survive_inserted_tags() {
        let before = scan_str("// [impl->dsn~a~1]\n// [impl->dsn~b~1]\n// [impl->dsn~a~1]\n");
        let after = scan_str(
            "// [impl->dsn~z~1]\n\n// [impl->dsn~a~1]\n// [utest->dsn~a~1]\n// [impl->dsn~b~1]\n// [impl->dsn~a~1]\n",
        );
        let before = ids(&before);
        let after = ids(&after);
        assert_eq!(before.len(), 3);
        for id in &before {
            assert!(after.contains(id), "{id} changed: {after:?}");
        }
        assert_ne!(before[0], before[2], "the same tag twice gets two IDs");
        let elsewhere = ids(&scan_at("src/y.cpp", "// [impl->dsn~a~1]\n"));
        assert!(!before.contains(&elsewhere[0]));
    }

    #[test]
    fn ignores_code_that_only_looks_similar() {
        let scanned = scan_str(
            "\
x = values[p->len];
y = a[i->next->len];
z = [](auto a) -> int {};
m = buf[hdr->size&~0x3u];
r = regs[dev->ctrl|~MASK];
See [config->~/.config/app].
",
        );
        assert_eq!(scanned.items, []);
        assert_eq!(scanned.diagnostics, []);
    }

    #[test]
    fn reports_invalid_tags() {
        let scanned = scan_str(
            "// [impl->dsn~x]\n// [impl->dsn~x~1, foo]\n// [impl~1x~1->dsn~x~1]\n// [impl->dsn~x~1>>im_pl]\n",
        );
        let messages: Vec<_> = scanned
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            messages,
            [
                "src/x.cpp:1: tag [impl->dsn~x]: invalid target: invalid ID 'dsn~x': expected 'typ~name~revision'",
                "src/x.cpp:2: tag [impl->dsn~x~1, foo]: invalid target: invalid ID 'foo': expected 'typ~name~revision'",
                "src/x.cpp:3: tag [impl~1x~1->dsn~x~1]: invalid own ID: invalid name '1x': expected a letter \
                 followed by ASCII letters, digits, '_' and '-', with '.' between non-empty segments",
                "src/x.cpp:4: tag [impl->dsn~x~1>>im_pl]: invalid needed type: invalid type 'im_pl': \
                 expected ASCII letters only",
            ]
        );
    }

    #[test]
    fn finds_gtest_definitions() {
        let scanned = scan_str(
            "\
// [utest->dsn~a~1]
TEST(TempMonitor, RaisesFlag) {}

// [utest->dsn~a~1]
// Checks the relay.
/* More
   text. */
#if defined(X)
namespace {
TEST_F( RelayDriverTest , OpensOnEstop ) {}

// [utest->dsn~a~1]
TEST_P(Param,
       Works) {}

// [utest->dsn~a~1]
TYPED_TEST(Typed, Works) {}

// [utest->dsn~a~1]
void helper() {}
TEST(Late, Test) {}

TEST(Same, Line) { EXPECT_TRUE(1); } // [utest->dsn~a~1]
TEST(Next, One) {}

// [utest->dsn~a~1]
*p = 1;
TEST(After, Pointer) {}
",
        );
        assert_eq!(
            test_names(&scanned),
            [
                Some("TempMonitor.RaisesFlag".to_owned()),
                Some("RelayDriverTest.OpensOnEstop".to_owned()),
                Some("Param.Works".to_owned()),
                Some("Typed.Works".to_owned()),
                None,
                Some("Same.Line".to_owned()),
                None,
            ]
        );
        let kinds: Vec<_> = scanned
            .items
            .iter()
            .filter_map(|item| test(item).map(|t| (t.kind, t.line)))
            .collect();
        assert_eq!(
            kinds,
            [
                (TestKind::Gtest, 2),
                (TestKind::Gtest, 10),
                (TestKind::GtestParameterized, 13),
                (TestKind::GtestTyped, 17),
                (TestKind::Gtest, 23),
            ]
        );
    }

    #[test]
    fn finds_rust_tests_with_module_path() {
        let scanned = scan_at(
            "src/lib.rs",
            "\
#[cfg(test)]
mod tests {
    // [utest->dsn~a~1]
    #[test]
    fn shuts_down() {}

    mod nested {
        /// [utest->dsn~a~1]
        /// Documented.
        #[tokio::test(flavor = \"multi_thread\")]
        #[should_panic(
            expected = \"x\"
        )]
        pub async fn later() {}
    }

    // [utest->dsn~a~1]
    #[test] fn same_line() {}

    // [utest->dsn~a~1]
    #[test_case(1)]
    const fn cased() {}
}

// [impl->dsn~a~1]
fn not_a_test() {}
",
        );
        assert_eq!(
            test_names(&scanned),
            [
                Some("tests::shuts_down".to_owned()),
                Some("tests::nested::later".to_owned()),
                Some("tests::same_line".to_owned()),
                Some("tests::cased".to_owned()),
                None,
            ]
        );
    }

    #[test]
    fn finds_tests_after_tags_in_comment_text() {
        let scanned = scan_str(
            "\
// Verifies [utest->dsn~a~1] and more
TEST(A, B) {}

/**
 * [utest->dsn~a~1]
 * Checks it.
 */
TEST(C, D) {}

/* [utest->dsn~a~1]
 */
TEST(E, F) {}

/* [utest->dsn~a~1] */ TEST(G, H) {}
",
        );
        assert_eq!(
            test_names(&scanned),
            [
                Some("A.B".to_owned()),
                Some("C.D".to_owned()),
                Some("E.F".to_owned()),
                Some("G.H".to_owned()),
            ]
        );
        let lines: Vec<_> = scanned
            .items
            .iter()
            .map(|item| test(item).unwrap().line)
            .collect();
        assert_eq!(lines, [2, 8, 12, 14]);
    }

    #[test]
    fn finds_rust_tests_around_attributes() {
        let scanned = scan_at(
            "src/lib.rs",
            "\
/* [utest->dsn~a~1] */ #[test] fn inline() {}

#[test] // [utest->dsn~a~1]
fn after_attribute() {}

#[test]
// [utest->dsn~a~1]
fn tag_between() {}

// [utest->dsn~a~1]
#[test_case(vec![1, 2],
    3)]
fn cased(a: Vec<u8>, b: u8) {}

// [utest->dsn~a~1]
#[rstest(
    case(vec![1]),
)]
fn rs() {}

// [utest->dsn~a~1]
#[
  test
]
fn spread() {}
",
        );
        assert_eq!(
            test_names(&scanned),
            [
                Some("inline".to_owned()),
                Some("after_attribute".to_owned()),
                Some("tag_between".to_owned()),
                Some("cased".to_owned()),
                Some("rs".to_owned()),
                Some("spread".to_owned()),
            ]
        );
    }

    #[test]
    fn module_paths_ignore_braces_in_literals() {
        let scanned = scan_at(
            "src/lib.rs",
            "\
mod tests {
    fn a() { let c = '{'; let s = \"}\"; let r = r#\"{\"#; let l: &'static str = \"//\"; }
    /* { */
    // [utest->dsn~a~1]
    #[test]
    fn inner() {}
}

#[cfg(test)] mod more {
    // [utest->dsn~a~1]
    #[test]
    fn t() {}
}

// [utest->dsn~a~1]
#[test]
fn top() {}
",
        );
        assert_eq!(
            test_names(&scanned),
            [
                Some("tests::inner".to_owned()),
                Some("more::t".to_owned()),
                Some("top".to_owned()),
            ]
        );
    }

    #[test]
    fn ids_survive_target_revision_updates() {
        let old = ids(&scan_str("// [impl->dsn~a~1]\n"));
        let new = ids(&scan_str("// [impl->dsn~a~2]\n"));
        assert_eq!(old, new);
    }

    #[test]
    fn rejects_revisions_with_leading_zeros() {
        let scanned = scan_str("// [impl~x~01->dsn~a~1]\n");
        assert_eq!(scanned.items, []);
        assert!(
            scanned.diagnostics[0]
                .message
                .contains("invalid revision '01'"),
            "{:?}",
            scanned.diagnostics
        );
    }

    #[test]
    fn several_tags_share_the_following_test() {
        let scanned =
            scan_str("// [utest->dsn~a~1]\n// [utest->dsn~b~1] [utest->dsn~c~1]\nTEST(S, N) {}\n");
        assert_eq!(
            test_names(&scanned),
            [
                Some("S.N".to_owned()),
                Some("S.N".to_owned()),
                Some("S.N".to_owned())
            ]
        );
    }
}
