//! Export of the requirements in standard OpenFastTrace syntax, for a
//! cross-check with OFT.
//!
//! Every requirement file becomes a Markdown file at the same path with
//! the items of that file, in their order. The extensions of `shall` are
//! rewritten into what OFT knows:
//!
//! - `Needs:` lists the types the item needs, also those its level needs
//!   by default, since OFT has no levels;
//! - a valid `Satisfied-by:` link becomes a `Covers:` link in its target,
//!   back to the item that names it, which then needs the target's type;
//! - `Params:`, `Source:` and `Features:` become a list in `Comment:`;
//! - `Status:` comes on the line right after the ID, since OFT does not
//!   read it after the text.
//!
//! A forwarding such as `sys --> swr : hlr~x~1` is OFT syntax and stays
//! one, unless it is the target of a `Satisfied-by:`: it then becomes an
//! item with both `Covers:` links.
//!
//! Text between the items, such as headings without an ID, is not part
//! of the export. Inside an item, OFT ends the item at any heading and
//! reads IDs and keywords also in code blocks, unlike `shall`; so every
//! line of a text that starts like a heading, an ID, a keyword or a code
//! fence is indented by one space, which Markdown still shows the same.
//! Every file starts with [`MARKER`], by which `shall` skips exported
//! files when it reads a project. Code and test tags need no export: their syntax is
//! OFT's.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use crate::item::{Item, Location, Origin, Reference};
use crate::trace::{LinkKind, LinkStatus, Trace};

/// The first line of every exported file.
pub const MARKER: &str = "<!-- Exported by shall for OpenFastTrace. -->";

/// The requirement files of `items` in OFT syntax, by path.
///
/// `trace` is the trace of `items`; rejected items have no needs there.
pub fn export(items: &[Item], trace: &Trace) -> BTreeMap<String, String> {
    let needs: HashMap<(&Location, &crate::ItemId), &[crate::ItemType]> = trace
        .items
        .iter()
        .map(|traced| ((&traced.location, &traced.id), traced.needs.as_slice()))
        .collect();
    // Covers links that `Satisfied-by:` adds to its targets.
    let mut added: HashMap<&Location, Vec<Reference>> = HashMap::new();
    for link in &trace.links {
        if link.kind == LinkKind::SatisfiedBy
            && link.status == LinkStatus::Valid
            && let Some(target) = &link.target_location
        {
            added.entry(target).or_default().push(Reference {
                id: link.source.clone(),
                line: link.location.line(),
            });
        }
    }
    let mut requirements: Vec<&Item> = items
        .iter()
        .filter(|item| item.origin == Origin::Requirement)
        .collect();
    requirements.sort_by(|a, b| a.location.cmp(&b.location));
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for item in requirements {
        let out = files
            .entry(item.location.path().to_owned())
            .or_insert_with(|| format!("{MARKER}\n"));
        let needs = needs
            .get(&(&item.location, &item.id))
            .copied()
            .unwrap_or(&item.needs);
        let extra = added.get(&item.location).map_or(&[][..], Vec::as_slice);
        // A forwarding that is a target of `Satisfied-by:` becomes an item
        // that covers the targets of both.
        if item.forwarded && extra.is_empty() {
            let needs: Vec<&str> = item.needs.iter().map(crate::ItemType::as_str).collect();
            let _ = writeln!(
                out,
                "\n{} --> {} : {}",
                item.id.typ(),
                needs.join(", "),
                item.covers[0].id
            );
            continue;
        }
        write_item(out, item, needs, extra);
    }
    files
}

/// The lines of a text, those that OFT would read as a heading, an ID, a
/// keyword or a code fence indented by one space.
fn text(text: &str) -> String {
    text.lines()
        .map(|line| {
            if structural(line) {
                format!(" {line}")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether OFT could read the line as more than text.
fn structural(line: &str) -> bool {
    if line.starts_with(['#', '`', '~']) {
        return true;
    }
    // An ID such as `swr~x~1`, or a keyword such as `Needs:`.
    let word: String = line
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '~' || *c == '.')
        .collect();
    word.contains('~') || line[word.len()..].starts_with(':') && !word.is_empty()
}

fn write_item(out: &mut String, item: &Item, needs: &[crate::ItemType], extra: &[Reference]) {
    out.push('\n');
    if !item.title.is_empty() {
        let _ = writeln!(out, "## {}", item.title);
    }
    let _ = writeln!(out, "`{}`", item.id);
    let _ = writeln!(out, "Status: {}", item.status);
    if !item.description.is_empty() {
        let _ = writeln!(out, "\n{}", text(&item.description));
    }
    if let Some(rationale) = item.rationale.as_deref().filter(|text| !text.is_empty()) {
        let _ = writeln!(out, "\nRationale:\n{}", text(rationale));
    }
    // As list entries, so that no line starts with a keyword.
    let mut extensions: Vec<String> = Vec::new();
    if let Some(source) = &item.source {
        extensions.push(format!("- Source: {source}"));
    }
    if !item.features.is_empty() {
        extensions.push(format!("- Features: {}", item.features.join(", ")));
    }
    for param in &item.params {
        extensions.push(format!("- Param {} = {}", param.name, param.value));
    }
    let mut comment: Vec<String> = item
        .comment
        .iter()
        .filter(|text| !text.is_empty())
        .map(|comment| text(comment))
        .collect();
    if !extensions.is_empty() {
        comment.push(extensions.join("\n"));
    }
    if !comment.is_empty() {
        let _ = writeln!(out, "\nComment:\n{}", comment.join("\n\n"));
    }
    let mut covers: Vec<&Reference> = item.covers.iter().collect();
    for link in extra {
        if !covers.iter().any(|known| known.id == link.id) {
            covers.push(link);
        }
    }
    if !covers.is_empty() {
        out.push_str("\nCovers:\n");
        for link in covers {
            let _ = writeln!(out, "- {}", link.id);
        }
    }
    if !item.depends.is_empty() {
        out.push_str("\nDepends:\n");
        for link in &item.depends {
            let _ = writeln!(out, "- {}", link.id);
        }
    }
    if !needs.is_empty() {
        let names: Vec<&str> = needs.iter().map(crate::ItemType::as_str).collect();
        let _ = writeln!(out, "\nNeeds: {}", names.join(", "));
    }
    if !item.tags.is_empty() {
        let _ = writeln!(out, "\nTags: {}", item.tags.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::config::Config;
    use crate::{markdown, trace};

    const CONFIG: &str = "\
[[level]]
type = \"hlr\"
name = \"H\"
needs = [\"swr\"]

[[level]]
type = \"cap\"
name = \"C\"
needs = [\"impl\"]
";

    #[test]
    fn rewrites_the_extensions_of_shall() {
        let config = Config::parse(CONFIG).unwrap();
        let mut items = markdown::parse(
            Path::new("a/reqs.md"),
            "# H\n`hlr~h~1`\n\nThe drive stops.\n\nSource: spec 4.2\nFeatures: x\n\
             Satisfied-by:\n- cap~c~1\n\nParams:\n- limit = 85\n",
        )
        .unwrap()
        .items;
        items.extend(
            markdown::parse(Path::new("p/caps.md"), "# C\n`cap~c~1`\nStatus: draft\n")
                .unwrap()
                .items,
        );
        let trace = trace::trace(&items, &config);
        let files = export(&items, &trace);
        assert_eq!(
            files["a/reqs.md"],
            "<!-- Exported by shall for OpenFastTrace. -->\n\n## H\n`hlr~h~1`\n\
             Status: approved\n\nThe drive stops.\n\nComment:\n- Source: spec 4.2\n\
             - Features: x\n- Param limit = 85\n\nNeeds: cap\n"
        );
        assert_eq!(
            files["p/caps.md"],
            "<!-- Exported by shall for OpenFastTrace. -->\n\n## C\n`cap~c~1`\n\
             Status: draft\n\nCovers:\n- hlr~h~1\n\nNeeds: impl\n"
        );
    }

    #[test]
    fn the_export_traces_like_the_original_without_levels() {
        let config = Config::parse(CONFIG).unwrap();
        let text = "# H\n`hlr~h~1`\nSatisfied-by:\n- cap~c~1\n\n# C\n`cap~c~1`\n\n\
                    # D\n`hlr~d~1`\nNeeds: swr\n\nswr --> impl, utest : hlr~d~1\n";
        let items = markdown::parse(Path::new("r.md"), text).unwrap().items;
        let original = trace::trace(&items, &config);
        let files = export(&items, &original);
        assert!(
            files["r.md"].contains("\n\nswr --> impl, utest : hlr~d~1\n"),
            "{}",
            files["r.md"]
        );
        let exported = markdown::parse(Path::new("r.md"), &files["r.md"]).unwrap();
        assert_eq!(exported.diagnostics, []);
        // OFT has no levels: the exported needs stand on their own.
        let again = trace::trace(&exported.items, &Config::default());
        let coverage = |trace: &Trace| -> Vec<(String, bool, Vec<String>)> {
            trace
                .items
                .iter()
                .map(|item| {
                    (
                        item.id.to_string(),
                        item.covered,
                        item.missing.iter().map(ToString::to_string).collect(),
                    )
                })
                .collect()
        };
        assert_eq!(coverage(&again), coverage(&original));
    }

    #[test]
    fn a_satisfying_forwarding_becomes_an_item() {
        let config = Config::parse(CONFIG).unwrap();
        let text = "# H\n`hlr~h~1`\nSatisfied-by:\n- cap~c~1\n\n# G\n`hlr~c~1`\n\n\
                    cap --> impl : hlr~c~1\n";
        let items = markdown::parse(Path::new("r.md"), text).unwrap().items;
        let files = export(&items, &trace::trace(&items, &config));
        assert!(
            files["r.md"].ends_with(
                "\n\n`cap~c~1`\nStatus: approved\n\nCovers:\n- hlr~c~1\n- hlr~h~1\n\nNeeds: impl\n"
            ),
            "{}",
            files["r.md"]
        );
    }

    #[test]
    fn indents_what_oft_would_read_as_structure() {
        assert_eq!(
            text(
                "Intro.\n### Details\n```\n`swr~fake~1`\nNeeds: impl\n```\nNote: x\nswr~y~1 is it\nplain: no\nPlain text."
            ),
            "Intro.\n ### Details\n ```\n `swr~fake~1`\n Needs: impl\n ```\n Note: x\n swr~y~1 is it\n plain: no\nPlain text."
        );
    }
}
