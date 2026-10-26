//! Import of items from Markdown files in OpenFastTrace syntax.
//!
//! An item starts with a line that begins with its ID at column 0,
//! optionally in backticks; the rest of the line is ignored, as in
//! OpenFastTrace (OFT). The heading directly above the ID becomes its
//! title. The item ends at the next ID line or at a heading of the same or
//! a higher level than its title; a deeper heading directly followed by an
//! ID starts a nested item with that heading as title.
//!
//! ```markdown
//! ## Shut down the motor on overtemperature
//! `swr~motor.overtemp-shutdown-k4f2~2`
//!
//! The software shuts down the motor within 100 ms.
//!
//! Rationale:
//! Protects the winding.
//!
//! Status: approved
//!
//! Covers:
//! - sys~motor.thermal-protection-q7m2~1
//!
//! Needs: dsn
//! ```
//!
//! Text between the ID and the first keyword is the description; an
//! explicit `Description:` is allowed. `Rationale:` and `Comment:` take
//! the text that follows them, `Covers:` and `Depends:` a bullet list of
//! IDs, `Needs:` and `Tags:` a comma-separated list on the same line or a
//! bullet list, and `Status:` and `Source:` a value on the same line. Lines
//! inside fenced code blocks are never IDs or keywords.
//!
//! Known differences from OFT, relevant for cross-checks with OFT:
//!
//! - shall accepts every keyword anywhere in an item and values on the
//!   same line as `Rationale:` and `Comment:`. OFT reads `Status:` only
//!   directly after the ID and a value after `Rationale:` as text.
//! - A heading deeper than the item's title does not end the item.
//! - shall skips fenced code blocks inside items; OFT only outside items.
//! - Setext headings (underlined with `===` or `---`) are not recognised
//!   as headings yet.

use std::collections::HashMap;
use std::path::Path;

use crate::diagnostic::Diagnostic;
use crate::id::{ItemId, ItemType};
use crate::item::{Item, Location, LocationError, Reference, Status};

/// Items and diagnostics read from one Markdown file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// Items in the order they appear in the file.
    pub items: Vec<Item>,
    /// Problems found in the file, in the order of their lines.
    pub diagnostics: Vec<Diagnostic>,
}

/// Parses the Markdown `text` of the file at `path`, which is relative to
/// the project root and used for the locations of items and diagnostics.
pub fn parse(path: &Path, text: &str) -> Result<Parsed, LocationError> {
    // Validate the path once, so that every later location is valid too.
    let file = Location::new(path, 1)?;
    let mut parser = Parser {
        file,
        parsed: Parsed::default(),
        item: None,
        heading: None,
        deferred: None,
        fence: None,
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for (index, line) in text.lines().enumerate() {
        parser.line(index + 1, line);
    }
    if let Some(fence) = parser.fence {
        parser.error(
            fence.line,
            "code block is never closed; the rest of the file is read as code",
        );
    }
    parser.finish_item();
    parser
        .parsed
        .diagnostics
        .sort_by_key(|diagnostic| diagnostic.location.line());
    Ok(parser.parsed)
}

/// Keywords that start a section of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Keyword {
    Description,
    Rationale,
    Comment,
    Covers,
    Depends,
    Needs,
    Tags,
    Status,
    Source,
}

impl Keyword {
    const ALL: [Keyword; 9] = [
        Keyword::Description,
        Keyword::Rationale,
        Keyword::Comment,
        Keyword::Covers,
        Keyword::Depends,
        Keyword::Needs,
        Keyword::Tags,
        Keyword::Status,
        Keyword::Source,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Keyword::Description => "Description",
            Keyword::Rationale => "Rationale",
            Keyword::Comment => "Comment",
            Keyword::Covers => "Covers",
            Keyword::Depends => "Depends",
            Keyword::Needs => "Needs",
            Keyword::Tags => "Tags",
            Keyword::Status => "Status",
            Keyword::Source => "Source",
        }
    }

    /// Whether the keyword may appear only once per item. List keywords
    /// may repeat; their entries are collected.
    fn is_single(self) -> bool {
        matches!(
            self,
            Keyword::Description
                | Keyword::Rationale
                | Keyword::Comment
                | Keyword::Status
                | Keyword::Source
        )
    }
}

/// Where the lines following the current line go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Description,
    Rationale,
    Comment,
    /// Bullet list of IDs or types after `Covers:`, `Depends:`, `Needs:`
    /// or `Tags:`.
    List(Keyword),
    /// After a keyword whose value is complete; further text belongs to
    /// the description.
    Done,
    /// After a repeated single keyword, whose text is dropped.
    Ignored,
}

/// The item being read.
#[derive(Debug)]
struct ItemBuilder {
    id: ItemId,
    title: String,
    heading_level: Option<usize>,
    line: usize,
    description: Vec<String>,
    rationale: Option<Vec<String>>,
    comment: Option<Vec<String>>,
    status: Option<Status>,
    needs: Vec<ItemType>,
    covers: Vec<Reference>,
    depends: Vec<Reference>,
    tags: Vec<String>,
    source: Option<String>,
    section: Section,
    seen: HashMap<Keyword, usize>,
}

/// An open fenced code block: its fence character, length and line.
#[derive(Debug, Clone, Copy)]
struct Fence {
    marker: u8,
    len: usize,
    line: usize,
}

/// A heading inside an item that is deeper than the item's title. It
/// becomes the title of the next item if an ID follows, otherwise text.
#[derive(Debug)]
struct Deferred {
    level: usize,
    title: String,
    /// The heading line and the blank lines after it.
    lines: Vec<String>,
}

struct Parser {
    file: Location,
    parsed: Parsed,
    item: Option<ItemBuilder>,
    /// Last heading outside an item that may become the next item's title.
    heading: Option<(usize, String)>,
    deferred: Option<Deferred>,
    fence: Option<Fence>,
}

impl Parser {
    fn location(&self, line: usize) -> Location {
        Location::new(Path::new(self.file.path()), to_u32(line))
            .expect("the path was validated when parsing started")
    }

    fn error(&mut self, line: usize, message: impl Into<String>) {
        let diagnostic = Diagnostic::error(self.location(line), message);
        self.parsed.diagnostics.push(diagnostic);
    }

    fn warning(&mut self, line: usize, message: impl Into<String>) {
        let diagnostic = Diagnostic::warning(self.location(line), message);
        self.parsed.diagnostics.push(diagnostic);
    }

    fn line(&mut self, number: usize, line: &str) {
        if let Some(fence) = self.fence {
            if closes_fence(line, fence) {
                self.fence = None;
            }
            self.text(line);
            return;
        }
        if let Some(deferred) = &mut self.deferred {
            if line.trim().is_empty() {
                deferred.lines.push(line.to_owned());
                return;
            }
            if id_candidate(line).is_none() {
                self.flush_deferred();
            }
        }
        if let Some(fence) = opens_fence(line, number) {
            self.fence = Some(fence);
            self.heading = None;
            self.text(line);
            return;
        }
        if let Some(candidate) = id_candidate(line) {
            match candidate.parse::<ItemId>() {
                Ok(id) => self.start_item(number, id),
                Err(error) => {
                    // The lines that follow belong to the broken item, so
                    // they must not end up in the previous one.
                    self.deferred = None;
                    self.finish_item();
                    self.heading = None;
                    self.error(
                        number,
                        format!("{error}; lines up to the next item are ignored"),
                    );
                }
            }
            return;
        }
        if let Some((level, title)) = heading(line) {
            let nested = self
                .item
                .as_ref()
                .is_some_and(|item| item.heading_level.is_some_and(|own| level > own));
            if nested {
                self.deferred = Some(Deferred {
                    level,
                    title: title.to_owned(),
                    lines: vec![line.to_owned()],
                });
            } else {
                self.finish_item();
                self.heading = Some((level, title.to_owned()));
            }
            return;
        }
        if self.item.is_none() {
            if !line.trim().is_empty() {
                self.heading = None;
            }
            return;
        }
        if let Some((keyword, value)) = keyword(line) {
            self.keyword(number, keyword, value.trim());
            return;
        }
        if let Some((word, suggestion)) = misspelled_keyword(line) {
            self.warning(
                number,
                format!(
                    "'{word}:' is not a keyword, did you mean '{suggestion}:'? \
                     The line is read as text"
                ),
            );
        }
        self.content(number, line);
    }

    /// Adds a deferred heading and the blank lines after it as text.
    fn flush_deferred(&mut self) {
        if let Some(deferred) = self.deferred.take() {
            for line in &deferred.lines {
                self.text(line);
            }
        }
    }

    /// Appends a line of free text, e.g. inside a code block.
    fn text(&mut self, line: &str) {
        let Some(item) = &mut self.item else { return };
        match item.section {
            Section::Description | Section::Done | Section::List(_) => {
                item.description.push(line.to_owned());
            }
            Section::Rationale => push_text(&mut item.rationale, line),
            Section::Comment => push_text(&mut item.comment, line),
            Section::Ignored => {}
        }
    }

    /// Handles a line inside an item that is no ID, heading or keyword.
    fn content(&mut self, number: usize, line: &str) {
        let Some(item) = &self.item else { return };
        let Section::List(keyword) = item.section else {
            self.text(line);
            return;
        };
        if line.trim().is_empty() {
            return;
        }
        match bullet(line) {
            Some(entry) => self.list_entry(number, keyword, entry),
            None => self.error(
                number,
                format!(
                    "expected a list entry like '- ...' after '{}:'",
                    keyword.as_str()
                ),
            ),
        }
    }

    fn start_item(&mut self, number: usize, id: ItemId) {
        let nested = self.deferred.take();
        self.finish_item();
        let (heading_level, title) = match nested
            .map(|deferred| (deferred.level, deferred.title))
            .or_else(|| self.heading.take())
        {
            Some((level, title)) => (Some(level), title),
            None => (None, String::new()),
        };
        self.item = Some(ItemBuilder {
            id,
            title,
            heading_level,
            line: number,
            description: Vec::new(),
            rationale: None,
            comment: None,
            status: None,
            needs: Vec::new(),
            covers: Vec::new(),
            depends: Vec::new(),
            tags: Vec::new(),
            source: None,
            section: Section::Description,
            seen: HashMap::new(),
        });
    }

    fn keyword(&mut self, number: usize, keyword: Keyword, value: &str) {
        let Some(item) = &mut self.item else { return };
        if keyword.is_single()
            && let Some(&first) = item.seen.get(&keyword)
        {
            item.section = Section::Ignored;
            self.error(
                number,
                format!(
                    "duplicate keyword '{}:', first used in line {first}",
                    keyword.as_str()
                ),
            );
            return;
        }
        item.seen.entry(keyword).or_insert(number);
        match keyword {
            Keyword::Description => {
                item.section = Section::Description;
                if !value.is_empty() {
                    item.description.push(value.to_owned());
                }
            }
            Keyword::Rationale => {
                item.section = Section::Rationale;
                item.rationale = Some(Vec::new());
                if !value.is_empty() {
                    push_text(&mut item.rationale, value);
                }
            }
            Keyword::Comment => {
                item.section = Section::Comment;
                item.comment = Some(Vec::new());
                if !value.is_empty() {
                    push_text(&mut item.comment, value);
                }
            }
            Keyword::Status => {
                item.section = Section::Done;
                match value.parse() {
                    Ok(status) => item.status = Some(status),
                    Err(error) => self.error(number, error.to_string()),
                }
            }
            Keyword::Source => {
                item.section = Section::Done;
                if value.is_empty() {
                    self.error(number, "'Source:' needs a value on the same line");
                } else {
                    item.source = Some(value.to_owned());
                }
            }
            Keyword::Covers | Keyword::Depends => {
                item.section = Section::List(keyword);
                if !value.is_empty() {
                    self.error(
                        number,
                        format!(
                            "'{}:' expects a list of IDs in the following lines",
                            keyword.as_str()
                        ),
                    );
                }
            }
            Keyword::Needs | Keyword::Tags => {
                if value.is_empty() {
                    item.section = Section::List(keyword);
                } else {
                    item.section = Section::Done;
                    for entry in value.split(',') {
                        self.list_entry(number, keyword, entry.trim());
                    }
                }
            }
        }
    }

    fn list_entry(&mut self, number: usize, keyword: Keyword, entry: &str) {
        let entry = match keyword {
            Keyword::Tags => strip_backticks(entry.trim()),
            _ => reference(entry),
        };
        if entry.is_empty() {
            self.error(number, format!("empty entry in '{}:'", keyword.as_str()));
            return;
        }
        let item = self.item.as_mut().expect("lists only exist in items");
        let added = match keyword {
            Keyword::Covers | Keyword::Depends => entry.parse::<ItemId>().map(|id| {
                let references = if keyword == Keyword::Covers {
                    &mut item.covers
                } else {
                    &mut item.depends
                };
                let new = !references.iter().any(|existing| existing.id == id);
                if new {
                    let line = to_u32(number);
                    references.push(Reference { id, line });
                }
                new
            }),
            Keyword::Needs => entry.parse::<ItemType>().map(|typ| {
                let new = !item.needs.contains(&typ);
                if new {
                    item.needs.push(typ);
                }
                new
            }),
            Keyword::Tags => {
                let new = !item.tags.iter().any(|tag| tag == entry);
                if new {
                    item.tags.push(entry.to_owned());
                }
                Ok(new)
            }
            _ => unreachable!("only list keywords have entries"),
        };
        match added {
            Ok(true) => {}
            Ok(false) => self.warning(
                number,
                format!("duplicate entry '{entry}' in '{}:'", keyword.as_str()),
            ),
            Err(error) => self.error(number, error.to_string()),
        }
    }

    fn finish_item(&mut self) {
        self.flush_deferred();
        let Some(item) = self.item.take() else { return };
        let location = self.location(item.line);
        self.parsed.items.push(Item {
            id: item.id,
            title: item.title,
            description: join_text(&item.description),
            rationale: item.rationale.as_deref().map(join_text),
            comment: item.comment.as_deref().map(join_text),
            status: item.status.unwrap_or_default(),
            needs: item.needs,
            covers: item.covers,
            depends: item.depends,
            tags: item.tags,
            source: item.source,
            location,
        });
    }
}

fn to_u32(line: usize) -> u32 {
    u32::try_from(line).expect("files have fewer than 2^32 lines")
}

fn push_text(text: &mut Option<Vec<String>>, line: &str) {
    text.get_or_insert_with(Vec::new).push(line.to_owned());
}

/// Joins lines, dropping leading and trailing blank lines.
fn join_text(lines: &[String]) -> String {
    let is_text = |line: &&String| !line.trim().is_empty();
    let start = lines.iter().position(|line| is_text(&line));
    let end = lines.iter().rposition(|line| is_text(&line));
    match (start, end) {
        (Some(start), Some(end)) => lines[start..=end].join("\n"),
        _ => String::new(),
    }
}

/// Returns the ID at the start of a line, where OpenFastTrace looks for
/// it: at column 0, optionally in backticks, followed by anything. Only
/// the shape `type~name` is checked here, so that a mistyped ID such as
/// `req~x` or `l3~x~1` is reported instead of silently read as text.
fn id_candidate(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('`').unwrap_or(line);
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '`')
        .unwrap_or(rest.len());
    let token = &rest[..end];
    let (typ, name) = token.split_once('~')?;
    let looks_like_id = typ.starts_with(|c: char| c.is_ascii_alphabetic())
        && typ.bytes().all(|b| b.is_ascii_alphanumeric())
        && name.starts_with(|c: char| c.is_ascii_alphabetic());
    looks_like_id.then_some(token)
}

/// Returns level and text of an ATX heading such as `## Title ##`.
fn heading(line: &str) -> Option<(usize, &str)> {
    let rest = strip_indent(line)?;
    let level = rest.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let text = &rest[level..];
    if !text.is_empty() && !text.starts_with([' ', '\t']) {
        return None;
    }
    let text = text.trim();
    // A closing sequence of `#` must be separated by a space; otherwise it
    // belongs to the text, as in `## C#`.
    let without_closing = text.trim_end_matches('#');
    let text = if without_closing.is_empty() || without_closing.ends_with([' ', '\t']) {
        without_closing.trim_end()
    } else {
        text
    };
    Some((level, text))
}

/// Strips up to three spaces of indentation, as CommonMark allows for
/// headings and fences; more indentation makes an indented code block.
fn strip_indent(line: &str) -> Option<&str> {
    let spaces = line.bytes().take_while(|&b| b == b' ').count();
    (spaces <= 3).then(|| &line[spaces..])
}

fn opens_fence(line: &str, number: usize) -> Option<Fence> {
    let rest = strip_indent(line)?;
    let marker = *rest.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let len = rest.bytes().take_while(|&b| b == marker).count();
    // A backtick fence's info string must not contain backticks.
    let valid = len >= 3 && (marker == b'~' || !rest[len..].contains('`'));
    valid.then_some(Fence {
        marker,
        len,
        line: number,
    })
}

fn closes_fence(line: &str, fence: Fence) -> bool {
    let Some(rest) = strip_indent(line) else {
        return false;
    };
    let len = rest.bytes().take_while(|&b| b == fence.marker).count();
    len >= fence.len && rest[len..].trim().is_empty()
}

/// Returns the keyword and the rest of the line for lines like `Needs: dsn`.
fn keyword(line: &str) -> Option<(Keyword, &str)> {
    let (word, value) = line.split_once(':')?;
    let keyword = Keyword::ALL.into_iter().find(|k| k.as_str() == word)?;
    Some((keyword, value))
}

/// Detects a line starting with a word and a colon that differs from a
/// keyword only in case or by one letter, e.g. `needs:` or `Neds:`. The
/// line stays text, since words like `States:` can be ordinary prose.
fn misspelled_keyword(line: &str) -> Option<(&str, &'static str)> {
    let (word, _) = line.split_once(':')?;
    if word.is_empty() || !word.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-') {
        return None;
    }
    let lower = word.to_ascii_lowercase();
    Keyword::ALL
        .into_iter()
        .map(Keyword::as_str)
        .find(|keyword| {
            let distance = edit_distance(&lower, &keyword.to_ascii_lowercase());
            distance == 0 || (distance == 1 && keyword.len() > 4)
        })
        .map(|keyword| (word, keyword))
}

/// Levenshtein distance between two ASCII strings.
fn edit_distance(a: &str, b: &str) -> usize {
    let b = b.as_bytes();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, &ca) in a.as_bytes().iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, &cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != cb);
            current.push(substitution.min(previous[j + 1] + 1).min(current[j] + 1));
        }
        previous = current;
    }
    previous[b.len()]
}

/// Returns the text of a bullet list entry such as `- swr~x~1`.
fn bullet(line: &str) -> Option<&str> {
    let rest = line.trim_start();
    let rest = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))?;
    Some(rest.trim())
}

/// Returns the ID of a list entry. As in OpenFastTrace, text after the ID
/// is ignored and the ID may be the text of a Markdown link, e.g.
/// `[sys~x~1](system.md#x) (parent)`.
fn reference(entry: &str) -> &str {
    let entry = entry.trim();
    let entry = entry
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("]("))
        .map_or(entry, |(text, _)| text);
    let token = entry.split_whitespace().next().unwrap_or("");
    strip_backticks(token)
}

fn strip_backticks(text: &str) -> &str {
    text.strip_prefix('`')
        .and_then(|rest| rest.strip_suffix('`'))
        .unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(text: &str) -> Parsed {
        parse(Path::new("reqs/x.md"), text).unwrap()
    }

    fn messages(parsed: &Parsed) -> Vec<String> {
        parsed.diagnostics.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn reads_all_keywords() {
        let parsed = parse_str(
            "\
# Software

## Shut down on overtemperature
`swr~motor.overtemp-k4f2~2`

The software shuts down the motor
within 100 ms.

Rationale:
Protects the winding.

Comment: Checked with the thermal model.

Status: proposed
Source: Customer spec 4.2
Tags: safety, `thermal`

Covers:
- sys~motor.thermal-q7m2~1
* `sys~motor.limits-h3x9~3`

Depends:
+ swr~motor.sensor-c2v6~1

Needs: dsn, utest
",
        );
        assert_eq!(messages(&parsed), Vec::<String>::new());
        let [item] = &parsed.items[..] else {
            panic!("expected one item: {:?}", parsed.items)
        };
        assert_eq!(item.id.to_string(), "swr~motor.overtemp-k4f2~2");
        assert_eq!(item.title, "Shut down on overtemperature");
        assert_eq!(
            item.description,
            "The software shuts down the motor\nwithin 100 ms."
        );
        assert_eq!(item.rationale.as_deref(), Some("Protects the winding."));
        assert_eq!(
            item.comment.as_deref(),
            Some("Checked with the thermal model.")
        );
        assert_eq!(item.status, Status::Proposed);
        assert_eq!(item.source.as_deref(), Some("Customer spec 4.2"));
        assert_eq!(item.tags, ["safety", "thermal"]);
        let covers: Vec<_> = item
            .covers
            .iter()
            .map(|r| (r.id.to_string(), r.line))
            .collect();
        assert_eq!(
            covers,
            [
                ("sys~motor.thermal-q7m2~1".to_owned(), 19),
                ("sys~motor.limits-h3x9~3".to_owned(), 20),
            ]
        );
        assert_eq!(item.depends[0].id.to_string(), "swr~motor.sensor-c2v6~1");
        let needs: Vec<_> = item.needs.iter().map(ItemType::as_str).collect();
        assert_eq!(needs, ["dsn", "utest"]);
        assert_eq!(item.location.to_string(), "reqs/x.md:4");
    }

    #[test]
    fn heading_of_same_level_ends_item() {
        let parsed = parse_str(
            "\
## First
req~first~1

Text of first.

### Detail
Still first.

## Second
`req~second~1`

## Not an item

Text outside of items.
",
        );
        assert_eq!(messages(&parsed), Vec::<String>::new());
        let [first, second] = &parsed.items[..] else {
            panic!("expected two items: {:?}", parsed.items)
        };
        assert_eq!(first.title, "First");
        assert_eq!(
            first.description,
            "Text of first.\n\n### Detail\nStill first."
        );
        assert_eq!(second.title, "Second");
        assert_eq!(second.description, "");
    }

    #[test]
    fn title_must_directly_precede_id() {
        let parsed = parse_str("## Heading\n\nSome text.\n\n`req~x~1`\n");
        assert_eq!(parsed.items[0].title, "");
        let parsed = parse_str("## Heading\n\n`req~x~1`\n");
        assert_eq!(parsed.items[0].title, "Heading");
    }

    #[test]
    fn item_without_heading_ends_at_any_heading() {
        let parsed = parse_str("`req~x~1`\nText.\n#### Deep\nMore.\n");
        assert_eq!(parsed.items[0].description, "Text.");
    }

    #[test]
    fn ignores_ids_and_keywords_in_code_blocks() {
        let parsed = parse_str(
            "\
## Item
`req~x~1`

````markdown
`req~example~1`
```
Needs: impl
```
````

~~~
Covers:
~~~
Needs: impl
",
        );
        assert_eq!(messages(&parsed), Vec::<String>::new());
        let [item] = &parsed.items[..] else {
            panic!("expected one item: {:?}", parsed.items)
        };
        assert!(item.description.contains("`req~example~1`"));
        assert!(item.covers.is_empty());
        assert_eq!(item.needs.len(), 1);
    }

    #[test]
    fn code_block_before_id_clears_title() {
        let parsed = parse_str("## Example\n```\ncode\n```\n`req~x~1`\n");
        assert_eq!(parsed.items[0].title, "");
    }

    #[test]
    fn reports_invalid_ids() {
        let parsed = parse_str(
            "\
`l3~x~1`

## Item
`req~x~1`

Covers:
- l3d~y~1
- req~y

Needs: impl, l3
",
        );
        assert_eq!(
            messages(&parsed),
            [
                "reqs/x.md:1: invalid type 'l3': expected ASCII letters only; \
                 lines up to the next item are ignored",
                "reqs/x.md:7: invalid type 'l3d': expected ASCII letters only",
                "reqs/x.md:8: invalid ID 'req~y': expected 'typ~name~revision'",
                "reqs/x.md:10: invalid type 'l3': expected ASCII letters only",
            ]
        );
        assert_eq!(parsed.items.len(), 1);
        assert!(parsed.items[0].covers.is_empty());
        assert_eq!(parsed.items[0].needs.len(), 1);
    }

    #[test]
    fn invalid_id_ends_previous_item() {
        let parsed = parse_str(
            "\
`req~first~1`
Text.
`req~second`
Covers:
- req~other~1

## Next
`req~second~x`
Needs: impl
",
        );
        assert_eq!(
            messages(&parsed),
            [
                "reqs/x.md:3: invalid ID 'req~second': expected 'typ~name~revision'; \
                 lines up to the next item are ignored",
                "reqs/x.md:8: invalid revision 'x': expected a number without leading zeros, \
                 at most 2147483647; lines up to the next item are ignored",
            ]
        );
        let [first] = &parsed.items[..] else {
            panic!("expected one item: {:?}", parsed.items)
        };
        assert_eq!(first.description, "Text.");
        assert!(first.covers.is_empty());
        assert!(first.needs.is_empty());
    }

    #[test]
    fn detects_id_candidates() {
        assert_eq!(id_candidate("`req~x~1`"), Some("req~x~1"));
        assert_eq!(id_candidate("req~x~1 (draft)"), Some("req~x~1"));
        assert_eq!(id_candidate("`req~x~1` <!-- note -->"), Some("req~x~1"));
        assert_eq!(id_candidate("`req~x~1"), Some("req~x~1"));
        assert_eq!(id_candidate("req~x"), Some("req~x"));
        assert_eq!(id_candidate("l3~x~1"), Some("l3~x~1"));
        assert_eq!(id_candidate("  req~x~1"), None);
        assert_eq!(id_candidate("    `req~x~1`"), None);
        assert_eq!(id_candidate("~~"), None);
        assert_eq!(id_candidate("~~~"), None);
        assert_eq!(id_candidate("https://wiki.example.com/~team/spec"), None);
        assert_eq!(id_candidate("approx~5 ms"), None);
        assert_eq!(id_candidate("see req~x~1"), None);
        assert_eq!(id_candidate("- req~x~1"), None);
        assert_eq!(id_candidate("plain"), None);
    }

    #[test]
    fn reports_misspelled_keywords() {
        let parsed = parse_str(
            "\
`req~x~1`
Neds: impl
needs: impl
Rationle: because
Note: this is ordinary text.
Tag: x
",
        );
        assert_eq!(
            messages(&parsed),
            [
                "reqs/x.md:2: 'Neds:' is not a keyword, did you mean 'Needs:'? \
                 The line is read as text",
                "reqs/x.md:3: 'needs:' is not a keyword, did you mean 'Needs:'? \
                 The line is read as text",
                "reqs/x.md:4: 'Rationle:' is not a keyword, did you mean 'Rationale:'? \
                 The line is read as text",
            ]
        );
        assert!(
            parsed
                .diagnostics
                .iter()
                .all(|d| d.severity == crate::Severity::Warning)
        );
        assert_eq!(
            parsed.items[0].description,
            "Neds: impl\nneeds: impl\nRationle: because\nNote: this is ordinary text.\nTag: x"
        );
        assert!(parsed.items[0].needs.is_empty());
    }

    #[test]
    fn prose_with_colons_is_no_keyword() {
        let parsed =
            parse_str("`req~x~1`\nStates: idle, running\nContent: x\nResource: y\nCovered: z\n");
        // One letter away from `Status:`: a warning, but the text is kept.
        assert_eq!(
            messages(&parsed),
            [
                "reqs/x.md:2: 'States:' is not a keyword, did you mean 'Status:'? \
              The line is read as text"
            ]
        );
        assert_eq!(
            parsed.items[0].description,
            "States: idle, running\nContent: x\nResource: y\nCovered: z"
        );
    }

    #[test]
    fn reports_wrong_keyword_values() {
        let parsed = parse_str(
            "\
`req~x~1`
Status: accepted
Source:
Covers: req~y~1
Status: draft
Ignored text.
Covers:
- req~y~1
not a list entry
Needs: impl,
",
        );
        assert_eq!(
            messages(&parsed),
            [
                "reqs/x.md:2: unknown status 'accepted': expected draft, proposed, approved or rejected",
                "reqs/x.md:3: 'Source:' needs a value on the same line",
                "reqs/x.md:4: 'Covers:' expects a list of IDs in the following lines",
                "reqs/x.md:5: duplicate keyword 'Status:', first used in line 2",
                "reqs/x.md:9: expected a list entry like '- ...' after 'Covers:'",
                "reqs/x.md:10: empty entry in 'Needs:'",
            ]
        );
        let item = &parsed.items[0];
        assert_eq!(item.status, Status::Approved);
        assert_eq!(item.covers.len(), 1);
        assert_eq!(item.description, "");
    }

    #[test]
    fn lists_may_follow_needs_and_tags() {
        let parsed = parse_str("`req~x~1`\nNeds:\nNeeds:\n- impl\n\n- utest\nTags:\n* a\n");
        let item = &parsed.items[0];
        let needs: Vec<_> = item.needs.iter().map(ItemType::as_str).collect();
        assert_eq!(needs, ["impl", "utest"]);
        assert_eq!(item.tags, ["a"]);
    }

    #[test]
    fn text_after_single_keywords_belongs_to_description() {
        let parsed = parse_str("`req~x~1`\nFirst.\n\nStatus: draft\n\nSecond.\n");
        assert_eq!(parsed.items[0].description, "First.\n\n\nSecond.");
    }

    #[test]
    fn rejects_non_relative_paths() {
        assert!(parse(Path::new("/abs.md"), "").is_err());
    }

    #[test]
    fn deeper_heading_before_id_titles_nested_item() {
        let parsed = parse_str(
            "\
## Feature
`feat~parent~1`

Parent text.

### Child requirement

`req~child~1`
Child text.

#### Notes
Still the child.

### Sibling note
Outside of every item.
",
        );
        assert_eq!(messages(&parsed), Vec::<String>::new());
        let [parent, child] = &parsed.items[..] else {
            panic!("expected two items: {:?}", parsed.items)
        };
        assert_eq!(parent.description, "Parent text.");
        assert_eq!(child.title, "Child requirement");
        assert_eq!(
            child.description,
            "Child text.\n\n#### Notes\nStill the child."
        );
    }

    #[test]
    fn deferred_heading_at_end_of_file_is_text() {
        let parsed = parse_str("## A\n`req~a~1`\nText.\n### Trailing\n\n");
        assert_eq!(parsed.items[0].description, "Text.\n### Trailing");
    }

    #[test]
    fn reports_unclosed_code_block() {
        let parsed = parse_str("## A\n`req~a~1`\nText\n```\ncode\n\n## B\n`req~b~1`\n");
        assert_eq!(
            messages(&parsed),
            ["reqs/x.md:4: code block is never closed; the rest of the file is read as code"]
        );
    }

    #[test]
    fn id_line_may_have_trailing_text() {
        let parsed = parse_str("`req~a~1` <!-- note -->\nNeeds: impl\nreq~b~1 (draft)\n");
        assert_eq!(messages(&parsed), Vec::<String>::new());
        let ids: Vec<_> = parsed.items.iter().map(|i| i.id.to_string()).collect();
        assert_eq!(ids, ["req~a~1", "req~b~1"]);
        assert_eq!(parsed.items[0].needs.len(), 1);
    }

    #[test]
    fn urls_and_indented_ids_are_text() {
        let parsed = parse_str(
            "\
`req~a~1`
See
https://wiki.example.com/~team/spec

    `req~example~1`

Covers:
- sys~x~1
",
        );
        assert_eq!(messages(&parsed), Vec::<String>::new());
        let [item] = &parsed.items[..] else {
            panic!("expected one item: {:?}", parsed.items)
        };
        assert_eq!(item.covers.len(), 1);
        assert!(item.description.contains("    `req~example~1`"));
    }

    #[test]
    fn ignores_byte_order_mark() {
        let parsed = parse_str("\u{feff}`req~a~1`\n");
        assert_eq!(parsed.items[0].id.to_string(), "req~a~1");
        let parsed = parse_str("\u{feff}# Title\n`req~a~1`\n");
        assert_eq!(parsed.items[0].title, "Title");
    }

    #[test]
    fn reads_description_keyword() {
        let parsed = parse_str("`req~a~1`\nDescription: First line.\nSecond line.\n");
        assert_eq!(parsed.items[0].description, "First line.\nSecond line.");
    }

    #[test]
    fn list_entries_follow_oft() {
        let parsed = parse_str(
            "\
`req~a~1`
Covers:
- sys~x~1 (parent)
- [sys~y~1](system.md#y)
- `sys~z~1`
- sys~x~1
Needs: impl, impl
Tags: safety critical, safety critical
",
        );
        assert_eq!(
            messages(&parsed),
            [
                "reqs/x.md:6: duplicate entry 'sys~x~1' in 'Covers:'",
                "reqs/x.md:7: duplicate entry 'impl' in 'Needs:'",
                "reqs/x.md:8: duplicate entry 'safety critical' in 'Tags:'",
            ]
        );
        let item = &parsed.items[0];
        let covers: Vec<_> = item.covers.iter().map(|r| r.id.to_string()).collect();
        assert_eq!(covers, ["sys~x~1", "sys~y~1", "sys~z~1"]);
        assert_eq!(item.tags, ["safety critical"]);
    }

    #[test]
    fn parses_headings() {
        assert_eq!(heading("## Title"), Some((2, "Title")));
        assert_eq!(heading("   # Title #"), Some((1, "Title")));
        assert_eq!(heading("## C#"), Some((2, "C#")));
        assert_eq!(heading("###"), Some((3, "")));
        assert_eq!(heading("    # code"), None);
        assert_eq!(heading("#hashtag"), None);
        assert_eq!(heading("####### seven"), None);
    }

    #[test]
    fn computes_edit_distance() {
        assert_eq!(edit_distance("needs", "needs"), 0);
        assert_eq!(edit_distance("neds", "needs"), 1);
        assert_eq!(edit_distance("rationle", "rationale"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
    }
}
