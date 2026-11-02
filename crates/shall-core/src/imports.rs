//! The direction of links between a project and its imports.
//!
//! A project imports others, such as a platform, which may import others
//! in turn; links point from a project to the projects it imports,
//! directly or not, never the other way round or between projects that do
//! not import each other. A platform item that covers a customer
//! requirement would make the platform depend on its customers, so the
//! tracer gives such a link the status
//! [`LinkStatus::AgainstImports`](crate::trace::LinkStatus::AgainstImports):
//! an error that covers nothing, also when its target exists in the
//! traced project.
//!
//! Items tell their project by [`Item::import`](crate::Item::import):
//! none for the traced project, otherwise the name of the import.

use std::collections::{HashMap, HashSet};

/// Which project of a trace imports which, by import name, none being
/// the traced project; without any, every link is allowed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Imports {
    /// The projects each project imports, directly or not, itself
    /// included.
    reach: HashMap<Option<String>, HashSet<Option<String>>>,
    /// How messages name each project, e.g. its directory below the
    /// workspace root, the same in the trace of every project that
    /// imports it.
    names: HashMap<Option<String>, String>,
}

impl Imports {
    /// Adds a project of the trace with the projects it imports, directly
    /// or not, and its name for messages.
    pub fn add(
        &mut self,
        import: Option<&str>,
        name: impl Into<String>,
        imported: impl IntoIterator<Item = Option<String>>,
    ) {
        let key = import.map(str::to_owned);
        let mut reach: HashSet<Option<String>> = imported.into_iter().collect();
        reach.insert(key.clone());
        self.reach.insert(key.clone(), reach);
        self.names.insert(key, name.into());
    }

    /// Whether an item of the project `from` may link to one of `to`: the
    /// traced project may link anywhere, every other project to itself and
    /// what it imports. Projects not added may link anywhere.
    pub fn allows(&self, from: Option<&str>, to: Option<&str>) -> bool {
        from.is_none()
            || from == to
            || self
                .reach
                .get(&from.map(str::to_owned))
                .is_none_or(|reach| reach.contains(&to.map(str::to_owned)))
    }

    /// How messages name the project, e.g. `'platform'`.
    pub fn name(&self, import: Option<&str>) -> String {
        match self.names.get(&import.map(str::to_owned)) {
            Some(name) => format!("'{name}'"),
            None => match import {
                Some(import) => format!("'{import}'"),
                None => "the traced project".to_owned(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_may_link_to_what_they_import() {
        let mut imports = Imports::default();
        imports.add(None, "projects/a", [Some("plat".to_owned())]);
        imports.add(Some("plat"), "platform", [Some("plat.lib".to_owned())]);
        imports.add(Some("plat.lib"), "lib", []);
        assert!(imports.allows(None, Some("plat.lib")));
        assert!(imports.allows(Some("plat"), Some("plat.lib")));
        assert!(imports.allows(Some("plat"), Some("plat")));
        assert!(!imports.allows(Some("plat"), None));
        assert!(!imports.allows(Some("plat.lib"), Some("plat")));
        assert!(Imports::default().allows(Some("x"), None));
        assert_eq!(imports.name(Some("plat")), "'platform'");
    }
}
