//! Export of the traced project as `trace.json`, the only data source of
//! the report and of other evaluations.
//!
//! The document is deterministic: the same input gives byte-identical
//! output, without timestamps or absolute paths of the machine, so that
//! it can be compared and versioned. Paths are relative to the project
//! root and separated by `/`. Its layout:
//!
//! - `schema_version`: [`SCHEMA_VERSION`], increased on every change that
//!   readers have to know about from the first release on; until then
//!   the layout grows without a new version;
//! - `generator`: name and version of the tool that wrote the file;
//! - `variant`: name and features of the traced variant, or `null` when
//!   all items are traced;
//! - `levels` and `verification_types`: from `shall.toml`;
//! - `items`: every item ordered by location, with its fields, its
//!   coverage from [`trace`](crate::trace), whether it is
//!   [verified](crate::verify) and, for verification tags, its test and
//!   verdict from [`results`](crate::results);
//! - `links`: every `Covers:`, `Depends:` and `Satisfied-by:` link of
//!   items that are not rejected, with its status;
//! - `test_results`: every test case read from the JUnit files, also those
//!   without a tag, or `null` if `[sources]` has no `junit` patterns;
//! - `defects`: every diagnostic, as `shall check` prints them.

use serde::Serialize;

use crate::config::{Config, Variant};
use crate::diagnostic::{Diagnostic, Severity};
use crate::id::{ItemId, ItemType};
use crate::item::{Item, Location, Origin, Param, Status};
use crate::junit::{Outcome, TestResult};
use crate::results::{Evaluation, Verdict};
use crate::trace::{ItemRef, Link, Trace};
use crate::verify::{self, Joined};

/// Version of the layout of `trace.json`.
pub const SCHEMA_VERSION: u32 = 1;

/// Name of the tool in `generator`.
const GENERATOR: &str = "shall";

/// The content of `trace.json`; serialize it with [`Document::to_json`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Document<'a> {
    /// [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The tool that wrote the document.
    pub generator: Generator,
    /// The traced variant; none when all items are traced.
    pub variant: Option<VariantEntry<'a>>,
    /// The levels of `shall.toml`, in their order there.
    pub levels: Vec<LevelEntry<'a>>,
    /// Types whose tags are tests, from `[verification]`.
    pub verification_types: &'a [ItemType],
    /// Every item, ordered by location.
    pub items: Vec<ItemEntry<'a>>,
    /// Every link of items that are not rejected, ordered by location.
    pub links: &'a [Link],
    /// Every test case of the JUnit files, in the order they were read;
    /// none without `junit` sources.
    pub test_results: Option<Vec<ResultEntry<'a>>>,
    /// Every diagnostic, in the order given.
    pub defects: Vec<DefectEntry<'a>>,
}

/// Name and version of the tool that wrote a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Generator {
    /// Always `shall`.
    pub name: &'static str,
    /// Version of `shall-core`.
    pub version: &'static str,
}

/// A variant from `shall.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VariantEntry<'a> {
    /// Name of the variant, from `[variant.<name>]`.
    pub name: &'a str,
    /// The selected features.
    pub features: &'a [String],
}

/// A level of `shall.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LevelEntry<'a> {
    /// Type of the level's items.
    #[serde(rename = "type")]
    pub typ: &'a ItemType,
    /// Readable name.
    pub name: &'a str,
    /// Types the level needs.
    pub needs: &'a [ItemType],
}

/// An item with its coverage and test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemEntry<'a> {
    /// The ID, e.g. `swr~motor.overtemp-k4f2~2`.
    pub id: &'a ItemId,
    /// Type of the ID.
    #[serde(rename = "type")]
    pub typ: &'a ItemType,
    /// Name of the ID.
    pub name: &'a str,
    /// Revision of the ID.
    pub revision: u32,
    /// Title; empty for tag items.
    pub title: &'a str,
    /// Text between the ID and the first keyword, as Markdown.
    pub description: &'a str,
    /// `Rationale:`.
    pub rationale: Option<&'a str>,
    /// `Comment:`.
    pub comment: Option<&'a str>,
    /// `Status:`.
    pub status: Status,
    /// `Tags:`.
    pub tags: &'a [String],
    /// `Source:`.
    pub source: Option<&'a str>,
    /// `Params:`: parameter values for the item's `Satisfied-by:` links.
    pub params: &'a [Param],
    /// `Features:`.
    pub features: &'a [String],
    /// Where the item is defined.
    pub location: &'a Location,
    /// Requirement file or tag, with the tag's text and test definition.
    pub origin: &'a Origin,
    /// Types the item needs, from `Needs:` or its level; empty for
    /// rejected items.
    pub needs: &'a [ItemType],
    /// Items that cover this item with a valid link.
    pub covered_by: &'a [ItemRef],
    /// Needed types without a covering item.
    pub missing: &'a [ItemType],
    /// Whether every needed type is covered.
    pub covered: bool,
    /// Whether the item and everything below it is covered.
    pub deep_covered: bool,
    /// Whether another item has the same type and name.
    pub duplicate: bool,
    /// Whether the item's type is unknown to `shall.toml`, which is an
    /// error; its coverage then means nothing.
    pub unknown_type: bool,
    /// Whether everything below the item is implemented and successfully
    /// validated, see [`verify`](mod@crate::verify).
    pub verified: bool,
    /// The test of a verification tag; none for other items and without
    /// `junit` sources.
    pub test: Option<TestEntry<'a>>,
}

/// The test of a verification tag and its verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestEntry<'a> {
    /// The test as runners name it; none without a test definition.
    pub name: Option<&'a str>,
    /// The combined outcome of its results.
    pub verdict: Verdict,
    /// Its results, as ascending indices into `test_results`.
    pub results: &'a [usize],
}

/// A test case read from a JUnit file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResultEntry<'a> {
    /// The `classname` attribute, or the suite's name without one.
    pub classname: &'a str,
    /// The `name` attribute.
    pub name: &'a str,
    /// The name of the enclosing `<testsuite>`.
    pub suite: &'a str,
    /// `passed`, `failed` with a `message` or `skipped`.
    #[serde(flatten)]
    pub outcome: &'a Outcome,
    /// Where the `<testcase>` element is.
    pub location: &'a Location,
}

/// A diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DefectEntry<'a> {
    /// `error` or `warning`.
    pub severity: Severity,
    /// The message, without location and severity.
    pub message: &'a str,
    /// Where the problem is.
    pub location: &'a Location,
}

/// The parts of a traced project that make up a document.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// The configuration.
    pub config: &'a Config,
    /// Every item: the slice passed to [`trace`](crate::trace::trace) and
    /// [`evaluate`](crate::results::evaluate), in the same order.
    pub items: &'a [Item],
    /// The result of [`trace`](crate::trace::trace) for `items`.
    pub trace: &'a Trace,
    /// The test results and the result of
    /// [`evaluate`](crate::results::evaluate) for `items` and them; none
    /// without `junit` sources.
    pub tests: Option<(&'a [TestResult], &'a Evaluation)>,
    /// Every diagnostic, usually sorted by location.
    pub diagnostics: &'a [Diagnostic],
    /// Name and selection of the traced variant, whose items `items` are;
    /// none when all items are traced.
    pub variant: Option<(&'a str, &'a Variant)>,
}

impl<'a> Document<'a> {
    /// Builds the document.
    ///
    /// # Panics
    ///
    /// If `trace` was not computed for `items`, or the items are in
    /// another order.
    pub fn new(input: Input<'a>) -> Self {
        let joined = verify::join(input.items, input.trace, input.tests.map(|(_, e)| e));
        let verified = verify::verified(&joined, input.config);
        let items = joined
            .iter()
            .zip(verified)
            .map(|(joined, verified)| entry(joined, verified))
            .collect();
        Self {
            schema_version: SCHEMA_VERSION,
            generator: Generator {
                name: GENERATOR,
                version: env!("CARGO_PKG_VERSION"),
            },
            variant: input.variant.map(|(name, variant)| VariantEntry {
                name,
                features: &variant.features,
            }),
            levels: input
                .config
                .levels
                .iter()
                .map(|level| LevelEntry {
                    typ: &level.typ,
                    name: &level.name,
                    needs: &level.needs,
                })
                .collect(),
            verification_types: &input.config.verification.types,
            items,
            links: &input.trace.links,
            test_results: input.tests.map(|(results, _)| {
                results
                    .iter()
                    .map(|result| ResultEntry {
                        classname: &result.classname,
                        name: &result.name,
                        suite: &result.suite,
                        outcome: &result.outcome,
                        location: &result.location,
                    })
                    .collect()
            }),
            defects: input
                .diagnostics
                .iter()
                .map(|diagnostic| DefectEntry {
                    severity: diagnostic.severity,
                    message: &diagnostic.message,
                    location: &diagnostic.location,
                })
                .collect(),
        }
    }

    /// Serializes the document as indented JSON with a final newline.
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self).expect("document serializes");
        json.push('\n');
        json
    }
}

fn entry<'a>(joined: &Joined<'a>, verified: bool) -> ItemEntry<'a> {
    let Joined { item, traced, run } = *joined;
    ItemEntry {
        id: &item.id,
        typ: item.id.typ(),
        name: item.id.name(),
        revision: item.id.revision(),
        title: &item.title,
        description: &item.description,
        rationale: item.rationale.as_deref(),
        comment: item.comment.as_deref(),
        status: item.status,
        tags: &item.tags,
        source: item.source.as_deref(),
        params: &item.params,
        features: &item.features,
        location: &item.location,
        origin: &item.origin,
        needs: &traced.needs,
        covered_by: &traced.covered_by,
        missing: &traced.missing,
        covered: traced.covered,
        deep_covered: traced.deep_covered,
        duplicate: traced.duplicate,
        unknown_type: traced.unknown_type,
        verified,
        test: run.map(|run| TestEntry {
            name: run.test.as_deref(),
            verdict: run.verdict,
            results: &run.results,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::results::Packages;
    use crate::{junit, markdown, results, tags, trace};

    const CONFIG: &str = r#"
[[level]]
type = "dsn"
name = "Design"
needs = ["utest"]

[verification]
types = ["utest"]
"#;

    const REQUIREMENTS: &str = "\
# A

`dsn~a-k4f2~1`

# B

`dsn~b-k4f2~1`
";

    const TESTS: &str = "\
// [utest->dsn~a-k4f2~1, dsn~b-k4f2~1]
TEST(Suite, Passes) {}

// [utest->dsn~b-k4f2~1]
TEST(Suite, Fails) {}

// [utest->dsn~b-k4f2~1]
int helper();
";

    // On one line, as many generators write it: all results have the
    // same location.
    const JUNIT: &str = r#"<testsuites><testsuite name="Suite"><testcase classname="Other" name="Untagged"/><testcase classname="Suite" name="Fails"><failure message="boom"/></testcase><testcase classname="Suite" name="Passes"/></testsuite></testsuites>"#;

    fn json(with_tests: bool) -> serde_json::Value {
        let config = Config::parse(CONFIG).unwrap();
        // Tags first, so that the document has to order the items.
        let mut items = tags::scan(Path::new("tests/a.cpp"), TESTS).unwrap().items;
        items.extend(
            markdown::parse(Path::new("reqs/a.md"), REQUIREMENTS)
                .unwrap()
                .items,
        );
        let trace = trace::trace(&items, &config);
        let parsed = junit::parse(Path::new("results.xml"), JUNIT).unwrap();
        let evaluation = results::evaluate(&items, &parsed.results, &config, &Packages::default());
        let mut diagnostics = trace.defects.clone();
        diagnostics.extend(evaluation.defects.iter().cloned());
        let document = Document::new(Input {
            config: &config,
            items: &items,
            trace: &trace,
            tests: with_tests.then_some((parsed.results.as_slice(), &evaluation)),
            diagnostics: &diagnostics,
            variant: None,
        });
        let text = document.to_json();
        assert!(text.ends_with("}\n"), "{text}");
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn orders_items_and_joins_coverage_and_tests() {
        let json = json(true);
        assert_eq!(json["schema_version"], SCHEMA_VERSION);
        assert_eq!(json["levels"][0]["type"], "dsn");
        assert_eq!(json["verification_types"], serde_json::json!(["utest"]));
        let items = json["items"].as_array().unwrap();
        let summary: Vec<String> = items
            .iter()
            .map(|item| {
                format!(
                    "{}:{} {} covered={} test={}",
                    item["location"]["path"].as_str().unwrap(),
                    item["location"]["line"],
                    item["type"].as_str().unwrap(),
                    item["covered"],
                    item["test"]["name"].as_str().unwrap_or("-"),
                ) + &item["test"]["verdict"]
                    .as_str()
                    .map(|verdict| format!(" {verdict}"))
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(
            summary,
            [
                "reqs/a.md:3 dsn covered=true test=-",
                "reqs/a.md:7 dsn covered=true test=-",
                // One tag with two targets: two items at the same location,
                // each with the tag's test.
                "tests/a.cpp:1 utest covered=true test=Suite.Passes passed",
                "tests/a.cpp:1 utest covered=true test=Suite.Passes passed",
                "tests/a.cpp:4 utest covered=true test=Suite.Fails failed",
                "tests/a.cpp:7 utest covered=true test=- no_test",
            ]
        );
        assert_eq!(items[2]["origin"]["kind"], "tag");
        assert_eq!(items[2]["origin"]["test"]["suite"], "Suite");
        assert_eq!(items[1]["covered_by"].as_array().unwrap().len(), 3);
        // dsn~b is not verified: one of its tests failed.
        assert_eq!(items[0]["verified"], true);
        assert_eq!(items[1]["verified"], false);
        assert_eq!(items[2]["verified"], true);
        // Results by index, since they all are on line 1.
        assert_eq!(items[2]["test"]["results"], serde_json::json!([2]));
        assert_eq!(items[4]["test"]["results"], serde_json::json!([1]));
        assert_eq!(items[5]["test"]["name"], serde_json::Value::Null);
        assert_eq!(items[5]["test"]["results"], serde_json::json!([]));

        // Untagged results are exported too.
        let results = json["test_results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["classname"], "Other");
        assert_eq!(results[1]["outcome"], "failed");
        assert_eq!(results[1]["message"], "boom");
        assert_eq!(results[2]["outcome"], "passed");
        assert!(results[2].get("message").is_none());

        assert_eq!(json["links"].as_array().unwrap().len(), 4);
        assert_eq!(
            json["links"][0],
            serde_json::json!({
                "kind": "covers",
                "source": items[2]["id"],
                "source_location": {"path": "tests/a.cpp", "line": 1},
                "target": "dsn~a-k4f2~1",
                "target_location": {"path": "reqs/a.md", "line": 3},
                "location": {"path": "tests/a.cpp", "line": 1},
                "status": "valid",
            })
        );
        assert_eq!(
            json["defects"],
            serde_json::json!([
                {
                    "severity": "error",
                    "message": "test Suite.Fails failed",
                    "location": {"path": "tests/a.cpp", "line": 4},
                },
                {
                    "severity": "warning",
                    "message": "tag [utest->dsn~b-k4f2~1]: no test definition follows the tag",
                    "location": {"path": "tests/a.cpp", "line": 7},
                },
            ])
        );
    }

    #[test]
    fn links_tell_copies_of_a_duplicate_id_apart() {
        let config = Config::parse(
            "[[level]]\ntype = \"swr\"\nname = \"S\"\nneeds = [\"dsn\"]\n\n\
             [[level]]\ntype = \"dsn\"\nname = \"D\"\n",
        )
        .unwrap();
        let text = "\
# S

`swr~s-k4f2~1`

# C

`dsn~c-k4f2~1`

# C again

`dsn~c-k4f2~1`

Covers:
- swr~s-k4f2~1
- dsn~c-k4f2~1
";
        let items = markdown::parse(Path::new("reqs/a.md"), text).unwrap().items;
        let trace = trace::trace(&items, &config);
        let document = Document::new(Input {
            config: &config,
            items: &items,
            trace: &trace,
            tests: None,
            diagnostics: &trace.defects,
            variant: None,
        });
        let json: serde_json::Value = serde_json::from_str(&document.to_json()).unwrap();
        let link = |n: usize| {
            let link = &json["links"][n];
            format!(
                "{} {} -> {} {} {}",
                link["source"].as_str().unwrap(),
                link["source_location"]["line"],
                link["target"].as_str().unwrap(),
                link["target_location"]["line"],
                link["status"].as_str().unwrap(),
            )
        };
        assert_eq!(link(0), "dsn~c-k4f2~1 11 -> swr~s-k4f2~1 3 valid");
        assert_eq!(link(1), "dsn~c-k4f2~1 11 -> dsn~c-k4f2~1 null ambiguous");
    }

    #[test]
    fn without_junit_sources_has_no_tests() {
        let json = json(false);
        assert_eq!(json["test_results"], serde_json::Value::Null);
        assert!(
            json["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["test"].is_null() && item["verified"] == false)
        );
    }
}
