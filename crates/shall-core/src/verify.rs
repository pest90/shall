//! The verified status: an item is verified when everything below it is
//! implemented and successfully validated.
//!
//! An item is *verified* when
//!
//! - it is deep covered (see [`trace`](crate::trace)), not rejected, not
//!   a copy of a duplicate ID and not of an unknown type, since those are
//!   errors themselves;
//! - its own evidence holds, if its type is a verification type of
//!   `[verification]`: the test of a verification tag passed (see
//!   [`results`](crate::results)); a verification item written in a
//!   requirement file, a review or an analysis such as `vrev` or `vana`,
//!   is approved, which is also the default without `Status:`, as in
//!   OpenFastTrace;
//! - every item covering it with a valid link is verified.
//!
//! Without `junit` sources no test has passed, so no item that needs a
//! test is verified. The status of items of other types does not count:
//! a draft requirement can be verified; which statuses are mature enough
//! is up to the check modes. A review that is not approved is no defect,
//! like a requirement in draft; it only keeps the items above it from
//! being verified.

use std::collections::{HashMap, VecDeque};

use crate::config::Config;
use crate::id::ItemId;
use crate::item::{Item, Location, Origin, Status};
use crate::results::{Evaluation, TestRun, Verdict};
use crate::trace::{Trace, TracedItem};

/// Computes for every item of `trace.items`, in that order, whether it is
/// verified.
///
/// `items` is the slice passed to [`trace`](crate::trace::trace) and
/// [`evaluate`](crate::results::evaluate), in the same order;
/// `evaluation` is none without `junit` sources.
///
/// # Panics
///
/// If `trace` was not computed for `items`, or the items are in another
/// order.
pub fn verify(
    items: &[Item],
    trace: &Trace,
    config: &Config,
    evaluation: Option<&Evaluation>,
) -> Vec<bool> {
    verified(&join(items, trace, evaluation), config)
}

/// An item with its coverage and the run of its test.
#[derive(Clone, Copy)]
pub(crate) struct Joined<'a> {
    pub item: &'a Item,
    pub traced: &'a TracedItem,
    /// The run of a verification tag; none for other items and without
    /// `junit` sources.
    pub run: Option<&'a TestRun>,
}

/// Joins `items` with their trace and test runs, in the order of
/// `trace.items`.
///
/// # Panics
///
/// If `trace` was not computed for `items`, or the items are in another
/// order.
pub(crate) fn join<'a>(
    items: &'a [Item],
    trace: &'a Trace,
    evaluation: Option<&'a Evaluation>,
) -> Vec<Joined<'a>> {
    // `trace` orders its items by location, keeping the order of items at
    // the same location, e.g. those of one tag with several targets.
    let mut items: Vec<&Item> = items.iter().collect();
    items.sort_by(|a, b| a.location.cmp(&b.location));
    assert_eq!(
        items.len(),
        trace.items.len(),
        "trace computed for other items"
    );
    let mut runs = evaluation.map(runs_by_item).unwrap_or_default();
    items
        .into_iter()
        .zip(&trace.items)
        .map(|(item, traced)| {
            assert_eq!(item.id, traced.id, "trace computed for other items");
            let run = runs
                .get_mut(&(&item.location, &item.id))
                .and_then(VecDeque::pop_front);
            Joined { item, traced, run }
        })
        .collect()
}

/// The runs of `evaluation` by the item they belong to. Runs follow the
/// order of the items, one per verification tag; items with the same
/// location and ID take their runs in that order, which sorting by
/// location keeps.
fn runs_by_item(evaluation: &Evaluation) -> HashMap<(&Location, &ItemId), VecDeque<&TestRun>> {
    let mut runs = HashMap::new();
    for run in &evaluation.runs {
        runs.entry((&run.location, &run.item))
            .or_insert_with(VecDeque::new)
            .push_back(run);
    }
    runs
}

/// The verified status of joined items, in their order.
pub(crate) fn verified(joined: &[Joined<'_>], config: &Config) -> Vec<bool> {
    let mut position: HashMap<(&Location, &ItemId), usize> = HashMap::new();
    for (index, entry) in joined.iter().enumerate() {
        position
            .entry((&entry.item.location, &entry.item.id))
            .or_insert(index);
    }
    let sources: Vec<Vec<usize>> = joined
        .iter()
        .map(|entry| {
            entry
                .traced
                .covered_by
                .iter()
                .map(|source| position[&(&source.location, &source.id)])
                .collect()
        })
        .collect();

    // Depth first without recursion, so that long chains cannot overflow
    // the stack. Only deep covered items are descended into, and the
    // items covering them are deep covered and not part of a cycle, so
    // the search ends.
    let mut verified: Vec<Option<bool>> = vec![None; joined.len()];
    for root in 0..joined.len() {
        let mut stack = vec![root];
        while let Some(&node) = stack.last() {
            if verified[node].is_some() {
                stack.pop();
                continue;
            }
            if !own_evidence(&joined[node], config) {
                verified[node] = Some(false);
                stack.pop();
                continue;
            }
            let pending: Vec<usize> = sources[node]
                .iter()
                .copied()
                .filter(|&source| verified[source].is_none())
                .collect();
            if pending.is_empty() {
                verified[node] = Some(sources[node].iter().all(|&s| verified[s] == Some(true)));
                stack.pop();
            } else {
                stack.extend(pending);
            }
        }
    }
    verified
        .into_iter()
        .map(|verified| verified.expect("every item is visited"))
        .collect()
}

/// Whether the item may be verified by itself: deep covered, no error,
/// and its own evidence holds.
fn own_evidence(entry: &Joined<'_>, config: &Config) -> bool {
    let traced = entry.traced;
    if !traced.deep_covered || traced.rejected || traced.duplicate || traced.unknown_type {
        return false;
    }
    if !config.verification.types.contains(entry.item.id.typ()) {
        return true;
    }
    match entry.item.origin {
        Origin::Tag { .. } => entry.run.is_some_and(|run| run.verdict == Verdict::Passed),
        Origin::Requirement => entry.item.status == Status::Approved,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::junit;
    use crate::results::{self, Packages};
    use crate::{markdown, tags, trace};

    const CONFIG: &str = r#"
[[level]]
type = "swr"
name = "Software"
needs = ["dsn"]

[[level]]
type = "dsn"
name = "Design"
needs = ["impl", "utest"]

[verification]
types = ["utest", "vrev"]
"#;

    const CODE: &str = "\
// [impl->dsn~a-k4f2~1, dsn~b-k4f2~1, dsn~c-k4f2~1]
void f();
";

    const TESTS: &str = "\
// [utest->dsn~a-k4f2~1, dsn~c-k4f2~1]
TEST(Suite, Passes) {}

// [utest->dsn~b-k4f2~1]
TEST(Suite, Fails) {}
";

    const JUNIT: &str = r#"<testsuite name="Suite">
<testcase classname="Suite" name="Passes"/>
<testcase classname="Suite" name="Fails"><failure message="boom"/></testcase>
</testsuite>
"#;

    /// `verify` for the requirements plus the tags above, as
    /// `ID verified` lines in the order of the trace.
    fn run(requirements: &str, with_junit: bool) -> Vec<String> {
        let config = Config::parse(CONFIG).unwrap();
        let mut items = markdown::parse(Path::new("reqs/a.md"), requirements)
            .unwrap()
            .items;
        items.extend(tags::scan(Path::new("src/a.c"), CODE).unwrap().items);
        items.extend(tags::scan(Path::new("tests/a.cpp"), TESTS).unwrap().items);
        let trace = trace::trace(&items, &config);
        let results = junit::parse(Path::new("results.xml"), JUNIT)
            .unwrap()
            .results;
        let evaluation = results::evaluate(&items, &results, &config, &Packages::default());
        let verified = verify(&items, &trace, &config, with_junit.then_some(&evaluation));
        trace
            .items
            .iter()
            .zip(verified)
            .filter(|(item, _)| item.location.path() == "reqs/a.md")
            .map(|(item, verified)| format!("{} {verified}", item.id))
            .collect()
    }

    /// Requirements with a review of `dsn~c` in `status`, plus `extra`.
    fn design(status: &str, extra: &str) -> String {
        format!(
            "\
# A

`dsn~a-k4f2~1`

Status: draft

# B

`dsn~b-k4f2~1`

# C

`dsn~c-k4f2~1`

Covers:
- swr~s-k4f2~1

Needs: impl, utest, vrev

# S

`swr~s-k4f2~1`

# R

`vrev~r-k4f2~1`

Status: {status}

Covers:
- dsn~c-k4f2~1
{extra}"
        )
    }

    #[test]
    fn verified_needs_passed_tests_and_approved_reviews_below() {
        assert_eq!(
            run(&design("approved", ""), true),
            [
                // A draft requirement can be verified.
                "dsn~a-k4f2~1 true",
                // Its test failed.
                "dsn~b-k4f2~1 false",
                "dsn~c-k4f2~1 true",
                "swr~s-k4f2~1 true",
                "vrev~r-k4f2~1 true",
            ]
        );
    }

    #[test]
    fn a_review_that_is_not_approved_verifies_nothing() {
        for status in ["draft", "proposed"] {
            let lines = run(&design(status, ""), true);
            assert_eq!(lines[2], "dsn~c-k4f2~1 false");
            assert_eq!(lines[3], "swr~s-k4f2~1 false");
            assert_eq!(lines[4], "vrev~r-k4f2~1 false");
        }
        // It still covers its target.
        let config = Config::parse(CONFIG).unwrap();
        let items = markdown::parse(Path::new("reqs/a.md"), &design("draft", ""))
            .unwrap()
            .items;
        let trace = trace::trace(&items, &config);
        let review = trace
            .items
            .iter()
            .find(|item| item.id.typ().as_str() == "vrev");
        let target = trace.items.iter().find(|item| item.id.name() == "c-k4f2");
        assert_eq!(
            target.unwrap().covered_by[0].id,
            review.unwrap().id,
            "{trace:?}"
        );
    }

    #[test]
    fn a_review_without_status_is_approved() {
        let text = design("approved", "").replace("Status: approved\n\n", "");
        assert!(!text.contains("Status: approved"));
        assert_eq!(run(&text, true)[4], "vrev~r-k4f2~1 true");
    }

    /// `verify` without levels, as `ID verified` lines of the items of
    /// `reqs/a.md` in the order of the trace.
    fn run_without_levels(requirements: &str, tests: &str, junit: &str) -> Vec<String> {
        let config = Config::parse("[verification]\ntypes = [\"utest\", \"vrev\"]\n").unwrap();
        let mut items = markdown::parse(Path::new("reqs/a.md"), requirements)
            .unwrap()
            .items;
        items.extend(tags::scan(Path::new("tests/a.cpp"), tests).unwrap().items);
        let trace = trace::trace(&items, &config);
        let results = junit::parse(Path::new("results.xml"), junit)
            .unwrap()
            .results;
        let evaluation = results::evaluate(&items, &results, &config, &Packages::default());
        trace
            .items
            .iter()
            .zip(verify(&items, &trace, &config, Some(&evaluation)))
            .filter(|(item, _)| item.location.path() == "reqs/a.md")
            .map(|(item, verified)| format!("{} {verified}", item.id))
            .collect()
    }

    #[test]
    fn skipped_tests_and_tags_without_test_verify_nothing() {
        let tests = "\
// [utest->dsn~a-k4f2~1]
TEST(Suite, Skipped) {}

// [utest->dsn~b-k4f2~1]
int helper();
";
        let junit = r#"<testsuite name="Suite"><testcase classname="Suite" name="Skipped"><skipped/></testcase></testsuite>"#;
        let lines = run_without_levels(
            "`dsn~a-k4f2~1`\nNeeds: utest\n\n`dsn~b-k4f2~1`\nNeeds: utest\n",
            tests,
            junit,
        );
        assert_eq!(lines, ["dsn~a-k4f2~1 false", "dsn~b-k4f2~1 false"]);
    }

    #[test]
    fn a_shared_source_decides_both_paths() {
        // swr~s <- dsn~c <- vrev~r and swr~s <- dsn~d <- vrev~r.
        let text = "\
`swr~s-k4f2~1`
Needs: dsn

`dsn~c-k4f2~1`
Needs: vrev
Covers:
- swr~s-k4f2~1

`dsn~d-k4f2~1`
Needs: vrev
Covers:
- swr~s-k4f2~1

`vrev~r-k4f2~1`
Covers:
- dsn~c-k4f2~1
- dsn~d-k4f2~1
";
        let lines = run_without_levels(text, "", "<testsuite name=\"S\"/>");
        assert!(
            lines.iter().all(|line| line.ends_with(" true")),
            "{lines:?}"
        );
        let lines = run_without_levels(
            &text.replace("`vrev~r-k4f2~1`\n", "`vrev~r-k4f2~1`\nStatus: draft\n"),
            "",
            "<testsuite name=\"S\"/>",
        );
        assert!(
            lines.iter().all(|line| line.ends_with(" false")),
            "{lines:?}"
        );
    }

    #[test]
    fn items_in_or_above_a_cycle_are_not_verified() {
        let text = "\
`swr~s-k4f2~1`
Needs: dsn

`dsn~a-k4f2~1`
Needs: dsn
Covers:
- swr~s-k4f2~1
- dsn~b-k4f2~1

`dsn~b-k4f2~1`
Needs: dsn
Covers:
- dsn~a-k4f2~1
";
        let lines = run_without_levels(text, "", "<testsuite name=\"S\"/>");
        assert_eq!(
            lines,
            [
                "swr~s-k4f2~1 false",
                "dsn~a-k4f2~1 false",
                "dsn~b-k4f2~1 false"
            ]
        );
    }

    #[test]
    fn long_chains_do_not_overflow_the_stack() {
        // A tiny stack: a recursive search would overflow long before
        // 20 000 levels, the iterative one does not.
        let count = 20_000;
        let mut items = Vec::new();
        for i in 0..count {
            let mut text = format!("`swr~n{i}~1`\n");
            if i + 1 < count {
                text.push_str("Needs: swr\n");
            }
            if i > 0 {
                text.push_str(&format!("Covers:\n- swr~n{}~1\n", i - 1));
            }
            items.extend(markdown::parse(Path::new("x.md"), &text).unwrap().items);
        }
        let verified = std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(move || {
                let config = Config::default();
                let trace = trace::trace(&items, &config);
                verify(&items, &trace, &config, None)
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(verified.len(), count);
        assert!(verified.iter().all(|&verified| verified));
    }

    #[test]
    fn verified_needs_deep_coverage() {
        // dsn~u covers swr~s too, but nothing implements it: swr~s is
        // covered, but neither deep covered nor verified.
        let extra = "\n# U\n\n`dsn~u-k4f2~1`\n\nCovers:\n- swr~s-k4f2~1\n\nNeeds: impl\n";
        let lines = run(&design("approved", extra), true);
        assert_eq!(lines[2], "dsn~c-k4f2~1 true");
        assert_eq!(lines[3], "swr~s-k4f2~1 false");
        assert_eq!(lines[5], "dsn~u-k4f2~1 false");
    }

    #[test]
    fn without_junit_sources_no_test_has_passed() {
        let lines = run(&design("approved", ""), false);
        assert!(
            lines
                .iter()
                .filter(|line| line.starts_with("dsn") || line.starts_with("swr"))
                .all(|line| line.ends_with(" false")),
            "{lines:?}"
        );
        // A review needs no test.
        assert_eq!(lines[4], "vrev~r-k4f2~1 true");
    }

    #[test]
    fn rejected_duplicate_and_unknown_items_are_not_verified() {
        let config = Config::parse("[[level]]\ntype = \"dsn\"\nname = \"Design\"\n").unwrap();
        let text = "\
# A

`dsn~a-k4f2~1`

# A again

`dsn~a-k4f2~1`

# R

`dsn~r-k4f2~1`

Status: rejected

# U

`dns~u-k4f2~1`

# D

`dsn~d-k4f2~1`
";
        let items = markdown::parse(Path::new("reqs/a.md"), text).unwrap().items;
        let trace = trace::trace(&items, &config);
        assert_eq!(
            verify(&items, &trace, &config, None),
            [false, false, false, false, true]
        );
    }
}
