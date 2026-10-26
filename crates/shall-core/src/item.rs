//! Items: requirements, design decisions and the artifacts covering them.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::id::{ItemId, ItemType};

/// A requirement or other traced artifact, e.g. a design decision, a review
/// or an item generated from a code tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// Unique ID.
    pub id: ItemId,
    /// Title, taken from the heading above the ID.
    pub title: String,
    /// Free text between the ID and the first keyword.
    pub description: String,
    /// Reason for the requirement (`Rationale:`).
    pub rationale: Option<String>,
    /// Additional remarks (`Comment:`).
    pub comment: Option<String>,
    /// Maturity (`Status:`).
    pub status: Status,
    /// Types that must cover this item (`Needs:`).
    pub needs: Vec<ItemType>,
    /// Items this item covers (`Covers:`), i.e. its parents.
    pub covers: Vec<Reference>,
    /// Lateral dependencies without effect on coverage (`Depends:`).
    pub depends: Vec<Reference>,
    /// Free-form tags (`Tags:`).
    pub tags: Vec<String>,
    /// Origin, e.g. a chapter of a customer specification (`Source:`).
    pub source: Option<String>,
    /// Where the item is defined: the line of its ID.
    pub location: Location,
}

/// A link from one item to another, with the line it is written on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    /// Target of the link, including the revision the link was made for.
    pub id: ItemId,
    /// Line of the link in the file of the item that contains it.
    pub line: u32,
}

/// A position in a source file, displayed as `path:line`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Location {
    path: String,
    line: u32,
}

impl Location {
    /// Creates a location from a path relative to the project root.
    ///
    /// `.` components are dropped and the platform's separators are
    /// replaced with `/`, so `./reqs/x.md`, and `reqs\x.md` on Windows,
    /// become `reqs/x.md`. Absolute paths and `..` are rejected, because
    /// they would make diagnostics depend on where the project is checked
    /// out.
    pub fn new(path: &Path, line: u32) -> Result<Self, LocationError> {
        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                Component::Normal(part) => parts.push(part.to_string_lossy()),
                Component::CurDir => {}
                Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                    return Err(LocationError(path.to_path_buf()));
                }
            }
        }
        if parts.is_empty() {
            return Err(LocationError(path.to_path_buf()));
        }
        Ok(Self {
            path: parts.join("/"),
            line,
        })
    }

    /// Path relative to the project root, separated by `/` on every
    /// platform, so that diagnostics and `trace.json` do not depend on the
    /// operating system.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Line number, starting at 1.
    pub fn line(&self) -> u32 {
        self.line
    }
}

/// Error for a path that is not relative to the project root.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("'{}' is not a file path relative to the project root", .0.display())]
pub struct LocationError(pub PathBuf);

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.path, self.line)
    }
}

/// Maturity of an item, from the `Status:` keyword.
///
/// As in OpenFastTrace, an item without `Status:` is [`Status::Approved`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Work in progress.
    Draft,
    /// Ready for review.
    Proposed,
    /// Reviewed and accepted.
    #[default]
    Approved,
    /// Reviewed and rejected; kept for reference.
    Rejected,
}

impl Status {
    /// All statuses. The order is not a ranking: which statuses count as
    /// mature is configured (`check.mature_status`).
    pub const ALL: [Status; 4] = [
        Status::Draft,
        Status::Proposed,
        Status::Approved,
        Status::Rejected,
    ];

    /// Returns the keyword used in requirement files.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::Proposed => "proposed",
            Status::Approved => "approved",
            Status::Rejected => "rejected",
        }
    }
}

impl FromStr for Status {
    type Err = ParseStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Status::ALL
            .into_iter()
            .find(|status| status.as_str() == s)
            .ok_or_else(|| ParseStatusError(s.to_owned()))
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error for an unknown `Status:` value.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown status '{0}': expected draft, proposed, approved or rejected")]
pub struct ParseStatusError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_displays_as_path_and_line() {
        let location = Location::new(&Path::new("reqs").join("l3").join("x.md"), 42).unwrap();
        assert_eq!(location.path(), "reqs/l3/x.md");
        assert_eq!(location.to_string(), "reqs/l3/x.md:42");
    }

    #[test]
    fn location_drops_current_dir() {
        let location = Location::new(Path::new("./reqs/./x.md"), 1).unwrap();
        assert_eq!(location.path(), "reqs/x.md");
    }

    #[test]
    fn location_rejects_non_relative_paths() {
        for path in ["/reqs/x.md", "../x.md", "reqs/../x.md", ".", ""] {
            assert_eq!(
                Location::new(Path::new(path), 1),
                Err(LocationError(PathBuf::from(path))),
                "{path}",
            );
        }
    }

    #[test]
    fn status_round_trips() {
        for status in Status::ALL {
            assert_eq!(status.as_str().parse(), Ok(status));
        }
        assert_eq!(Status::default(), Status::Approved);
    }

    #[test]
    fn rejects_unknown_status() {
        for s in ["", "Approved", "accepted", " draft"] {
            assert_eq!(s.parse::<Status>(), Err(ParseStatusError(s.to_owned())));
        }
    }
}
