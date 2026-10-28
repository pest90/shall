//! Variants: which items belong to a selection of features.
//!
//! An item with `Features:` belongs to a variant when the variant selects
//! all of its features. An item without `Features:`, such as a code or
//! test tag, inherits from the items it covers: it belongs to the variant
//! when at least one of them does, or when it covers no existing item.
//! `Satisfied-by:` and `Depends:` do not pass features on, so a customer
//! requirement stays in every variant; a `Satisfied-by:` link to an item
//! outside the variant is a defect of that variant (see [`crate::trace`]).
//!
//! The selection is a filtering step before the tracer: items outside the
//! variant are left out of the trace, and links to them get the status
//! [`LinkStatus::OutsideVariant`](crate::trace::LinkStatus::OutsideVariant),
//! unless they are wrong for another reason, see [`crate::trace`].
//!
//! ```markdown
//! ## Read out fault memory over UDS
//! `cap~diag.uds-readout-t4m1~1`
//!
//! Features: diag_uds
//! ```
//!
//! ```toml
//! [variant.basis]
//! features = ["motor_ctrl", "can"]
//!
//! [variant.premium]
//! features = ["motor_ctrl", "can", "diag_uds"]
//! ```

use std::collections::{BTreeSet, HashMap};

use crate::config::{Config, Variant};
use crate::diagnostic::Diagnostic;
use crate::item::Item;

/// Items split by whether they belong to a variant, each part in the
/// original order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Items of the variant.
    pub inside: Vec<Item>,
    /// Items outside the variant.
    pub outside: Vec<Item>,
}

/// Splits `items` by whether they belong to `variant`.
pub fn select(items: &[Item], variant: &Variant) -> Selection {
    let included = included(items, variant);
    let mut selection = Selection {
        inside: Vec::new(),
        outside: Vec::new(),
    };
    for (item, included) in items.iter().zip(included) {
        if included {
            selection.inside.push(item.clone());
        } else {
            selection.outside.push(item.clone());
        }
    }
    selection
}

/// Whether each item belongs to the variant, in the order of `items`.
///
/// Inheritance follows `Covers:` links through an iterative depth-first
/// search, so long chains cannot overflow the stack. An item met again
/// while its own targets are still being evaluated, in a coverage cycle,
/// counts as inside, so that the cycle stays visible in the variant.
fn included(items: &[Item], variant: &Variant) -> Vec<bool> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Unvisited,
        InProgress,
        Done(bool),
    }

    let mut by_name: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (position, item) in items.iter().enumerate() {
        by_name
            .entry((item.id.typ().as_str(), item.id.name()))
            .or_default()
            .push(position);
    }
    let targets: Vec<Vec<usize>> = items
        .iter()
        .map(|item| {
            if !item.features.is_empty() {
                return Vec::new();
            }
            item.covers
                .iter()
                .filter_map(|link| by_name.get(&(link.id.typ().as_str(), link.id.name())))
                .flatten()
                .copied()
                .collect()
        })
        .collect();
    let selected: BTreeSet<&str> = variant.features.iter().map(String::as_str).collect();
    let own = |item: &Item| {
        item.features
            .iter()
            .all(|feature| selected.contains(feature.as_str()))
    };

    let mut state = vec![State::Unvisited; items.len()];
    for root in 0..items.len() {
        if state[root] != State::Unvisited {
            continue;
        }
        state[root] = State::InProgress;
        // (item, index of its next target)
        let mut calls = vec![(root, 0)];
        while let Some(&(node, next)) = calls.last() {
            if let Some(&target) = targets[node].get(next) {
                calls.last_mut().expect("a call is active").1 += 1;
                if state[target] == State::Unvisited {
                    state[target] = State::InProgress;
                    calls.push((target, 0));
                }
                continue;
            }
            calls.pop();
            let inside = if !items[node].features.is_empty() {
                own(&items[node])
            } else {
                targets[node].is_empty()
                    || targets[node].iter().any(|&target| match state[target] {
                        State::Done(inside) => inside,
                        State::InProgress | State::Unvisited => true,
                    })
            };
            state[node] = State::Done(inside);
        }
    }
    state
        .into_iter()
        .map(|state| matches!(state, State::Done(true)))
        .collect()
}

/// Warns about features of items that no variant selects, since they are
/// often typos. Without any `[variant.*]`, features are not checked.
pub fn check_features(items: &[Item], config: &Config) -> Vec<Diagnostic> {
    if config.variants.is_empty() {
        return Vec::new();
    }
    let known: BTreeSet<&str> = config
        .variants
        .values()
        .flat_map(|variant| &variant.features)
        .map(String::as_str)
        .collect();
    items
        .iter()
        .flat_map(|item| {
            item.features
                .iter()
                .filter(|feature| !known.contains(feature.as_str()))
                .map(move |feature| {
                    Diagnostic::warning(
                        item.location.clone(),
                        format!("{}: feature '{feature}' is selected by no variant", item.id),
                    )
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::item::Origin;
    use crate::{markdown, tags};

    const REQUIREMENTS: &str = "\
`cap~base~1`

`cap~uds~1`
Features: diag_uds

`cap~ota-uds~1`
Features: diag_uds, ota

`swr~uds-read~1`
Covers:
- cap~uds~1

`swr~both~1`
Covers:
- cap~base~1
- cap~uds~1

`hlr~customer~1`
Satisfied-by:
- cap~uds~1

`swr~dangling~1`
Covers:
- cap~gone~1
";

    const CODE: &str = "\
// [impl->swr~uds-read~1]
void read();

// [impl->swr~both~1]
void both();
";

    fn items() -> Vec<Item> {
        let mut items = markdown::parse(Path::new("x.md"), REQUIREMENTS)
            .unwrap()
            .items;
        items.extend(tags::scan(Path::new("src/a.c"), CODE).unwrap().items);
        items
    }

    fn outside(features: &[&str]) -> Vec<String> {
        let variant = Variant::new(features.iter().map(|f| (*f).to_owned()).collect());
        select(&items(), &variant)
            .outside
            .iter()
            .map(|item| match item.origin {
                Origin::Tag { .. } => format!("tag->{}", item.covers[0].id),
                Origin::Requirement => item.id.to_string(),
            })
            .collect()
    }

    #[test]
    fn items_need_all_their_features() {
        assert_eq!(
            outside(&[]),
            [
                "cap~uds~1",
                "cap~ota-uds~1",
                "swr~uds-read~1",
                "tag->swr~uds-read~1",
            ]
        );
        assert_eq!(outside(&["diag_uds"]), ["cap~ota-uds~1"]);
        assert_eq!(outside(&["diag_uds", "ota", "can"]), Vec::<String>::new());
    }

    #[test]
    fn selection_keeps_the_order() {
        let variant = Variant::default();
        let selection = select(&items(), &variant);
        assert_eq!(selection.inside.len() + selection.outside.len(), 9);
        assert_eq!(selection.inside[0].id.to_string(), "cap~base~1");
        assert_eq!(selection.inside[1].id.to_string(), "swr~both~1");
    }

    #[test]
    fn cycles_stay_inside() {
        let text = "`swr~a~1`\nCovers:\n- swr~b~1\n\n`swr~b~1`\nCovers:\n- swr~a~1\n- cap~x~1\n\n`cap~x~1`\nFeatures: x\n";
        let items = markdown::parse(Path::new("x.md"), text).unwrap().items;
        let selection = select(&items, &Variant::default());
        let outside: Vec<String> = selection
            .outside
            .iter()
            .map(|item| item.id.to_string())
            .collect();
        assert_eq!(outside, ["cap~x~1"]);
    }

    #[test]
    fn long_chains_do_not_overflow_the_stack() {
        let mut text = String::from("`swr~n0~1`\nFeatures: x\n\n");
        for n in 1..20_000 {
            text.push_str(&format!("`swr~n{n}~1`\nCovers:\n- swr~n{}~1\n\n", n - 1));
        }
        let items = markdown::parse(Path::new("x.md"), &text).unwrap().items;
        let handle = std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(move || select(&items, &Variant::default()).outside.len())
            .unwrap();
        assert_eq!(handle.join().unwrap(), 20_000);
    }

    #[test]
    fn warns_about_features_of_no_variant() {
        let config = Config::parse("[variant.a]\nfeatures = [\"diag_uds\"]\n").unwrap();
        let messages: Vec<String> = check_features(&items(), &config)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            messages,
            ["x.md:6: cap~ota-uds~1: feature 'ota' is selected by no variant"]
        );
        assert_eq!(check_features(&items(), &Config::default()), []);
    }
}
