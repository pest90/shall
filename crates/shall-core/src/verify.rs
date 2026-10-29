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
//!
//! The [`Light`] of an item sums up its whole chain for the Green Graph:
//! the worst light of the item itself and of everything covering it, so
//! that a requirement is green only when the chain below it is. It tells
//! an open gap or defect (red) from missing evidence (yellow); a green
//! item is verified, but a verified item with an invalid link of its own
//! is red, and one in draft yellow.
//!
//! An item that an import provides, but that the project does not
//! request, is *provided, not requested*: information for a project
//! report, grey, and nothing in the lights above it. The project
//! requests its own items, what they cover and the items above that, and
//! everything below its own items and below what they cover directly.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;

use crate::config::{Config, NotExecuted};
use crate::id::ItemId;
use crate::item::{Item, Location, Origin, Status};
use crate::results::{Evaluation, TestRun, Verdict};
use crate::trace::{Link, LinkKind, LinkStatus, Trace, TracedItem};

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

/// The status of an item and everything below it, as a traffic light.
///
/// The light of an item by itself is
///
/// - grey for a rejected item, which needs nothing;
/// - red for a copy of a duplicate ID, an item of an unknown type, a
///   missing type, an invalid link of the item itself (such as an outdated
///   revision; a `Covers:` link out of the traced variant is fine), a
///   failed test, and a test that was not executed if `[verification]
///   not-executed` makes that an error;
/// - yellow for missing evidence: a test that was skipped or, by default,
///   not executed, a verification tag without a test definition or
///   without test results (no `junit` sources), a review that is not
///   approved, and an item in `draft` or `proposed`;
/// - green otherwise.
///
/// The light of the chain is the worst of the item's own and those of the
/// items covering it with a valid link, red before yellow before green;
/// an item in a coverage cycle, and every item above one, is red. The
/// order of the variants is that ranking, with grey below green, so that
/// the worst of several lights is their maximum and a rejected item never
/// hides another one's light. Build data (M9) will add yellow and red for
/// code that is not known to be built.
///
/// A green item is verified. A verified item need not be green: an
/// invalid link of its own makes it red, and an item in draft makes it
/// yellow, while "verified" looks only at what is below the item and at
/// tests and reviews.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Light {
    /// Rejected, and not counted; or provided by an import but requested
    /// by nothing of the project, which [`crate::export`] makes grey.
    Grey,
    /// Complete: covered all the way down, every test passed, every review
    /// approved, nothing in draft.
    Green,
    /// Complete, but evidence is missing.
    Yellow,
    /// A gap or a defect.
    Red,
}

impl Light {
    /// Sign of the light, as in the report: `✓`, `!`, `✗` or `–`.
    pub fn sign(self) -> &'static str {
        match self {
            Light::Green => "✓",
            Light::Yellow => "!",
            Light::Red => "✗",
            Light::Grey => "–",
        }
    }

    /// The light in words.
    pub fn as_str(self) -> &'static str {
        match self {
            Light::Green => "green",
            Light::Yellow => "yellow",
            Light::Red => "red",
            Light::Grey => "grey",
        }
    }
}

/// The light of the item by itself, without the items covering it and
/// without cycles, which [`lights`] adds; `broken` tells whether one of
/// its own links is a defect.
fn own_light(entry: &Joined<'_>, broken: bool, config: &Config) -> Light {
    let traced = entry.traced;
    if traced.rejected {
        return Light::Grey;
    }
    if traced.duplicate || traced.unknown_type || !traced.missing.is_empty() || broken {
        return Light::Red;
    }
    if config.verification.types.contains(entry.item.id.typ()) {
        return match entry.item.origin {
            Origin::Tag { .. } => match entry.run.map(|run| run.verdict) {
                Some(Verdict::Passed) => Light::Green,
                Some(Verdict::Failed) => Light::Red,
                Some(Verdict::NotExecuted)
                    if config.verification.not_executed == NotExecuted::Error =>
                {
                    Light::Red
                }
                _ => Light::Yellow,
            },
            Origin::Requirement if entry.item.status == Status::Approved => Light::Green,
            Origin::Requirement => Light::Yellow,
        };
    }
    match entry.item.status {
        Status::Draft | Status::Proposed => Light::Yellow,
        Status::Approved | Status::Rejected => Light::Green,
    }
}

/// The light of every joined item's chain, in their order; `links` are
/// those of the trace.
///
/// A `provided` item is grey and counts as nothing in the chains above it.
pub(crate) fn lights(
    joined: &[Joined<'_>],
    links: &[Link],
    provided: &[bool],
    config: &Config,
) -> Vec<Light> {
    let broken: HashSet<(&Location, &ItemId)> = links
        .iter()
        .filter(|link| {
            !matches!(
                (link.kind, link.status),
                (_, LinkStatus::Valid) | (LinkKind::Covers, LinkStatus::OutsideVariant)
            )
        })
        .map(|link| (&link.source_location, &link.source))
        .collect();
    let own = |index: usize| {
        if provided.get(index).copied().unwrap_or(false) {
            return Light::Grey;
        }
        let entry = &joined[index];
        let broken = broken.contains(&(&entry.item.location, &entry.item.id));
        own_light(entry, broken, config)
    };
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Unvisited,
        Visiting,
        Done(Light),
    }
    let sources = sources(joined);
    let mut state = vec![State::Unvisited; joined.len()];
    // Depth first without recursion; an item met again while it is being
    // visited closes a coverage cycle, which is red.
    for root in 0..joined.len() {
        if state[root] != State::Unvisited {
            continue;
        }
        let mut stack = vec![(root, 0)];
        state[root] = State::Visiting;
        let mut worst = vec![(root, own(root))];
        while let Some(&mut (node, ref mut next)) = stack.last_mut() {
            if let Some(&source) = sources[node].get(*next) {
                *next += 1;
                match state[source] {
                    State::Unvisited => {
                        state[source] = State::Visiting;
                        stack.push((source, 0));
                        worst.push((source, own(source)));
                    }
                    State::Visiting => {
                        let top = worst.last_mut().expect("a light per visit");
                        top.1 = top.1.max(Light::Red);
                    }
                    State::Done(light) => {
                        let top = worst.last_mut().expect("a light per visit");
                        top.1 = top.1.max(light);
                    }
                }
                continue;
            }
            stack.pop();
            let (_, light) = worst.pop().expect("a light per visit");
            state[node] = State::Done(light);
            if let Some(parent) = worst.last_mut() {
                parent.1 = parent.1.max(light);
            }
        }
    }
    state
        .into_iter()
        .map(|state| match state {
            State::Done(light) => light,
            _ => unreachable!("every item is visited"),
        })
        .collect()
}

/// For every joined item, whether it is *provided, not requested*: it
/// comes from an import, and the project does not request it. The
/// project requests its own items, what they cover and the items above
/// that (e.g. a platform capability its own code implements), and
/// everything below its own items and below what they cover directly, by
/// `Covers:` or `Satisfied-by:` (e.g. the capabilities its customer
/// requirements are satisfied by, with their design, code and tests).
/// The other items below an item above are not requested by that: they
/// stay provided, e.g. the sibling capabilities under a platform's top
/// item. Provided items, e.g. platform capabilities no customer
/// requirement asks for, are information.
pub(crate) fn provided(joined: &[Joined<'_>]) -> Vec<bool> {
    let sources = sources(joined);
    let mut targets: Vec<Vec<usize>> = vec![Vec::new(); joined.len()];
    for (target, sources) in sources.iter().enumerate() {
        for &source in sources {
            targets[source].push(target);
        }
    }
    let own: Vec<usize> = (0..joined.len())
        .filter(|&index| joined[index].item.import.is_none())
        .collect();
    let mut requested = vec![false; joined.len()];
    // Upwards from the own items: what they cover, and what that covers.
    let mut stack = own.clone();
    while let Some(node) = stack.pop() {
        for &target in &targets[node] {
            if !requested[target] {
                requested[target] = true;
                stack.push(target);
            }
        }
    }
    // Downwards from the own items and what they cover directly.
    let mut stack: Vec<usize> = own
        .iter()
        .flat_map(|&node| std::iter::once(node).chain(targets[node].iter().copied()))
        .collect();
    for &node in &stack {
        requested[node] = true;
    }
    while let Some(node) = stack.pop() {
        for &source in &sources[node] {
            if !requested[source] {
                requested[source] = true;
                stack.push(source);
            }
        }
    }
    requested.into_iter().map(|requested| !requested).collect()
}

/// For every joined item, the positions of the items covering it.
fn sources(joined: &[Joined<'_>]) -> Vec<Vec<usize>> {
    let mut position: HashMap<(&Location, &ItemId), usize> = HashMap::new();
    for (index, entry) in joined.iter().enumerate() {
        position
            .entry((&entry.item.location, &entry.item.id))
            .or_insert(index);
    }
    joined
        .iter()
        .map(|entry| {
            entry
                .traced
                .covered_by
                .iter()
                .map(|source| position[&(&source.location, &source.id)])
                .collect()
        })
        .collect()
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
    let sources = sources(joined);

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
        run_with(CONFIG, requirements, with_junit)
    }

    /// [`run`] with another configuration.
    fn run_with(config: &str, requirements: &str, with_junit: bool) -> Vec<String> {
        let config = Config::parse(config).unwrap();
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

    #[test]
    fn satisfied_by_takes_the_verified_status_of_its_targets() {
        let config = format!("{CONFIG}\n[satisfied_by]\ntargets = [\"dsn\"]\n");
        let requirements = "\
`swr~p-k4f2~1`
Satisfied-by:
- dsn~a-k4f2~1

`swr~q-k4f2~1`
Satisfied-by:
- dsn~a-k4f2~1
- dsn~b-k4f2~1

`dsn~a-k4f2~1`

`dsn~b-k4f2~1`

`dsn~c-k4f2~1`
";
        assert_eq!(
            run_with(&config, requirements, true),
            [
                "swr~p-k4f2~1 true",
                // The test of dsn~b failed.
                "swr~q-k4f2~1 false",
                "dsn~a-k4f2~1 true",
                "dsn~b-k4f2~1 false",
                "dsn~c-k4f2~1 true",
            ]
        );
    }

    /// The lights of the requirements plus the tags above, as `ID light`
    /// lines in the order of the trace.
    fn lights_of(requirements: &str, with_junit: bool) -> Vec<String> {
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
        let joined = join(&items, &trace, with_junit.then_some(&evaluation));
        joined
            .iter()
            .zip(lights(&joined, &trace.links, &[], &config))
            .filter(|(entry, _)| entry.item.location.path() == "reqs/a.md")
            .map(|(entry, light)| format!("{} {}", entry.item.id, light.as_str()))
            .collect()
    }

    #[test]
    fn lights_show_the_worst_of_the_chain() {
        assert_eq!(
            lights_of(&design("approved", ""), true),
            [
                // In draft: complete, but not mature.
                "dsn~a-k4f2~1 yellow",
                // Its test failed.
                "dsn~b-k4f2~1 red",
                "dsn~c-k4f2~1 green",
                "swr~s-k4f2~1 green",
                "vrev~r-k4f2~1 green",
            ]
        );
        // Without results, every test is missing evidence; a review that is
        // not approved too.
        assert_eq!(
            lights_of(&design("proposed", ""), false),
            [
                "dsn~a-k4f2~1 yellow",
                "dsn~b-k4f2~1 yellow",
                "dsn~c-k4f2~1 yellow",
                "swr~s-k4f2~1 yellow",
                "vrev~r-k4f2~1 yellow",
            ]
        );
    }

    #[test]
    fn gaps_cycles_and_rejected_items_have_their_own_lights() {
        let lines = lights_of(
            "\
`swr~gap-k4f2~1`

`swr~top-k4f2~1`

`dsn~below-k4f2~1`
Needs: dsn
Covers:
- swr~top-k4f2~1

`dsn~x-k4f2~1`
Needs: dsn
Covers:
- dsn~y-k4f2~1

`dsn~y-k4f2~1`
Needs: dsn
Covers:
- dsn~x-k4f2~1

`swr~old-k4f2~1`
Status: rejected

`vrev~stale-k4f2~1`
Covers:
- swr~top-k4f2~0
",
            true,
        );
        assert_eq!(
            lines,
            [
                // Not covered by dsn.
                "swr~gap-k4f2~1 red",
                // The item below has a gap.
                "swr~top-k4f2~1 red",
                "dsn~below-k4f2~1 red",
                "dsn~x-k4f2~1 red",
                "dsn~y-k4f2~1 red",
                "swr~old-k4f2~1 grey",
                // Its link names a revision that does not exist.
                "vrev~stale-k4f2~1 red",
            ]
        );
    }

    #[test]
    fn grey_is_below_every_other_light() {
        let lights = [Light::Grey, Light::Green, Light::Yellow, Light::Red];
        assert_eq!(lights.iter().max(), Some(&Light::Red));
        assert_eq!(
            [Light::Grey, Light::Green].iter().max(),
            Some(&Light::Green)
        );
    }

    #[test]
    fn imports_are_requested_through_the_own_items() {
        let config = Config::parse(
            "[[level]]\ntype = \"hlr\"\nname = \"H\"\n\n\
             [[level]]\ntype = \"cap\"\nname = \"C\"\nneeds = [\"impl\"]\n",
        )
        .unwrap();
        let mut items = markdown::parse(
            Path::new("a/reqs.md"),
            "`hlr~h~1`\nSatisfied-by:\n- cap~asked~1\n",
        )
        .unwrap()
        .items;
        // The own code implements a capability nobody asks for.
        items.extend(
            tags::scan(Path::new("a/log.c"), "// [impl->cap~log~1]\n")
                .unwrap()
                .items,
        );
        let mut platform = markdown::parse(
            Path::new("p/caps.md"),
            "`cap~asked~1`\n\n`cap~log~1`\n\n`cap~unused~1`\n",
        )
        .unwrap()
        .items;
        platform.extend(
            tags::scan(
                Path::new("p/caps.c"),
                "// [impl->cap~asked~1]\n// [impl->cap~log~1]\n// [impl->cap~unused~1]\n",
            )
            .unwrap()
            .items,
        );
        for item in &mut platform {
            item.import = Some("plat".to_owned());
        }
        items.extend(platform);
        let trace = trace::trace(&items, &config);
        let joined = join(&items, &trace, None);
        let lines: Vec<String> = joined
            .iter()
            .zip(provided(&joined))
            .filter(|(_, provided)| *provided)
            .map(|(entry, _)| entry.item.location.to_string())
            .collect();
        // Only the unused capability and its code are provided.
        assert_eq!(lines, ["p/caps.c:3", "p/caps.md:5"]);
    }

    #[test]
    fn items_above_are_requested_but_not_their_other_children() {
        let config = Config::parse(
            "[[level]]\ntype = \"top\"\nname = \"T\"\nneeds = [\"cap\"]\n\n\
             [[level]]\ntype = \"cap\"\nname = \"C\"\nneeds = [\"impl\"]\n",
        )
        .unwrap();
        let mut items = tags::scan(Path::new("a/log.c"), "// [impl->cap~log~1]\n")
            .unwrap()
            .items;
        let mut platform = markdown::parse(
            Path::new("p/caps.md"),
            "`top~t~1`\n\n`cap~log~1`\nCovers:\n- top~t~1\n\n`cap~unused~1`\nCovers:\n- top~t~1\n",
        )
        .unwrap()
        .items;
        platform.extend(
            tags::scan(Path::new("p/caps.c"), "// [impl->cap~unused~1]\n")
                .unwrap()
                .items,
        );
        for item in &mut platform {
            item.import = Some("plat".to_owned());
        }
        items.extend(platform);
        let trace = trace::trace(&items, &config);
        let joined = join(&items, &trace, None);
        let provided = provided(&joined);
        let names: Vec<String> = joined
            .iter()
            .zip(&provided)
            .filter(|(_, provided)| **provided)
            .map(|(entry, _)| entry.item.location.to_string())
            .collect();
        // The sibling of the implemented capability stays provided.
        assert_eq!(names, ["p/caps.c:1", "p/caps.md:7"]);
        // And it does not count in the light of the top item.
        let light: Vec<(String, Light)> = joined
            .iter()
            .zip(lights(&joined, &trace.links, &provided, &config))
            .map(|(entry, light)| (entry.item.id.to_string(), light))
            .collect();
        assert!(
            light.contains(&("top~t~1".to_owned(), Light::Green)),
            "{light:?}"
        );
        assert!(
            light.contains(&("cap~unused~1".to_owned(), Light::Grey)),
            "{light:?}"
        );
    }
}
