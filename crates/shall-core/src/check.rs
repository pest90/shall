//! Checks over all items of a project, independent of their source format.

use std::collections::HashMap;

use crate::diagnostic::Diagnostic;
use crate::item::Item;

/// Reports items that share type and name, whatever their revision.
///
/// The first definition, ordered by location, is the original; every other
/// one gets an error that points back to it. Duplicate IDs are errors of
/// the highest severity, because links to them are ambiguous.
pub fn duplicate_ids(items: &[Item]) -> Vec<Diagnostic> {
    let mut by_id: HashMap<(&str, &str), Vec<&Item>> = HashMap::new();
    for item in items {
        by_id
            .entry((item.id.typ().as_str(), item.id.name()))
            .or_default()
            .push(item);
    }
    let mut diagnostics: Vec<Diagnostic> = by_id
        .into_values()
        .filter(|group| group.len() > 1)
        .flat_map(|mut group| {
            group.sort_by(|a, b| a.location.cmp(&b.location));
            let first = group[0];
            group[1..]
                .iter()
                .map(|item| {
                    Diagnostic::error(
                        item.location.clone(),
                        format!(
                            "duplicate ID {}, already defined as {} at {}",
                            item.id, first.id, first.location
                        ),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    diagnostics.sort_by(|a, b| a.location.cmp(&b.location));
    diagnostics
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::markdown;

    fn items(files: &[(&str, &str)]) -> Vec<Item> {
        files
            .iter()
            .flat_map(|(path, text)| markdown::parse(Path::new(path), text).unwrap().items)
            .collect()
    }

    #[test]
    fn reports_duplicates_across_files_and_revisions() {
        let items = items(&[
            ("b.md", "`req~x~2`\n\n`req~y~1`\n"),
            ("a.md", "`dsn~x~1`\n`req~x~1`\n"),
            ("c.md", "`req~x~1`\n"),
        ]);
        let messages: Vec<_> = duplicate_ids(&items)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            messages,
            [
                "b.md:1: duplicate ID req~x~2, already defined as req~x~1 at a.md:2",
                "c.md:1: duplicate ID req~x~1, already defined as req~x~1 at a.md:2",
            ]
        );
    }

    #[test]
    fn accepts_unique_ids() {
        let items = items(&[("a.md", "`req~x~1`\n`dsn~x~1`\n`req~y~1`\n")]);
        assert!(duplicate_ids(&items).is_empty());
    }
}
