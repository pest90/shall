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
//! `Satisfied-by:` is a `Covers:` link written the other way round: the
//! target covers the item that contains the link. An item with
//! `Satisfied-by:` needs the types of its targets plus its own `Needs:`,
//! but not the `needs` of its level, since a reused item takes the place
//! of the levels below. The target's type must be one of the `targets` of
//! `[satisfied_by]` in `shall.toml`; a `fallback` type is valid with a
//! warning. Without any `[[level]]`, every type is allowed. Only allowed
//! types become needs; without any, the item needs what it would without
//! `Satisfied-by:`, so a link to a disallowed type neither creates a need
//! nor hides one.
//!
//! A `Covers:` link is valid when its target exists once, in exactly the
//! linked revision, is not rejected, and needs the type of the linking
//! item. Every other link is a defect at the line of the link and does
//! not count as coverage; [`LinkStatus`] lists the cases. `Depends:` links
//! are checked the same way, except for the needed type, and never count
//! as coverage; `Satisfied-by:` links are checked against `[satisfied_by]`
//! instead of the needed type.
//!
//! When a [variant](crate::variant) is traced, items outside it are left
//! out. Links to them get the status [`LinkStatus::OutsideVariant`]: no
//! defect for `Covers:`, since a tag or requirement may serve several
//! variants, but an error for `Depends:` and `Satisfied-by:`, whose source
//! needs the target in this variant. A link to a rejected item or a wrong
//! revision outside the variant is rejected, outdated or predated as
//! usual, and a duplicate ID stays a duplicate.
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
//! known only from the `Needs:` or `Satisfied-by:` of items gets a
//! warning, since it is often a typo. Without any `[[level]]`, types are
//! not checked.

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
    /// Every `Covers:`, `Depends:` and `Satisfied-by:` link of items that
    /// are not rejected, ordered by location.
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
    /// Types the item needs, from `Needs:` and `Satisfied-by:` or from its
    /// level; empty for rejected items.
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
    /// Whether the item's type is unknown, which is an error: `shall.toml`
    /// has levels, but the type is no level, no type needed by a level, no
    /// verification type and not needed by any item. Always false for
    /// rejected items.
    pub unknown_type: bool,
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
    /// `Covers:`, `Depends:` or `Satisfied-by:`.
    pub kind: LinkKind,
    /// The item that contains the link.
    pub source: ItemId,
    /// Where the source item is defined, which tells copies of a
    /// duplicate ID apart.
    pub source_location: Location,
    /// The target as written, including the linked revision.
    pub target: ItemId,
    /// Where the target is defined, if exactly one item has its type and
    /// name, whatever its revision or status; none for
    /// [`LinkStatus::Unknown`] and [`LinkStatus::Ambiguous`], and for links
    /// to items outside the traced variant, whatever their status.
    pub target_location: Option<Location>,
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
    /// `Satisfied-by:`: the target covers the source, e.g. a platform
    /// capability meets a customer requirement.
    SatisfiedBy,
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
    /// The target's type is neither a target nor a fallback of
    /// `[satisfied_by]` (`Satisfied-by:` only).
    Disallowed,
    /// The target exists, but outside the traced variant; a defect except
    /// for `Covers:`.
    OutsideVariant,
}

/// Traces `items` against the levels of `config`.
///
/// Diagnostics of the parsers are not part of the result; duplicate IDs
/// are, because they make links ambiguous.
pub fn trace(items: &[Item], config: &Config) -> Trace {
    trace_variant(items, &[], config)
}

/// Traces the `inside` items of a variant; links to `outside` items get
/// [`LinkStatus::OutsideVariant`], see [`crate::variant::select`].
pub fn trace_variant(inside: &[Item], outside: &[Item], config: &Config) -> Trace {
    // IDs are global: a duplicate is a defect in every variant.
    let mut defects = check::duplicate_ids(inside.iter().chain(outside));
    let outside_items = outside;
    // Revision and rejection of the items outside, by type and name.
    let mut outside: HashMap<(&str, &str), Vec<(u32, bool)>> = HashMap::new();
    for item in outside_items {
        outside
            .entry((item.id.typ().as_str(), item.id.name()))
            .or_default()
            .push((item.id.revision(), item.status == Status::Rejected));
    }
    let mut items: Vec<&Item> = inside.iter().collect();
    items.sort_by(|a, b| a.location.cmp(&b.location));

    let mut copies: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (position, item) in items.iter().enumerate() {
        copies
            .entry((item.id.typ().as_str(), item.id.name()))
            .or_default()
            .push(position);
    }
    let duplicate: Vec<bool> = items
        .iter()
        .map(|item| {
            let key = (item.id.typ().as_str(), item.id.name());
            copies[&key].len() + outside.get(&key).map_or(0, Vec::len) > 1
        })
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

    let unknown_type: Vec<bool> = items
        .iter()
        .zip(&rejected)
        .map(|(item, &rejected)| {
            let typ = item.id.typ();
            !rejected
                && !config.levels.is_empty()
                && !configured.contains(typ)
                && !needed_by_items.contains(typ)
        })
        .collect();

    // covered_by[target] = sources with a valid link to the target.
    let mut covered_by: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
    let mut links = Vec::new();
    for (source, item) in items.iter().enumerate() {
        if rejected[source] {
            continue;
        }
        let typ = item.id.typ();
        let subject = subject(item);
        if unknown_type[source] {
            defects.push(Diagnostic::error(
                item.location.clone(),
                format!(
                    "{subject}: type '{typ}' is not a level, a verification type or needed by any item"
                ),
            ));
        } else if !config.levels.is_empty() && !configured.contains(typ) {
            defects.push(Diagnostic::warning(
                item.location.clone(),
                format!(
                    "{subject}: type '{typ}' is only known from Needs: or Satisfied-by: of items; \
                         add it to shall.toml if it is no typo"
                ),
            ));
        }
        let all_links = item
            .covers
            .iter()
            .map(|link| (LinkKind::Covers, link))
            .chain(item.depends.iter().map(|link| (LinkKind::Depends, link)))
            .chain(
                item.satisfied_by
                    .iter()
                    .map(|link| (LinkKind::SatisfiedBy, link)),
            );
        for (kind, link) in all_links {
            let resolved = resolve(&copies, &outside, &items, &rejected, link);
            let status = match (kind, resolved) {
                (LinkKind::Covers, Ok(target)) => {
                    if needs[target].contains(item.id.typ()) {
                        covered_by[target].push(source);
                        LinkStatus::Valid
                    } else {
                        LinkStatus::Unwanted
                    }
                }
                (LinkKind::SatisfiedBy, Ok(target)) => {
                    let allowed = &config.satisfied_by;
                    let target_type = link.id.typ();
                    if config.levels.is_empty() || allowed.targets.contains(target_type) {
                        covered_by[source].push(target);
                        LinkStatus::Valid
                    } else if allowed.fallback.contains(target_type) {
                        covered_by[source].push(target);
                        let prefer = if allowed.targets.is_empty() {
                            String::new()
                        } else {
                            format!("; prefer {}", type_list(&allowed.targets))
                        };
                        defects.push(Diagnostic::warning(
                            item.location.with_line(link.line),
                            format!(
                                "{subject}: is satisfied by {}, whose type '{target_type}' \
                                 is only a fallback{prefer}",
                                link.id,
                            ),
                        ));
                        LinkStatus::Valid
                    } else {
                        LinkStatus::Disallowed
                    }
                }
                (LinkKind::Depends, Ok(_)) => LinkStatus::Valid,
                (_, Err(status)) => status,
            };
            if let Some(mut message) = link_message(kind, item, link, status, config) {
                if let (LinkStatus::Unwanted, Ok(target)) = (status, resolved)
                    && needed_without_satisfied_by(items[target], item.id.typ(), config)
                {
                    message.push_str(&format!(
                        "; Satisfied-by: replaces the needs of its level, \
                         add 'Needs: {}' to it to refine it in the project",
                        item.id.typ()
                    ));
                }
                defects.push(Diagnostic::error(
                    item.location.with_line(link.line),
                    message,
                ));
            }
            let key = (link.id.typ().as_str(), link.id.name());
            let target_location = match copies.get(&key).map(Vec::as_slice) {
                Some([target]) if !outside.contains_key(&key) => {
                    Some(items[*target].location.clone())
                }
                _ => None,
            };
            links.push(Link {
                kind,
                source: item.id.clone(),
                source_location: item.location.clone(),
                target: link.id.clone(),
                target_location,
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
                        format!("{}: not covered by required type '{typ}'", subject(item)),
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
                unknown_type: unknown_type[position],
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

/// How defects name an item: by its ID, or by the tag as written, since
/// the ID of a tag's item is generated.
fn subject(item: &Item) -> String {
    match &item.origin {
        Origin::Tag { text, .. } => format!("tag {text}"),
        Origin::Requirement => item.id.to_string(),
    }
}

/// Types the item needs: its own `Needs:` and the types of its
/// `Satisfied-by:` targets, or else those of its level.
///
/// Only allowed target types count, so that a link of a wrong type cannot
/// turn that type into a need; if no target has an allowed type, the item
/// needs what it would without `Satisfied-by:`.
fn effective_needs(item: &Item, config: &Config) -> Vec<ItemType> {
    let mut needs = item.needs.clone();
    for link in &item.satisfied_by {
        let typ = link.id.typ();
        if satisfied_by_allows(config, typ) && !needs.contains(typ) {
            needs.push(typ.clone());
        }
    }
    if !needs.is_empty() {
        return needs;
    }
    config
        .level(item.id.typ())
        .map(|level| level.needs.clone())
        .unwrap_or_default()
}

/// Whether `Satisfied-by:` may name items of the type: a target or a
/// fallback of `[satisfied_by]`, or any type without levels.
fn satisfied_by_allows(config: &Config, typ: &ItemType) -> bool {
    let allowed = &config.satisfied_by;
    config.levels.is_empty() || allowed.targets.contains(typ) || allowed.fallback.contains(typ)
}

/// Whether a `Covers:` link of type `typ` is unwanted only because the
/// target's `Satisfied-by:` replaces the needs of its level.
fn needed_without_satisfied_by(target: &Item, typ: &ItemType, config: &Config) -> bool {
    !target.satisfied_by.is_empty()
        && target.needs.is_empty()
        && config
            .level(target.id.typ())
            .is_some_and(|level| level.needs.contains(typ))
}

/// Finds the single target of a link in the linked revision.
fn resolve(
    copies: &HashMap<(&str, &str), Vec<usize>>,
    outside: &HashMap<(&str, &str), Vec<(u32, bool)>>,
    items: &[&Item],
    rejected: &[bool],
    link: &Reference,
) -> Result<usize, LinkStatus> {
    let key = (link.id.typ().as_str(), link.id.name());
    let inside = copies.get(&key).map_or(&[][..], Vec::as_slice);
    let outside = outside.get(&key).map_or(&[][..], Vec::as_slice);
    let linked = link.id.revision();
    let target = match (inside, outside) {
        ([], []) => return Err(LinkStatus::Unknown),
        ([target], []) => *target,
        // A rejection or a wrong revision is the root cause, whichever
        // variant the target belongs to.
        ([], [(_, true)]) => return Err(LinkStatus::Rejected),
        ([], [(current, false)]) => {
            return Err(match linked.cmp(current) {
                std::cmp::Ordering::Less => LinkStatus::Outdated { current: *current },
                std::cmp::Ordering::Greater => LinkStatus::Predated { current: *current },
                std::cmp::Ordering::Equal => LinkStatus::OutsideVariant,
            });
        }
        _ => return Err(LinkStatus::Ambiguous),
    };
    // A rejected target is the root cause even if the revision differs
    // too: updating the revision would not make the link valid.
    if rejected[target] {
        return Err(LinkStatus::Rejected);
    }
    let current = items[target].id.revision();
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
    config: &Config,
) -> Option<String> {
    let verb = match kind {
        LinkKind::Covers => "covers",
        LinkKind::Depends => "depends on",
        LinkKind::SatisfiedBy => "is satisfied by",
    };
    let target = &link.id;
    // The ID of a tag's item is generated, so its defects name the tag as
    // written; a tag with an unknown target is orphaned code.
    let is_tag = matches!(item.origin, Origin::Tag { .. });
    let subject = subject(item);
    let problem = match status {
        LinkStatus::Valid => return None,
        LinkStatus::OutsideVariant if kind == LinkKind::Covers => return None,
        LinkStatus::OutsideVariant => format!("{verb} {target}, which is not part of the variant"),
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
        LinkStatus::Disallowed => format!(
            "{verb} {target}, but type '{}' is no target of [satisfied_by]; allowed: {}",
            target.typ(),
            type_list(
                config
                    .satisfied_by
                    .targets
                    .iter()
                    .chain(&config.satisfied_by.fallback)
            )
        ),
    };
    Some(format!("{subject}: {problem}"))
}

/// Lists types for a message, e.g. `'cap', 'swr'`.
fn type_list<'a>(types: impl IntoIterator<Item = &'a ItemType>) -> String {
    let types: Vec<String> = types.into_iter().map(|typ| format!("'{typ}'")).collect();
    if types.is_empty() {
        "none".to_owned()
    } else {
        types.join(", ")
    }
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
    fn forwarding_skips_a_level() {
        let trace = run("\
`sys~a~1`

swr --> dsn : sys~a~1

`dsn~c~1`
Covers:
- swr~a~1

`impl~d~0`
Covers:
- dsn~c~1

`sys~b~1`

dsn --> impl : sys~b~1
");
        assert_eq!(
            messages(&trace),
            [
                "x.md:13: sys~b~1: not covered by required type 'swr'",
                "x.md:15: dsn~b~1: covers sys~b~1, which does not need type 'dsn'",
                "x.md:15: dsn~b~1: not covered by required type 'impl'",
            ]
        );
        assert!(item(&trace, "sys~a~1").deep_covered);
        assert!(item(&trace, "swr~a~1").deep_covered);
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
                "x.md:8: itest~y~0: type 'itest' is only known from Needs: or Satisfied-by: of items; \
                 add it to shall.toml if it is no typo",
                "x.md:16: req~u~1: type 'req' is not a level, a verification type or needed by any item",
            ]
        );
        assert!(item(&trace, "dsn~a~1").deep_covered);
        // Only the error, not the warning, is an unknown type.
        assert!(item(&trace, "req~u~1").unknown_type);
        assert_eq!(
            trace.items.iter().filter(|item| item.unknown_type).count(),
            1
        );
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
                "x.md:4: imlp~x~0: type 'imlp' is only known from Needs: or Satisfied-by: of items; \
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
            "// [iml->dsn~x~2]\n// [impl->dsn~x~1]\n// [impl->dsn~gone~1]\n// [impl->dsn~x~2]\n// [impl->dsn~x~2>>utest]\n",
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
                "a.c:5: tag [impl->dsn~x~2>>utest]: not covered by required type 'utest'",
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

    const PLATFORM: &str = r#"
[[level]]
type = "hlr"
name = "Customer"
needs = ["swr"]

[[level]]
type = "cap"
name = "Capabilities"
needs = ["impl"]

[[level]]
type = "swr"
name = "Software"
needs = ["impl"]

[satisfied_by]
fallback = ["swr"]
"#;

    fn run_platform(text: &str) -> Trace {
        run_files(&[("x.md", text)], PLATFORM)
    }

    #[test]
    fn satisfied_by_covers_the_source() {
        let trace = run_platform(
            "\
`hlr~a~1`
Satisfied-by:
- cap~c~1

`cap~c~1`

`impl~i~0`
Covers:
- cap~c~1
",
        );
        assert_eq!(messages(&trace), Vec::<String>::new());
        let a = item(&trace, "hlr~a~1");
        // The level's `swr` is not needed: the capability takes its place.
        assert_eq!(a.needs, ["cap".parse::<ItemType>().unwrap()]);
        assert_eq!(a.covered_by[0].id.to_string(), "cap~c~1");
        assert!(a.covered && a.deep_covered);
        assert_eq!(
            item(&trace, "cap~c~1").covered_by[0].id.to_string(),
            "impl~i~0"
        );
        let link = &trace.links[0];
        assert_eq!(link.kind, LinkKind::SatisfiedBy);
        assert_eq!(link.source.to_string(), "hlr~a~1");
        assert_eq!(link.status, LinkStatus::Valid);
        assert_eq!(link.location.to_string(), "x.md:3");
        assert_eq!(link.target_location.as_ref().unwrap().to_string(), "x.md:5");
        let json = serde_json::to_value(link).unwrap();
        assert_eq!(json["kind"], "satisfied_by");
    }

    #[test]
    fn satisfied_by_adds_to_own_needs() {
        let trace = run_platform(
            "\
`hlr~a~1`
Needs: swr
Satisfied-by:
- cap~c~1

`cap~c~1`
Needs: impl

`impl~i~0`
Covers:
- cap~c~1
",
        );
        assert_eq!(
            messages(&trace),
            ["x.md:1: hlr~a~1: not covered by required type 'swr'"]
        );
        let a = item(&trace, "hlr~a~1");
        assert_eq!(a.needs.len(), 2);
        assert_eq!(a.missing[0].as_str(), "swr");
    }

    #[test]
    fn satisfied_by_checks_the_target_type() {
        let trace = run_platform(
            "\
`hlr~a~1`
Satisfied-by:
- swr~s~1

`hlr~b~1`
Satisfied-by:
- hlr~a~1

`swr~s~1`

`impl~i~0`
Covers:
- swr~s~1
",
        );
        assert_eq!(
            messages(&trace),
            [
                "x.md:3: hlr~a~1: is satisfied by swr~s~1, whose type 'swr' is only a \
                 fallback; prefer 'cap'",
                // A disallowed type is no need; the level's need applies.
                "x.md:5: hlr~b~1: not covered by required type 'swr'",
                "x.md:7: hlr~b~1: is satisfied by hlr~a~1, but type 'hlr' is no target of \
                 [satisfied_by]; allowed: 'cap', 'swr'",
            ]
        );
        assert_eq!(
            trace.defects[0].severity,
            crate::diagnostic::Severity::Warning
        );
        assert!(item(&trace, "hlr~a~1").deep_covered);
        assert_eq!(
            statuses(&trace),
            [
                ("swr~s~1".to_owned(), LinkStatus::Valid),
                ("hlr~a~1".to_owned(), LinkStatus::Disallowed),
                ("swr~s~1".to_owned(), LinkStatus::Valid),
            ]
        );
    }

    #[test]
    fn satisfied_by_reports_invalid_targets() {
        let trace = run_platform(
            "\
`hlr~a~1`
Satisfied-by:
- cap~c~1
- cap~gone~1
- cap~old~1

`cap~c~2`
Needs: impl

`cap~old~1`
Status: rejected
",
        );
        assert_eq!(
            messages(&trace),
            [
                "x.md:1: hlr~a~1: not covered by required type 'cap'",
                "x.md:3: hlr~a~1: is satisfied by outdated revision cap~c~1 (current revision: 2)",
                "x.md:4: hlr~a~1: is satisfied by unknown item cap~gone~1",
                "x.md:5: hlr~a~1: is satisfied by rejected item cap~old~1",
                "x.md:7: cap~c~2: not covered by required type 'impl'",
            ]
        );
    }

    #[test]
    fn satisfied_by_needs_the_target_deep_covered() {
        let trace = run_platform("`hlr~a~1`\nSatisfied-by:\n- cap~c~1\n\n`cap~c~1`\n");
        // Only the root cause, the uncovered capability, is a defect.
        assert_eq!(
            messages(&trace),
            ["x.md:5: cap~c~1: not covered by required type 'impl'"]
        );
        let a = item(&trace, "hlr~a~1");
        assert!(a.covered);
        assert!(!a.deep_covered);
    }

    #[test]
    fn satisfied_by_can_close_a_cycle() {
        let trace = run_platform(
            "`cap~a~1`\nSatisfied-by:\n- cap~b~1\n\n`cap~b~1`\nSatisfied-by:\n- cap~a~1\n",
        );
        assert_eq!(
            messages(&trace),
            ["x.md:1: cap~a~1: coverage cycle cap~a~1 <- cap~b~1 <- cap~a~1"]
        );
        assert!(
            trace
                .items
                .iter()
                .all(|item| item.covered && !item.deep_covered)
        );
    }

    #[test]
    fn satisfied_by_of_rejected_items_is_ignored() {
        let trace = run_platform("`hlr~a~1`\nStatus: rejected\nSatisfied-by:\n- cap~gone~1\n");
        assert_eq!(messages(&trace), Vec::<String>::new());
        assert!(trace.links.is_empty());
    }

    #[test]
    fn without_levels_satisfied_by_accepts_every_type() {
        let trace = run_files(
            &[("x.md", "`req~a~1`\nSatisfied-by:\n- dsn~b~1\n\n`dsn~b~1`\n")],
            "",
        );
        assert_eq!(messages(&trace), Vec::<String>::new());
        assert!(item(&trace, "req~a~1").deep_covered);
    }

    #[test]
    fn satisfied_by_target_types_are_known_from_items() {
        let trace = run("`sys~a~1`\nSatisfied-by:\n- cap~c~1\n\n`cap~c~1`\n");
        assert_eq!(
            messages(&trace),
            [
                "x.md:5: cap~c~1: type 'cap' is only known from Needs: or Satisfied-by: of \
                 items; add it to shall.toml if it is no typo"
            ]
        );
    }

    #[test]
    fn disallowed_and_unknown_types_are_no_needs() {
        let trace = run_platform(
            "\
`hlr~a~1`
Satisfied-by:
- hlr~b~1
- cpa~typo~1

`hlr~b~1`
Needs: cap

`hlr~z~1`
Needs: cap
Covers:
- hlr~a~1
",
        );
        assert_eq!(
            messages(&trace),
            [
                // The level's need still applies.
                "x.md:1: hlr~a~1: not covered by required type 'swr'",
                "x.md:3: hlr~a~1: is satisfied by hlr~b~1, but type 'hlr' is no target of \
                 [satisfied_by]; allowed: 'cap', 'swr'",
                "x.md:4: hlr~a~1: is satisfied by unknown item cpa~typo~1",
                "x.md:6: hlr~b~1: not covered by required type 'cap'",
                "x.md:9: hlr~z~1: not covered by required type 'cap'",
                "x.md:12: hlr~z~1: covers hlr~a~1, which does not need type 'hlr'",
            ]
        );
        assert_eq!(item(&trace, "hlr~a~1").needs[0].as_str(), "swr");
    }

    #[test]
    fn covers_into_a_satisfied_item_needs_its_own_needs() {
        let trace = run_platform(
            "\
`hlr~a~1`
Satisfied-by:
- cap~c~1

`cap~c~1`
Needs: impl

`swr~s~1`
Needs: impl
Covers:
- hlr~a~1

`impl~i~0`
Covers:
- cap~c~1
- swr~s~1
",
        );
        assert_eq!(
            messages(&trace),
            [
                "x.md:11: swr~s~1: covers hlr~a~1, which does not need type 'swr'; \
              Satisfied-by: replaces the needs of its level, add 'Needs: swr' to it to \
              refine it in the project"
            ]
        );
    }

    #[test]
    fn fallback_without_targets_prefers_nothing() {
        let config = format!(
            "{}\n[satisfied_by]\ntargets = []\nfallback = [\"swr\"]\n",
            PLATFORM.replace("[satisfied_by]\nfallback = [\"swr\"]\n", "")
        );
        let trace = run_files(
            &[(
                "x.md",
                "`hlr~a~1`\nSatisfied-by:\n- swr~s~1\n\n`swr~s~1`\nNeeds: impl\n\n`impl~i~0`\nCovers:\n- swr~s~1\n",
            )],
            &config,
        );
        assert_eq!(
            messages(&trace),
            ["x.md:3: hlr~a~1: is satisfied by swr~s~1, whose type 'swr' is only a fallback"]
        );
    }

    #[test]
    fn own_needs_get_no_satisfied_by_hint() {
        let trace = run_platform(
            "\
`hlr~a~1`
Needs: impl
Satisfied-by:
- cap~c~1

`swr~s~1`
Covers:
- hlr~a~1
",
        );
        let unwanted: Vec<String> = messages(&trace)
            .into_iter()
            .filter(|message| message.contains("does not need"))
            .collect();
        assert_eq!(
            unwanted,
            ["x.md:8: swr~s~1: covers hlr~a~1, which does not need type 'swr'"]
        );
    }

    #[test]
    fn links_out_of_the_variant() {
        let text = "\
`hlr~a~1`
Satisfied-by:
- cap~in~1
- cap~out~1

`cap~in~1`
Needs: impl

`cap~out~1`
Features: x

`swr~s~1`
Needs: impl
Features: y
Covers:
- cap~out~1
Depends:
- cap~out~1

`impl~i~0`
Covers:
- cap~in~1
- swr~s~1
";
        let items = markdown::parse(Path::new("x.md"), text).unwrap().items;
        let variant = crate::config::Variant::new(vec!["y".to_owned()]);
        // swr~s~1 is inside by its own feature, although it covers only an
        // item outside.
        let selection = crate::variant::select(&items, &variant);
        let trace = trace_variant(
            &selection.inside,
            &selection.outside,
            &Config::parse(PLATFORM).unwrap(),
        );
        assert_eq!(
            messages(&trace),
            [
                "x.md:4: hlr~a~1: is satisfied by cap~out~1, which is not part of the variant",
                "x.md:18: swr~s~1: depends on cap~out~1, which is not part of the variant",
            ]
        );
        assert!(trace.items.iter().all(|item| item.id.name() != "out"));
        let out: Vec<&Link> = trace
            .links
            .iter()
            .filter(|link| link.status == LinkStatus::OutsideVariant)
            .collect();
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(out.iter().all(|link| link.target_location.is_none()));
        // The covering link to the part inside still counts.
        assert!(item(&trace, "hlr~a~1").covered);
    }

    #[test]
    fn duplicates_and_revisions_across_the_variant() {
        let text = "\
`cap~dup~1`

`cap~dup~1`
Features: x

`cap~old~2`
Features: x

`swr~s~1`
Needs: impl
Features: y
Covers:
- cap~dup~1
- cap~old~1
Depends:
- cap~old~3
- cap~gone~1

`cap~gone~1`
Status: rejected
Features: x
";
        let items = markdown::parse(Path::new("x.md"), text).unwrap().items;
        let variant = crate::config::Variant::new(vec!["y".to_owned()]);
        let selection = crate::variant::select(&items, &variant);
        let trace = trace_variant(
            &selection.inside,
            &selection.outside,
            &Config::parse(PLATFORM).unwrap(),
        );
        assert_eq!(
            messages(&trace),
            [
                "x.md:3: duplicate ID cap~dup~1, already defined as cap~dup~1 at x.md:1",
                "x.md:9: swr~s~1: not covered by required type 'impl'",
                "x.md:13: swr~s~1: covers cap~dup~1, whose type and name are defined more than once",
                "x.md:14: swr~s~1: covers outdated revision cap~old~1 (current revision: 2)",
                "x.md:16: swr~s~1: depends on nonexistent revision cap~old~3 (current revision: 2)",
                "x.md:17: swr~s~1: depends on rejected item cap~gone~1",
            ]
        );
        assert!(item(&trace, "cap~dup~1").duplicate);
        assert!(trace.links[0].target_location.is_none());
    }
}
