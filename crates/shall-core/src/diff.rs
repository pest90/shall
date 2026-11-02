//! Changes of the requirements between two states of a project, e.g. the
//! target branch of a merge request and its head, for reviewers.
//!
//! Requirements are matched by type and name, since a new revision keeps
//! both: a requirement is *added* or *removed* when only one state has
//! its type and name, *revised* when both have it with another revision,
//! and *edited* when both have the same ID but other content, which may
//! deserve a new revision. A link of the head is *broken* when it is not
//! valid there but was valid, or missing, in the base, e.g. a link to the
//! old revision of a revised requirement; links match by what they
//! connect, wherever they are written, and a link that was invalid for
//! the same reason before is not broken by the change.

use std::collections::{HashMap, HashSet};
use std::mem::{Discriminant, discriminant};

use crate::id::ItemId;
use crate::item::{Item, Origin};
use crate::trace::{Link, LinkKind, LinkStatus, Trace};

/// What changed from a base to a head state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changes<'a> {
    /// Requirements of the head whose type and name the base does not
    /// have, in the order of their locations.
    pub added: Vec<&'a Item>,
    /// Requirements of the base whose type and name the head does not
    /// have, in the order of their locations in the base.
    pub removed: Vec<&'a Item>,
    /// Requirements with another revision in the head: the base's and the
    /// head's, in the order of the head's locations.
    pub revised: Vec<(&'a Item, &'a Item)>,
    /// Requirements with the same ID and other content: the base's and the
    /// head's, in the order of the head's locations.
    pub edited: Vec<(&'a Item, &'a Item)>,
    /// Links of the head that are not valid, but were valid or did not
    /// exist in the base, in the order of the head's links.
    pub broken: Vec<&'a Link>,
}

impl Changes<'_> {
    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.revised.is_empty()
            && self.edited.is_empty()
            && self.broken.is_empty()
    }
}

/// Compares the requirements and links of `base` and `head`, each the
/// items of a state and their trace.
pub fn changes<'a>(
    (base_items, base): (&'a [Item], &'a Trace),
    (head_items, head): (&'a [Item], &'a Trace),
) -> Changes<'a> {
    let base_requirements = requirements(base_items);
    let head_requirements = requirements(head_items);
    let by_name = |items: &[&'a Item]| -> HashMap<(String, String), &'a Item> {
        let mut map = HashMap::new();
        for &item in items {
            map.entry(name(&item.id)).or_insert(item);
        }
        map
    };
    let base_names = by_name(&base_requirements);
    let head_names = by_name(&head_requirements);
    let mut changes = Changes {
        added: Vec::new(),
        removed: Vec::new(),
        revised: Vec::new(),
        edited: Vec::new(),
        broken: Vec::new(),
    };
    // A duplicate is no other requirement than its first copy.
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for &item in &head_requirements {
        if !seen.insert(name(&item.id)) {
            continue;
        }
        match base_names.get(&name(&item.id)) {
            None => changes.added.push(item),
            Some(&old) if old.id.revision() != item.id.revision() => {
                changes.revised.push((old, item));
            }
            Some(&old) if content(old) != content(item) => changes.edited.push((old, item)),
            Some(_) => {}
        }
    }
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for &item in &base_requirements {
        if seen.insert(name(&item.id)) && !head_names.contains_key(&name(&item.id)) {
            changes.removed.push(item);
        }
    }
    // The base's links by what they connect, as a multiset of statuses.
    let mut base_links: HashMap<LinkKey, Vec<Discriminant<LinkStatus>>> = HashMap::new();
    for link in &base.links {
        base_links
            .entry(key(link))
            .or_default()
            .push(discriminant(&link.status));
    }
    for link in &head.links {
        let invalid = match link.status {
            LinkStatus::Valid => false,
            // Not a defect, see `crate::variant`.
            LinkStatus::OutsideVariant => link.kind != LinkKind::Covers,
            _ => true,
        };
        if !invalid {
            continue;
        }
        // Broken in the same way before, whatever the current revision.
        let before = base_links.get_mut(&key(link)).and_then(|statuses| {
            let position = statuses
                .iter()
                .position(|status| *status == discriminant(&link.status))?;
            Some(statuses.swap_remove(position))
        });
        if before.is_none() {
            changes.broken.push(link);
        }
    }
    changes
}

/// The requirements among `items`, by location.
fn requirements(items: &[Item]) -> Vec<&Item> {
    let mut requirements: Vec<&Item> = items
        .iter()
        .filter(|item| item.origin == Origin::Requirement)
        .collect();
    requirements.sort_by(|a, b| a.location.cmp(&b.location));
    requirements
}

/// Type and name of an ID, which a new revision keeps.
fn name(id: &ItemId) -> (String, String) {
    (id.typ().to_string(), id.name().to_owned())
}

/// A link by what it connects, whatever the files and lines: the kind,
/// the source's type and name, and the target as written. The name of a
/// tag's item is generated from its place, so a tag counts by its type.
type LinkKey = (LinkKind, (String, String), String);

fn key(link: &Link) -> LinkKey {
    let source = if link.source.revision() == 0 {
        (link.source.typ().to_string(), String::new())
    } else {
        name(&link.source)
    };
    (link.kind, source, link.target.to_string())
}

/// What a reader of an item sees, without where it is.
fn content(item: &Item) -> impl PartialEq + '_ {
    let ids = |references: &'_ [crate::Reference]| -> Vec<String> {
        references.iter().map(|link| link.id.to_string()).collect()
    };
    (
        (
            &item.title,
            &item.description,
            &item.rationale,
            &item.comment,
            item.status,
            &item.needs,
        ),
        (
            ids(&item.covers),
            ids(&item.depends),
            ids(&item.satisfied_by),
            item.params
                .iter()
                .map(|param| (&param.name, &param.value))
                .collect::<Vec<_>>(),
            &item.tags,
            &item.features,
            &item.source,
        ),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::config::Config;
    use crate::{markdown, trace};

    const CONFIG: &str = "[[level]]\ntype = \"swr\"\nname = \"S\"\nneeds = [\"dsn\"]\n\n\
                          [[level]]\ntype = \"dsn\"\nname = \"D\"\n";

    fn state(text: &str) -> (Vec<Item>, Trace) {
        let items = markdown::parse(Path::new("r.md"), text).unwrap().items;
        let trace = trace::trace(&items, &Config::parse(CONFIG).unwrap());
        (items, trace)
    }

    #[test]
    fn lists_what_changed() {
        let base = state(
            "# Kept\n`swr~kept~1`\n\n# Revised\n`swr~rev~1`\n\n# Edited\n`swr~edit~1`\nOld.\n\n\
             # Gone\n`swr~gone~1`\n\n# D\n`dsn~d~1`\nCovers:\n- swr~rev~1\n- swr~kept~1\n\
             - swr~missing~1\n",
        );
        let head = state(
            "# New\n`swr~new~1`\n\n# Kept\n`swr~kept~1`\n\n# Revised\n`swr~rev~2`\n\n\
             # Edited\n`swr~edit~1`\nNew.\n\n# D\n`dsn~d~1`\nCovers:\n- swr~rev~1\n\
             - swr~kept~1\n- swr~missing~1\n- swr~gone~1\n",
        );
        let found = changes((&base.0, &base.1), (&head.0, &head.1));
        let ids = |items: &[&Item]| -> Vec<String> {
            items.iter().map(|item| item.id.to_string()).collect()
        };
        assert_eq!(ids(&found.added), ["swr~new~1"]);
        assert_eq!(ids(&found.removed), ["swr~gone~1"]);
        let pairs = |pairs: &[(&Item, &Item)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(old, new)| (old.id.to_string(), new.id.to_string()))
                .collect()
        };
        assert_eq!(
            pairs(&found.revised),
            [("swr~rev~1".to_owned(), "swr~rev~2".to_owned())]
        );
        assert_eq!(
            pairs(&found.edited),
            [
                ("swr~edit~1".to_owned(), "swr~edit~1".to_owned()),
                ("dsn~d~1".to_owned(), "dsn~d~1".to_owned())
            ]
        );
        // The link to the old revision and the new link to the removed item
        // are broken; the one that was unknown before stays as it was.
        let broken: Vec<(String, LinkStatus)> = found
            .broken
            .iter()
            .map(|link| (link.target.to_string(), link.status))
            .collect();
        assert_eq!(
            broken,
            [
                ("swr~rev~1".to_owned(), LinkStatus::Outdated { current: 2 }),
                ("swr~gone~1".to_owned(), LinkStatus::Unknown),
            ]
        );
        assert!(!found.is_empty());
        // Moved to another file and duplicated: the duplicate is no added
        // requirement, but makes a link ambiguous; the links that were
        // broken before stay unreported.
        let moved = {
            let mut items = markdown::parse(
                Path::new("other.md"),
                "# D\n`dsn~d~1`\nCovers:\n- swr~rev~1\n- swr~kept~1\n- swr~missing~1\n",
            )
            .unwrap()
            .items;
            items.extend(
                base.0
                    .iter()
                    .filter(|item| item.id.typ().as_str() == "swr")
                    .cloned(),
            );
            items.extend(
                markdown::parse(Path::new("z.md"), "# Kept\n`swr~kept~1`\n")
                    .unwrap()
                    .items,
            );
            let trace = trace::trace(&items, &Config::parse(CONFIG).unwrap());
            (items, trace)
        };
        let found = changes((&base.0, &base.1), (&moved.0, &moved.1));
        let broken: Vec<(String, LinkStatus)> = found
            .broken
            .iter()
            .map(|link| (link.target.to_string(), link.status))
            .collect();
        assert_eq!(broken, [("swr~kept~1".to_owned(), LinkStatus::Ambiguous)]);
        assert_eq!(ids(&found.added), Vec::<String>::new());
        assert!(changes((&base.0, &base.1), (&base.0, &base.1)).is_empty());
    }
}
