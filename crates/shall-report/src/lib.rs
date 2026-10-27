//! HTML report of `shall`, generated from the exported graph
//! ([`shall_core::export::Document`]).
//!
//! The report is one self-contained HTML file, `index.html`, with its CSS
//! embedded, so that it opens from disk without a server. Like
//! `trace.json` it is deterministic: the same document gives the same
//! bytes.
//!
//! The start page has a summary and then one table per level:
//!
//! - the summary counts requirements, tags, test verdicts and defects,
//!   and shows the coverage of every level: how many of its items that
//!   are not rejected are covered, and how many deep covered;
//! - the level tables list the level's requirements (not tag items)
//!   with ID, title, status, coverage and location. Coverage is shown
//!   with a sign and words besides the color: `✓` deep covered, `!`
//!   covered but with a gap further down, `✗` not covered, `–` rejected,
//!   `?` unknown type; a legend explains them. Copies of a duplicate ID
//!   are marked.
//!
//! The levels are those of `shall.toml`, in their order there; types of
//! requirement items without a level follow in alphabetical order, named
//! by their type. If such a type is unknown, its items are errors (see
//! [`TracedItem::unknown_type`](shall_core::trace::TracedItem::unknown_type))
//! and their level shows no coverage; missing types are still listed.
//! Percentages are rounded down, so that 100 % means complete.

use std::collections::BTreeSet;

use minijinja::Environment;
use minijinja::syntax::SyntaxConfig;
use minijinja::value::{Serde, Value};
use serde::Serialize;
use shall_core::export::{Document, ItemEntry};
use shall_core::item::Origin;
use shall_core::results::Verdict;
use shall_core::{ItemType, Severity, Status};

const INDEX: &str = include_str!("../templates/index.html");
const STYLE: &str = include_str!("../templates/style.css");

/// Renders the start page, `index.html`.
///
/// Fails only if a template is broken, since the document's text is
/// passed to the templates as values, never as template source.
pub fn render(document: &Document<'_>) -> Result<String, minijinja::Error> {
    let mut env = Environment::new();
    env.set_syntax(
        SyntaxConfig::builder()
            .trim_blocks(true)
            .lstrip_blocks(true)
            .keep_trailing_newline(true)
            .build()
            .expect("valid syntax"),
    );
    env.add_template("index.html", INDEX)?;
    env.add_template("style.css", STYLE)?;
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    let page = Page::new(document);
    env.get_template("index.html")?
        .render(Value::from(Serde(&page)))
}

/// Everything the template shows, computed here so that the template
/// stays simple.
#[derive(Debug, Serialize)]
struct Page<'a> {
    generator: String,
    summary: Summary,
    levels: Vec<LevelView<'a>>,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    requirements: usize,
    tags: usize,
    tests: Option<Tests>,
    errors: usize,
    warnings: usize,
}

/// Verdicts of the verification tags.
#[derive(Debug, Default, Serialize)]
struct Tests {
    passed: usize,
    failed: usize,
    skipped: usize,
    not_executed: usize,
    no_test: usize,
}

#[derive(Debug, Serialize)]
struct LevelView<'a> {
    typ: &'a str,
    name: &'a str,
    /// Whether `shall.toml` knows the type, at least from `Needs:` of
    /// items; items of an unknown type are errors, and their coverage is
    /// not shown.
    known: bool,
    needs: Vec<&'a str>,
    /// Items that are not rejected.
    counted: usize,
    covered: usize,
    deep_covered: usize,
    covered_percent: Option<u64>,
    deep_percent: Option<u64>,
    items: Vec<ItemView<'a>>,
}

#[derive(Debug, Serialize)]
struct ItemView<'a> {
    id: String,
    title: &'a str,
    status: &'static str,
    /// `ok`, `partial`, `missing`, `rejected` or `unknown`, for the CSS
    /// class.
    coverage: &'static str,
    /// The sign of the coverage.
    sign: &'static str,
    /// The coverage in words, for the legend and screen readers.
    label: &'static str,
    duplicate: bool,
    missing: Vec<&'a str>,
    location: String,
}

impl<'a> Page<'a> {
    fn new(document: &'a Document<'_>) -> Self {
        let mut summary = Summary::default();
        let mut tags = BTreeSet::new();
        let mut tests = Tests::default();
        for item in &document.items {
            // A tag with several targets has one item per target, but it
            // is one tag with one test.
            let new_tag = match item.origin {
                Origin::Requirement => {
                    summary.requirements += 1;
                    false
                }
                Origin::Tag { text, .. } => tags.insert((item.location, text)),
            };
            if let Some(test) = item.test.as_ref().filter(|_| new_tag) {
                match test.verdict {
                    Verdict::Passed => tests.passed += 1,
                    Verdict::Failed => tests.failed += 1,
                    Verdict::Skipped => tests.skipped += 1,
                    Verdict::NotExecuted => tests.not_executed += 1,
                    Verdict::NoTest => tests.no_test += 1,
                }
            }
        }
        summary.tags = tags.len();
        summary.tests = document.test_results.is_some().then_some(tests);
        for defect in &document.defects {
            match defect.severity {
                Severity::Error => summary.errors += 1,
                Severity::Warning => summary.warnings += 1,
            }
        }

        // Requirement types without a level, e.g. misspelled ones, get a
        // table of their own after the levels.
        let configured: BTreeSet<&ItemType> =
            document.levels.iter().map(|level| level.typ).collect();
        let others: BTreeSet<&ItemType> = document
            .items
            .iter()
            .filter(|item| matches!(item.origin, Origin::Requirement))
            .map(|item| item.typ)
            .filter(|typ| !configured.contains(typ))
            .collect();
        let levels = document
            .levels
            .iter()
            .map(|entry| level(document, entry.typ, entry.name, entry.needs))
            .chain(
                others
                    .into_iter()
                    .map(|typ| level(document, typ, typ.as_str(), &[])),
            )
            .collect();
        Self {
            generator: format!("{} {}", document.generator.name, document.generator.version),
            summary,
            levels,
        }
    }
}

fn level<'a>(
    document: &'a Document<'_>,
    typ: &'a ItemType,
    name: &'a str,
    needs: &'a [ItemType],
) -> LevelView<'a> {
    // Tag items are not listed: a tag with several targets would appear
    // once per target, and the tables are about requirements.
    let items: Vec<&ItemEntry> = document
        .items
        .iter()
        .filter(|item| item.typ == typ && matches!(item.origin, Origin::Requirement))
        .collect();
    let counted: Vec<&&ItemEntry> = items
        .iter()
        .filter(|item| item.status != Status::Rejected)
        .collect();
    let covered = counted.iter().filter(|item| item.covered).count();
    let deep_covered = counted.iter().filter(|item| item.deep_covered).count();
    // The type is unknown for all items of a level or none, except
    // rejected ones, which are not counted.
    let known = !counted.iter().any(|item| item.unknown_type);
    LevelView {
        typ: typ.as_str(),
        name,
        known,
        needs: needs.iter().map(ItemType::as_str).collect(),
        counted: counted.len(),
        covered,
        deep_covered,
        covered_percent: percent(covered, counted.len()),
        deep_percent: percent(deep_covered, counted.len()),
        items: items.into_iter().map(item_view).collect(),
    }
}

/// `part` of `total` in percent, rounded down; none for an empty level.
fn percent(part: usize, total: usize) -> Option<u64> {
    (total > 0).then(|| (part as u64 * 100) / total as u64)
}

fn item_view<'a>(item: &'a ItemEntry<'_>) -> ItemView<'a> {
    let (coverage, sign, label) = if item.status == Status::Rejected {
        ("rejected", "–", "rejected")
    } else if item.unknown_type {
        ("unknown", "?", "unknown type")
    } else if item.deep_covered {
        ("ok", "✓", "deep covered")
    } else if item.covered {
        ("partial", "!", "covered, gap below")
    } else {
        ("missing", "✗", "not covered")
    };
    ItemView {
        id: item.id.to_string(),
        title: item.title,
        label,
        duplicate: item.duplicate,
        status: item.status.as_str(),
        coverage,
        sign,
        missing: item.missing.iter().map(ItemType::as_str).collect(),
        location: item.location.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use shall_core::config::Config;
    use shall_core::export::Input;
    use shall_core::{markdown, trace};

    use super::*;

    fn html(config: &str, requirements: &str) -> String {
        let config = Config::parse(config).unwrap();
        let items = markdown::parse(Path::new("reqs/a.md"), requirements)
            .unwrap()
            .items;
        let trace = trace::trace(&items, &config);
        render(&Document::new(Input {
            config: &config,
            items: &items,
            trace: &trace,
            tests: None,
            diagnostics: &trace.defects,
        }))
        .unwrap()
    }

    #[test]
    fn unknown_types_show_no_coverage_and_duplicates_are_marked() {
        let html = html(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\n",
            "# A\n\n`dns~a-k4f2~1`\n\n# B\n\n`dsn~b-k4f2~1`\n\n# B\n\n`dsn~b-k4f2~1`\n",
        );
        assert!(
            html.contains("<td colspan=\"2\" class=\"unknown-type\">? unknown type</td>"),
            "{html}"
        );
        assert!(
            html.contains(
                "<tr class=\"unknown\">\n<td class=\"sign\">\
                 <span title=\"unknown type\" aria-hidden=\"true\">?</span>\
                 <span class=\"sr-only\">unknown type</span>"
            ),
            "{html}"
        );
        assert_eq!(html.matches("✗ duplicate ID").count(), 2, "{html}");
    }

    #[test]
    fn types_from_needs_and_rejected_items_are_not_unknown() {
        let html = html(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\n",
            "# A\n\n`dsn~a-k4f2~1`\n\nNeeds: itst\n\n# T\n\n`itst~t-k4f2~1`\n\n\
             # R\n\n`zzz~r-k4f2~1`\n\nStatus: rejected\n",
        );
        assert!(!html.contains("unknown-type\">"), "{html}");
        assert!(!html.contains("<tr class=\"unknown\">"), "{html}");
        assert!(html.contains("<tr class=\"rejected\">"), "{html}");
        assert!(html.contains("<h2>itst</h2>"), "{html}");
    }

    #[test]
    fn level_tables_list_requirements_only() {
        let html = html(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\nneeds = [\"impl\"]\n",
            "# A\n\n`dsn~a-k4f2~1`\n\n# I\n\n`impl~i-k4f2~1`\n\nCovers:\n- dsn~a-k4f2~1\n",
        );
        let impl_table = &html[html.find("<h2>impl</h2>").unwrap()..];
        assert_eq!(impl_table.matches("<tr class=").count(), 1, "{html}");
    }

    #[test]
    fn a_tag_with_several_targets_counts_its_test_once() {
        use shall_core::results::{self, Packages};
        use shall_core::{junit, tags};

        let config = Config::parse(
            "[[level]]\ntype = \"dsn\"\nname = \"Design\"\nneeds = [\"utest\"]\n\n\
             [verification]\ntypes = [\"utest\"]\n",
        )
        .unwrap();
        let mut items = markdown::parse(
            Path::new("reqs/a.md"),
            "# A\n\n`dsn~a-k4f2~1`\n\n# B\n\n`dsn~b-k4f2~1`\n",
        )
        .unwrap()
        .items;
        items.extend(
            tags::scan(
                Path::new("tests/a.cpp"),
                "// [utest->dsn~a-k4f2~1, dsn~b-k4f2~1]\nTEST(S, T) {}\n",
            )
            .unwrap()
            .items,
        );
        let trace = trace::trace(&items, &config);
        let parsed = junit::parse(
            Path::new("r.xml"),
            r#"<testsuite name="S"><testcase classname="S" name="T"/></testsuite>"#,
        )
        .unwrap();
        let evaluation = results::evaluate(&items, &parsed.results, &config, &Packages::default());
        let html = render(&Document::new(Input {
            config: &config,
            items: &items,
            trace: &trace,
            tests: Some((&parsed.results, &evaluation)),
            diagnostics: &[],
        }))
        .unwrap();
        assert!(html.contains("<dt>Tags</dt><dd>1</dd>"), "{html}");
        assert!(html.contains("<dt>✓ Passed</dt><dd>1</dd>"), "{html}");
    }

    #[test]
    fn types_without_a_level_get_a_table_after_the_levels() {
        let html = html(
            "[[level]]\ntype = \"swr\"\nname = \"Software\"\n",
            "# A\n\n`swr~a-k4f2~1`\n\n# B\n\n`sfr~b-k4f2~1`\n",
        );
        let software = html.find("<h2>Software <code>swr</code></h2>").unwrap();
        let other = html.find("<h2>sfr</h2>").unwrap();
        assert!(software < other, "{html}");
        assert!(html.contains("<code>sfr~b-k4f2~1</code>"), "{html}");
    }

    #[test]
    fn without_levels_every_requirement_type_is_a_level() {
        let html = html(
            "",
            "# <b>Bold</b> & co\n\n`swr~b-k4f2~1`\n\n# A\n\n`dsn~a-k4f2~1`\n\n\
             # Old\n\n`dsn~old-k4f2~1`\n\nStatus: rejected\n",
        );
        insta::assert_snapshot!(html);
    }

    #[test]
    fn percentages_round_down() {
        assert_eq!(percent(0, 0), None);
        assert_eq!(percent(0, 3), Some(0));
        assert_eq!(percent(2, 3), Some(66));
        assert_eq!(percent(199, 200), Some(99));
        assert_eq!(percent(3, 3), Some(100));
    }
}
