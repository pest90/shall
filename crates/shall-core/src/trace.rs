//! Tracing: resolve links between items, check them against the levels
//! of the project and compute coverage.
//!
//! An item is *covered* when, for every type it needs, at least one item
//! of that type covers it with a valid link. It is *deep covered* when it
//! is covered, every item covering it is deep covered too, and it is not
//! part of a coverage cycle.
//!
//! An item needs the types of its own `Needs:`; without them, the `needs`
//! of its level. Without any `[[level]]`, only `Needs:` of items apply.
//!
//! A `Covers:` link is valid when its target exists once, in exactly the
//! linked revision, is not rejected, and needs the type of the linking
//! item. Every other link is a defect at the line of the link and does
//! not count as coverage; [`LinkStatus`] lists the cases. `Depends:` links
//! are checked the same way, except for the needed type, and never count
//! as coverage.
//!
//! Only root causes are defects:
//!
//! - an item that is covered but not deep covered is a consequence of a
//!   defect further down and only a status;
//! - copies of a duplicate ID get no defects for missing coverage, since
//!   the duplicate itself is reported; their links are checked;
//! - rejected items need no coverage and count as covered; their own
//!   links are ignored, and links to them are defects.
//!
//! A type is known if it is a level, needed by a level or a verification
//! type in `shall.toml`. An item of an unknown type is an error; a type
//! known only from the `Needs:` of items gets a warning, since it is often
//! a typo. Without any `[[level]]`, types are not checked.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;

use crate::check;
use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::id::{ItemId, ItemType};
use crate::item::{Item, Location, Origin, Reference, Status};

/// Result of tracing all items of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Trace {
    /// Every item, ordered by location.
    pub items: Vec<TracedItem>,
    /// Every `Covers:` and `Depends:` link of items that are not rejected,
    /// ordered by location.
    pub links: Vec<Link>,
    /// Defects, ordered by location.
    pub defects: Vec<Diagnostic>,
}

/// Coverage of one item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TracedItem {
    /// The item's ID.
    pub id: ItemId,
    /// Where the item is defined.
    pub location: Location,
    /// The item's status.
    pub status: Status,
    /// Types the item needs, from `Needs:` or its level; empty for rejected
    /// items.
    pub needs: Vec<ItemType>,
    /// Items that cover this item with a valid link, ordered by location.
    pub covered_by: Vec<ItemRef>,
    /// Needed types without a covering item.
    pub missing: Vec<ItemType>,
    /// Whether every needed type is covered; always true for rejected items.
    pub covered: bool,
    /// Whether the item is covered, every covering item is deep covered and
    /// the item is not part of a coverage cycle.
    pub deep_covered: bool,
    /// Whether another item has the same type and name.
    pub duplicate: bool,
    /// Whether the item has `Status: rejected`.
    pub rejected: bool,
}

/// Reference to an item that tells copies of a duplicate ID apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemRef {
    /// The item's ID.
    pub id: ItemId,
    /// Where the item is defined.
    pub location: Location,
}

/// A link from one item to another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    /// `Covers:` or `Depends:`.
    pub kind: LinkKind,
    /// The item that contains the link.
    pub source: ItemId,
    /// The target as written, including the linked revision.
    pub target: ItemId,
    /// Where the link is written.
    pub location: Location,
    /// Whether and why the link is valid or not.
    #[serde(flatten)]
    pub status: LinkStatus,
}

/// Kind of a [`Link`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// `Covers:`: the source refines or implements the target.
    Covers,
    /// `Depends:`: a lateral dependency without effect on coverage.
    Depends,
}

/// Status of a [`Link`]. Only [`LinkStatus::Valid`] links count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum LinkStatus {
    /// The target exists once in the linked revision and accepts the link.
    Valid,
    /// No item has the target's type and name.
    Unknown,
    /// The target has a higher revision: it changed after the link was made.
    Outdated {
        /// The target's current revision.
        current: u32,
    },
    /// The target has a lower revision than linked.
    Predated {
        /// The target's current revision.
        current: u32,
    },
    /// More than one item has the target's type and name.
    Ambiguous,
    /// The target is rejected.
    Rejected,
    /// The target does not need the type of the source (`Covers:` only).
    Unwanted,
}

/// Traces `items` against the levels of `config`.
///
/// Diagnostics of the parsers are not part of the result; duplicate IDs
/// are, because they make links ambiguous.
pub fn trace(items: &[Item], config: &Config) -> Trace {
    let mut items: Vec<&Item> = items.iter().collect();
    items.sort_by(|a, b| a.location.cmp(&b.location));
    let mut defects = check::duplicate_ids(items.iter().copied());

    let mut copies: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (position, item) in items.iter().enumerate() {
        copies
            .entry((item.id.typ().as_str(), item.id.name()))
            .or_default()
            .push(position);
    }
    let duplicate: Vec<bool> = items
        .iter()
        .map(|item| copies[&(item.id.typ().as_str(), item.id.name())].len() > 1)
        .collect();
    let rejected: Vec<bool> = items
        .iter()
        .map(|item| item.status == Status::Rejected)
        .collect();
    let needs: Vec<Vec<ItemType>> = items
        .iter()
        .zip(&rejected)
        .map(|(item, &rejected)| {
            if rejected {
                Vec::new()
            } else {
                effective_needs(item, config)
            }
        })
        .collect();
    let configured: HashSet<&ItemType> = config
        .levels
        .iter()
        .flat_map(|level| std::iter::once(&level.typ).chain(&level.needs))
        .chain(&config.verification.types)
        .collect();
    let needed_by_items: HashSet<&ItemType> = needs
        .iter()
        .zip(&rejected)
        .filter(|(_, rejected)| !**rejected)
        .flat_map(|(needs, _)| needs)
        .collect();

    // covered_by[target] = sources with a valid link to the target.
    let mut covered_by: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
    let mut links = Vec::new();
    for (source, item) in items.iter().enumerate() {
        if rejected[source] {
            continue;
        }
        let typ = item.id.typ();
        let subject = match &item.origin {
            Origin::Tag { text, .. } => format!("tag {text}"),
            Origin::Requirement => item.id.to_string(),
        };
        if !config.levels.is_empty() && !configured.contains(typ) {
            if needed_by_items.contains(typ) {
                defects.push(Diagnostic::warning(
                    item.location.clone(),
                    format!(
                        "{subject}: type '{typ}' is only known from Needs: of items; \
                         add it to shall.toml if it is no typo"
                    ),
                ));
            } else {
                defects.push(Diagnostic::error(
                    item.location.clone(),
                    format!(
                        "{subject}: type '{typ}' is not a level, a verification type or needed by any item"
                    ),
                ));
            }
        }
        let all_links = item
            .covers
            .iter()
            .map(|link| (LinkKind::Covers, link))
            .chain(item.depends.iter().map(|link| (LinkKind::Depends, link)));
        for (kind, link) in all_links {
            let resolved = resolve(&copies, &items, &rejected, link);
            let status = match resolved {
                Ok(target) if kind == LinkKind::Covers => {
                    if needs[target].contains(item.id.typ()) {
                        covered_by[target].push(source);
                        LinkStatus::Valid
                    } else {
                        LinkStatus::Unwanted
                    }
                }
                Ok(_) => LinkStatus::Valid,
                Err(status) => status,
            };
            if let Some(message) = link_message(kind, item, link, status) {
                defects.push(Diagnostic::error(
                    item.location.with_line(link.line),
                    message,
                ));
            }
            links.push(Link {
                kind,
                source: item.id.clone(),
                target: link.id.clone(),
                location: item.location.with_line(link.line),
                status,
            });
        }
    }
    for sources in &mut covered_by {
        sources.sort_unstable();
        sources.dedup();
    }

    let mut traced: Vec<TracedItem> = items
        .iter()
        .enumerate()
        .map(|(position, item)| {
            let covering_types: Vec<&ItemType> = covered_by[position]
                .iter()
                .map(|&source| items[source].id.typ())
                .collect();
            let missing: Vec<ItemType> = needs[position]
                .iter()
                .filter(|typ| !covering_types.contains(typ))
                .cloned()
                .collect();
            if !duplicate[position] && !rejected[position] {
                for typ in &missing {
                    defects.push(Diagnostic::error(
                        item.location.clone(),
                        format!("{}: not covered by required type '{typ}'", item.id),
                    ));
                }
            }
            TracedItem {
                id: item.id.clone(),
                location: item.location.clone(),
                status: item.status,
                needs: needs[position].clone(),
                covered_by: covered_by[position]
                    .iter()
                    .map(|&source| ItemRef {
                        id: items[source].id.clone(),
                        location: items[source].location.clone(),
                    })
                    .collect(),
                covered: missing.is_empty(),
                missing,
                deep_covered: false,
                duplicate: duplicate[position],
                rejected: rejected[position],
            }
        })
        .collect();

    let covered: Vec<bool> = traced.iter().map(|item| item.covered).collect();
    let deep = deep_coverage(&items, &covered_by, &covered, &mut defects);
    for (item, deep) in traced.iter_mut().zip(deep) {
        item.deep_covered = deep;
    }

    defects.sort_by(|a, b| a.location.cmp(&b.location));
    links.sort_by(|a, b| a.location.cmp(&b.location));
    Trace {
        items: traced,
        links,
        defects,
    }
}

/// Types the item needs: its own `Needs:`, or else those of its level.
fn effective_needs(item: &Item, config: &Config) -> Vec<ItemType> {
    if !item.needs.is_empty() {
        return item.needs.clone();
    }
    config
        .level(item.id.typ())
        .map(|level| level.needs.clone())
        .unwrap_or_default()
}

/// Finds the single target of a link in the linked revision.
fn resolve(
    copies: &HashMap<(&str, &str), Vec<usize>>,
    items: &[&Item],
    rejected: &[bool],
    link: &Reference,
) -> Result<usize, LinkStatus> {
    let key = (link.id.typ().as_str(), link.id.name());
    let target = match copies.get(&key).map(Vec::as_slice) {
        None => return Err(LinkStatus::Unknown),
        Some([target]) => *target,
        Some(_) => return Err(LinkStatus::Ambiguous),
    };
    // A rejected target is the root cause even if the revision differs
    // too: updating the revision would not make the link valid.
    if rejected[target] {
        return Err(LinkStatus::Rejected);
    }
    let current = items[target].id.revision();
    let linked = link.id.revision();
    if linked < current {
        Err(LinkStatus::Outdated { current })
    } else if linked > current {
        Err(LinkStatus::Predated { current })
    } else {
        Ok(target)
    }
}

/// The defect message for an invalid link, without location.
fn link_message(
    kind: LinkKind,
    item: &Item,
    link: &Reference,
    status: LinkStatus,
) -> Option<String> {
    let verb = match kind {
        LinkKind::Covers => "covers",
        LinkKind::Depends => "depends on",
    };
    let target = &link.id;
    // The ID of a tag's item is generated, so its defects name the tag as
    // written; a tag with an unknown target is orphaned code.
    let (subject, is_tag) = match &item.origin {
        Origin::Tag { text, .. } => (format!("tag {text}"), true),
        Origin::Requirement => (item.id.to_string(), false),
    };
    let problem = match status {
        LinkStatus::Valid => return None,
        LinkStatus::Unknown if is_tag => format!("unknown target {target}"),
        LinkStatus::Unknown => format!("{verb} unknown item {target}"),
        LinkStatus::Outdated { current } => {
            format!("{verb} outdated revision {target} (current revision: {current})")
        }
        LinkStatus::Predated { current } => {
            format!("{verb} nonexistent revision {target} (current revision: {current})")
        }
        LinkStatus::Ambiguous => {
            format!("{verb} {target}, whose type and name are defined more than once")
        }
        LinkStatus::Rejected => format!("{verb} rejected item {target}"),
        LinkStatus::Unwanted => format!(
            "{verb} {target}, which does not need type '{}'",
            item.id.typ()
        ),
    };
    Some(format!("{subject}: {problem}"))
}

/// Computes deep coverage and reports coverage cycles.
///
/// The graph has an edge from every item to the items covering it. With
/// Tarjan's algorithm, run iteratively so that long chains cannot overflow
/// the stack, every strongly connected component is complete only after
/// all components it reaches, so the covering items are evaluated first.
/// A component with more than one item, or an item covering itself, is a
/// cycle: none of its items is deep covered, and one cycle through its
/// first item by location is reported there, independent of the order in
/// which the graph is searched.
fn deep_coverage(
    items: &[&Item],
    covered_by: &[Vec<usize>],
    covered: &[bool],
    defects: &mut Vec<Diagnostic>,
) -> Vec<bool> {
    const UNVISITED: usize = usize::MAX;
    let count = items.len();
    let mut order = vec![UNVISITED; count];
    let mut lowlink = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut deep = vec![false; count];
    let mut next = 0;

    for root in 0..count {
        if order[root] != UNVISITED {
            continue;
        }
        // (item, index of its next edge)
        let mut calls = vec![(root, 0)];
        order[root] = next;
        lowlink[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(node, edge)) = calls.last() {
            if let Some(&source) = covered_by[node].get(edge) {
                calls.last_mut().expect("a call is active").1 += 1;
                if order[source] == UNVISITED {
                    order[source] = next;
                    lowlink[source] = next;
                    next += 1;
                    stack.push(source);
                    on_stack[source] = true;
                    calls.push((source, 0));
                } else if on_stack[source] {
                    lowlink[node] = lowlink[node].min(order[source]);
                }
                continue;
            }
            calls.pop();
            if let Some(&(parent, _)) = calls.last() {
                lowlink[parent] = lowlink[parent].min(lowlink[node]);
            }
            if lowlink[node] != order[node] {
                continue;
            }
            let mut component = Vec::new();
            loop {
                let member = stack.pop().expect("the component is on the stack");
                on_stack[member] = false;
                component.push(member);
                if member == node {
                    break;
                }
            }
            let cyclic = component.len() > 1 || covered_by[node].contains(&node);
            if cyclic {
                report_cycle(items, covered_by, &component, defects);
            } else {
                deep[node] = covered[node] && covered_by[node].iter().all(|&s| deep[s]);
            }
        }
    }
    deep
}

/// Most IDs named in a cycle message; longer cycles are shortened.
const MAX_CYCLE_NAMES: usize = 10;

/// Reports the shortest cycle through the first item of a component.
fn report_cycle(
    items: &[&Item],
    covered_by: &[Vec<usize>],
    component: &[usize],
    defects: &mut Vec<Diagnostic>,
) {
    let first = *component.iter().min().expect("a component is not empty");
    let members: HashSet<usize> = component.iter().copied().collect();
    // Breadth-first search within the component, back to the first item.
    let mut previous: HashMap<usize, usize> = HashMap::new();
    let mut queue = VecDeque::from([first]);
    let mut last = None;
    'search: while let Some(node) = queue.pop_front() {
        for &source in &covered_by[node] {
            if source == first {
                last = Some(node);
                break 'search;
            }
            if members.contains(&source) && !previous.contains_key(&source) {
                previous.insert(source, node);
                queue.push_back(source);
            }
        }
    }
    let mut path = vec![first];
    let mut node = last.expect("a cyclic component has a cycle through each item");
    while node != first {
        path.push(node);
        node = previous[&node];
    }
    // path is first, ..., in reverse order of the search; turn it into
    // "first <- covered by ... <- first".
    path[1..].reverse();
    path.push(first);
    let mut names: Vec<String> = path.iter().map(|&p| items[p].id.to_string()).collect();
    if names.len() > MAX_CYCLE_NAMES {
        let omitted = names.len() - MAX_CYCLE_NAMES;
        names.splice(
            MAX_CYCLE_NAMES - 1..names.len() - 1,
            [format!("... ({omitted} more)")],
        );
    }
    let item = items[first];
    defects.push(Diagnostic::error(
        item.location.clone(),
        format!("{}: coverage cycle {}", item.id, names.join(" <- ")),
    ));
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::markdown;

    const CONFIG: &str = r#"
[[level]]
type = "sys"
name = "System"
needs = ["swr"]

[[level]]
type = "swr"
name = "Software"
needs = ["dsn"]

[[level]]
type = "dsn"
name = "Design"
needs = ["impl"]

[verification]
types = ["utest", "vrev"]
"#;

    fn run_files(files: &[(&str, &str)], config: &str) -> Trace {
        let mut items = Vec::new();
        for (path, text) in files {
            let parsed = markdown::parse(Path::new(path), text).unwrap();
            assert_eq!(parsed.diagnostics, []);
            items.extend(parsed.items);
        }
        trace(&items, &Config::parse(config).unwrap())
    }

    fn run(text: &str) -> Trace {
        run_files(&[("x.md", text)], CONFIG)
    }

    fn messages(trace: &Trace) -> Vec<String> {
        trace.defects.iter().map(ToString::to_string).collect()
    }

    fn item<'a>(trace: &'a Trace, id: &str) -> &'a TracedItem {
        trace
            .items
            .iter()
            .find(|item| item.id.to_string() == id)
            .unwrap()
    }

    fn statuses(trace: &Trace) -> Vec<(String, LinkStatus)> {
        trace
            .links
            .iter()
            .map(|link| (link.target.to_string(), link.status))
            .collect()
    }

    #[test]
    fn complete_chain_is_deep_covered() {
        let trace = run("\
`sys~a~1`

`swr~b~1`
Covers:
- sys~a~1

`dsn~c~1`
Covers:
- swr~b~1

`impl~d~0`
Covers:
- dsn~c~1
");
        assert_eq!(messages(&trace), Vec::<String>::new());
        assert!(trace.items.iter().all(|item| item.deep_covered));
        assert_eq!(
            item(&trace, "sys~a~1").covered_by[0].id.to_string(),
            "swr~b~1"
        );
        assert!(
            trace
                .links
                .iter()
                .all(|link| link.status == LinkStatus::Valid)
        );
    }

    #[test]
    fn reports_missing_coverage_once_at_the_root_cause() {
        let trace = run("\
`sys~a~1`

`swr~b~1`
Covers:
- sys~a~1

`swr~c~1`
Covers:
- sys~a~1
");
        assert_eq!(
            messages(&trace),
            [
                "x.md:3: swr~b~1: not covered by required type 'dsn'",
                "x.md:7: swr~c~1: not covered by required type 'dsn'",
            ]
        );
        let a = item(&trace, "sys~a~1");
        assert!(a.covered);
        assert!(!a.deep_covered);
        assert_eq!(a.missing, []);
    }

    #[test]
    fn reports_missing_types_separately() {
        let trace = run("`dsn~a~1`\nNeeds: impl, utest, vrev\n\n`utest~b~0`\nCovers:\n- dsn~a~1\n");
        assert_eq!(
            messages(&trace),
            [
                "x.md:1: dsn~a~1: not covered by required type 'impl'",
                "x.md:1: dsn~a~1: not covered by required type 'vrev'",
            ]
        );
    }

    #[test]
    fn reports_invalid_revisions_and_targets() {
        let trace = run("\
`sys~a~2`
Needs: swr

`swr~r~1`
Status: rejected

`swr~b~1`
Needs: dsn
Covers:
- sys~a~1
- sys~a~3
- sys~missing~1
Depends:
- swr~other~1
- swr~r~1
- sys~a~1
");
        assert_eq!(
            messages(&trace),
            [
                "x.md:1: sys~a~2: not covered by required type 'swr'",
                "x.md:7: swr~b~1: not covered by required type 'dsn'",
                "x.md:10: swr~b~1: covers outdated revision sys~a~1 (current revision: 2)",
                "x.md:11: swr~b~1: covers nonexistent revision sys~a~3 (current revision: 2)",
                "x.md:12: swr~b~1: covers unknown item sys~missing~1",
                "x.md:14: swr~b~1: depends on unknown item swr~other~1",
                "x.md:15: swr~b~1: depends on rejected item swr~r~1",
                "x.md:16: swr~b~1: depends on outdated revision sys~a~1 (current revision: 2)",
            ]
        );
        assert_eq!(
            statuses(&trace),
            [
                ("sys~a~1".to_owned(), LinkStatus::Outdated { current: 2 }),
                ("sys~a~3".to_owned(), LinkStatus::Predated { current: 2 }),
                ("sys~missing~1".to_owned(), LinkStatus::Unknown),
                ("swr~other~1".to_owned(), LinkStatus::Unknown),
                ("swr~r~1".to_owned(), LinkStatus::Rejected),
                ("sys~a~1".to_owned(), LinkStatus::Outdated { current: 2 }),
            ]
        );
    }

    #[test]
    fn reports_links_against_the_levels() {
        let trace = run("\
`sys~a~1`

`dsn~c~1`
Covers:
- sys~a~1

`impl~d~0`
Covers:
- dsn~c~1
- sys~a~1
");
        assert_eq!(
            messages(&trace),
            [
                "x.md:1: sys~a~1: not covered by required type 'swr'",
                "x.md:5: dsn~c~1: covers sys~a~1, which does not need type 'dsn'",
                "x.md:10: impl~d~0: covers sys~a~1, which does not need type 'impl'",
            ]
        );
        assert_eq!(trace.links[0].status, LinkStatus::Unwanted);
        assert_eq!(trace.links[1].status, LinkStatus::Valid);
    }

    #[test]
    fn item_needs_override_level_needs() {
        let trace = run("\
`sys~a~1`
Needs: dsn

`dsn~c~1`
Covers:
- sys~a~1

`impl~d~0`
Covers:
- dsn~c~1
");
        assert_eq!(messages(&trace), Vec::<String>::new());
        assert_eq!(item(&trace, "sys~a~1").needs[0].as_str(), "dsn");
    }

    #[test]
    fn known_types_include_verification_and_item_needs() {
        let trace = run("\
`dsn~a~1`
Needs: impl, itest, vrev

`impl~x~0`
Covers:
- dsn~a~1

`itest~y~0`
Covers:
- dsn~a~1

`vrev~z~1`
Covers:
- dsn~a~1

`req~u~1`
");
        // itest is no verification type in CONFIG, only needed by dsn~a~1.
        assert_eq!(
            messages(&trace),
            [
                "x.md:8: itest~y~0: type 'itest' is only known from Needs: of items; \
                 add it to shall.toml if it is no typo",
                "x.md:16: req~u~1: type 'req' is not a level, a verification type or needed by any item",
            ]
        );
        assert!(item(&trace, "dsn~a~1").deep_covered);
    }

    #[test]
    fn duplicates_make_links_ambiguous_and_keep_their_own_links() {
        let trace = run_files(
            &[
                ("a.md", "`sys~a~1`\n\n`swr~b~1`\nCovers:\n- sys~a~1\n"),
                (
                    "b.md",
                    "`swr~b~1`\nCovers:\n- sys~a~1\n- sys~nope~1\n\n`dsn~c~1`\nCovers:\n- swr~b~1\n",
                ),
            ],
            CONFIG,
        );
        assert_eq!(
            messages(&trace),
            [
                "b.md:1: duplicate ID swr~b~1, already defined as swr~b~1 at a.md:3",
                "b.md:4: swr~b~1: covers unknown item sys~nope~1",
                "b.md:6: dsn~c~1: not covered by required type 'impl'",
                "b.md:8: dsn~c~1: covers swr~b~1, whose type and name are defined more than once",
            ]
        );
        let a = item(&trace, "sys~a~1");
        assert_eq!(a.covered_by.len(), 2, "both copies cover sys~a~1");
        assert!(trace.items.iter().filter(|i| i.duplicate).count() == 2);
    }

    #[test]
    fn link_to_revision_of_a_duplicate_is_ambiguous() {
        let trace = run_files(
            &[
                ("a.md", "`sys~a~1`\nNeeds: swr\n"),
                (
                    "b.md",
                    "`sys~a~2`\nNeeds: swr\n\n`swr~c~1`\nNeeds: dsn\nCovers:\n- sys~a~2\n",
                ),
            ],
            CONFIG,
        );
        assert!(
            messages(&trace).contains(
                &"b.md:7: swr~c~1: covers sys~a~2, whose type and name are defined more than once"
                    .to_owned()
            ),
            "{:?}",
            messages(&trace)
        );
    }

    #[test]
    fn rejected_items_need_no_coverage_and_their_links_are_ignored() {
        let trace = run("\
`sys~a~1`
Status: rejected

`swr~b~1`
Status: rejected
Covers:
- sys~gone~1
");
        assert_eq!(messages(&trace), Vec::<String>::new());
        assert!(trace.links.is_empty());
        let a = item(&trace, "sys~a~1");
        assert!(a.rejected && a.covered && a.deep_covered);
        assert_eq!(a.needs, []);
        assert_eq!(a.status, Status::Rejected);
    }

    #[test]
    fn rejection_outranks_revision() {
        let trace =
            run("`sys~a~2`\nStatus: rejected\n\n`swr~b~1`\nNeeds: dsn\nCovers:\n- sys~a~1\n");
        assert_eq!(
            messages(&trace),
            [
                "x.md:4: swr~b~1: not covered by required type 'dsn'",
                "x.md:7: swr~b~1: covers rejected item sys~a~1",
            ]
        );
    }

    #[test]
    fn types_known_only_from_item_needs_get_a_warning() {
        let trace = run("`dsn~a~1`\nNeeds: imlp\n\n`imlp~x~0`\nCovers:\n- dsn~a~1\n");
        assert_eq!(
            messages(&trace),
            [
                "x.md:4: imlp~x~0: type 'imlp' is only known from Needs: of items; \
              add it to shall.toml if it is no typo"
            ]
        );
        assert_eq!(trace.defects[0].severity, crate::Severity::Warning);
        // A rejected item does not make its needs known.
        let trace = run("`dsn~a~1`\nStatus: rejected\nNeeds: imlp\n\n`imlp~x~0`\n");
        assert!(
            messages(&trace)[0].contains("is not a level"),
            "{:?}",
            messages(&trace)
        );
    }

    #[test]
    fn links_serialize_flat() {
        let trace = run("`sys~a~2`\nNeeds: swr\n\n`swr~b~1`\nNeeds: dsn\nCovers:\n- sys~a~1\n");
        insta::assert_yaml_snapshot!(trace.links);
    }

    #[test]
    fn without_levels_only_item_needs_apply() {
        let trace = run_files(
            &[(
                "x.md",
                "`req~a~1`\nNeeds: dsn\n\n`dsn~b~1`\nCovers:\n- req~a~1\n\n`dsn~c~1`\nCovers:\n- dsn~b~1\n",
            )],
            "",
        );
        assert_eq!(
            messages(&trace),
            ["x.md:10: dsn~c~1: covers dsn~b~1, which does not need type 'dsn'"]
        );
    }

    #[test]
    fn reports_each_cycle_once_at_its_first_item() {
        let cycle = "\
`swr~z~1`
Needs: swr
Covers:
- swr~y~1

`swr~y~1`
Needs: swr
Covers:
- swr~z~1
";
        let alone = run_files(&[("a.md", cycle)], CONFIG);
        let expected = ["a.md:1: swr~z~1: coverage cycle swr~z~1 <- swr~y~1 <- swr~z~1"];
        assert_eq!(messages(&alone), expected);
        assert!(
            alone
                .items
                .iter()
                .all(|item| item.covered && !item.deep_covered)
        );

        // An item above the cycle, found first, does not change the report.
        let above = run_files(
            &[
                ("0.md", "`sys~top~1`\n"),
                (
                    "a.md",
                    &cycle.replace("- swr~z~1", "- swr~z~1\n- sys~top~1"),
                ),
            ],
            CONFIG,
        );
        assert_eq!(messages(&above), expected);
        let top = item(&above, "sys~top~1");
        assert!(top.covered && !top.deep_covered);
    }

    #[test]
    fn reports_one_shortest_cycle_per_component() {
        // a <- b <- c <- a with the chord a <- c, and d below the cycle.
        let trace = run("\
`swr~a~1`
Needs: swr
Covers:
- swr~c~1

`swr~b~1`
Needs: swr
Covers:
- swr~a~1

`swr~c~1`
Needs: swr
Covers:
- swr~b~1
- swr~a~1

`swr~d~1`
Needs: swr
Covers:
- swr~c~1
");
        assert_eq!(
            messages(&trace),
            [
                "x.md:1: swr~a~1: coverage cycle swr~a~1 <- swr~c~1 <- swr~a~1",
                "x.md:17: swr~d~1: not covered by required type 'swr'",
            ]
        );
        assert!(trace.items.iter().all(|item| !item.deep_covered));
    }

    #[test]
    fn shortens_long_cycle_messages() {
        let mut text = String::new();
        for i in 0..20 {
            text.push_str(&format!(
                "`swr~n{i:02}~1`\nNeeds: swr\nCovers:\n- swr~n{:02}~1\n\n",
                (i + 1) % 20
            ));
        }
        let trace = run(&text);
        let [message] = &messages(&trace)[..] else {
            panic!("{:?}", messages(&trace))
        };
        assert!(message.contains("... (11 more) <- swr~n00~1"), "{message}");
        assert!(message.len() < 300, "{message}");
    }

    #[test]
    fn tag_defects_name_the_tag() {
        let mut items = markdown::parse(Path::new("x.md"), "`dsn~x~2`\nNeeds: impl\n")
            .unwrap()
            .items;
        let scanned = crate::tags::scan(
            Path::new("a.c"),
            "// [iml->dsn~x~2]\n// [impl->dsn~x~1]\n// [impl->dsn~gone~1]\n// [impl->dsn~x~2]\n",
        )
        .unwrap();
        items.extend(scanned.items);
        let trace = trace(&items, &Config::parse(CONFIG).unwrap());
        assert_eq!(
            messages(&trace),
            [
                "a.c:1: tag [iml->dsn~x~2]: type 'iml' is not a level, a verification type or needed by any item",
                "a.c:1: tag [iml->dsn~x~2]: covers dsn~x~2, which does not need type 'iml'",
                "a.c:2: tag [impl->dsn~x~1]: covers outdated revision dsn~x~1 (current revision: 2)",
                "a.c:3: tag [impl->dsn~gone~1]: unknown target dsn~gone~1",
            ]
        );
    }

    #[test]
    fn reports_self_coverage_as_cycle() {
        let trace = run("`swr~a~1`\nNeeds: swr\nCovers:\n- swr~a~1\n");
        assert_eq!(
            messages(&trace),
            ["x.md:1: swr~a~1: coverage cycle swr~a~1 <- swr~a~1"]
        );
    }

    #[test]
    fn long_chains_do_not_overflow_the_stack() {
        // A tiny stack: a recursive search would overflow long before
        // 20 000 levels, the iterative one does not.
        let count = 20_000;
        let mut items = Vec::new();
        for i in 0..count {
            let mut text = format!("`swr~n{i}~1`\nNeeds: swr\n");
            if i > 0 {
                text.push_str(&format!("Covers:\n- swr~n{}~1\n", i - 1));
            }
            items.extend(markdown::parse(Path::new("x.md"), &text).unwrap().items);
        }
        let trace = std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(move || trace(&items, &Config::default()))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(trace.defects.len(), 1, "only the last item lacks coverage");
        assert!(trace.items.iter().all(|item| !item.deep_covered));
    }
}
