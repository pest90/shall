//! Item IDs of the form `typ~name~revision`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Type of an item, i.e. its level (`swr`, `dsn`) or artifact kind (`impl`,
/// `utest`).
///
/// Consists of ASCII letters only, as OpenFastTrace requires.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ItemType(String);

impl ItemType {
    /// Returns the type as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ItemType {
    type Err = ParseIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic()) {
            Ok(Self(s.to_owned()))
        } else {
            Err(ParseIdError::InvalidType(s.to_owned()))
        }
    }
}

impl TryFrom<String> for ItemType {
    type Error = ParseIdError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<ItemType> for String {
    fn from(typ: ItemType) -> Self {
        typ.0
    }
}

impl fmt::Display for ItemType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Unique ID of an item, written as `typ~name~revision`, e.g.
/// `swr~motor.overtemp-k4f2~2`.
///
/// - `typ`: see [`ItemType`].
/// - `name`: starts with an ASCII letter, followed by ASCII letters,
///   digits, `_` and `-`; `.` separates non-empty segments.
/// - `revision`: decimal number without leading zeros, at most
///   [`MAX_REVISION`]. `0` is reserved for items generated from code and
///   test tags.
///
/// Every accepted ID is also valid in OpenFastTrace: the grammar is an
/// ASCII subset of OFT's `\p{Alpha}+~\p{Alpha}[\w-]*(?:\.[\w-]+)*~\d+`.
///
/// [`Display`](fmt::Display) writes the same form that
/// [`FromStr`] accepts, so IDs round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ItemId {
    typ: ItemType,
    name: String,
    revision: u32,
}

impl ItemId {
    /// Creates an ID, validating name and revision so that the ID
    /// round-trips.
    pub fn new(typ: ItemType, name: &str, revision: u32) -> Result<Self, ParseIdError> {
        if revision > MAX_REVISION {
            return Err(ParseIdError::InvalidRevision(revision.to_string()));
        }
        Ok(Self {
            typ,
            name: parse_name(name)?,
            revision,
        })
    }

    /// Level or artifact kind.
    pub fn typ(&self) -> &ItemType {
        &self.typ
    }

    /// Stable name; never changes after the item was created.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Revision, increased on every change of content.
    pub fn revision(&self) -> u32 {
        self.revision
    }

    /// Returns whether `other` names the same item, ignoring the revision.
    pub fn same_item(&self, other: &ItemId) -> bool {
        self.typ == other.typ && self.name == other.name
    }
}

impl FromStr for ItemId {
    type Err = ParseIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('~');
        let (Some(typ), Some(name), Some(revision), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(ParseIdError::Format(s.to_owned()));
        };
        Ok(Self {
            typ: typ.parse()?,
            name: parse_name(name)?,
            revision: parse_revision(revision)?,
        })
    }
}

/// Highest revision; OpenFastTrace stores revisions as Java `int`.
pub const MAX_REVISION: u32 = i32::MAX as u32;

fn parse_name(name: &str) -> Result<String, ParseIdError> {
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    let starts_with_letter = name.bytes().next().is_some_and(|b| b.is_ascii_alphabetic());
    if starts_with_letter && name.split('.').all(valid_segment) {
        Ok(name.to_owned())
    } else {
        Err(ParseIdError::InvalidName(name.to_owned()))
    }
}

fn parse_revision(revision: &str) -> Result<u32, ParseIdError> {
    let canonical = !revision.is_empty()
        && revision.bytes().all(|b| b.is_ascii_digit())
        && (revision == "0" || !revision.starts_with('0'));
    canonical
        .then(|| revision.parse().ok())
        .flatten()
        .filter(|&revision| revision <= MAX_REVISION)
        .ok_or_else(|| ParseIdError::InvalidRevision(revision.to_owned()))
}

impl TryFrom<String> for ItemId {
    type Error = ParseIdError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<ItemId> for String {
    fn from(id: ItemId) -> Self {
        id.to_string()
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}~{}~{}", self.typ, self.name, self.revision)
    }
}

/// Error when parsing an [`ItemId`] or [`ItemType`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseIdError {
    /// The ID does not consist of exactly three parts separated by `~`.
    #[error("invalid ID '{0}': expected 'typ~name~revision'")]
    Format(String),
    /// The type is empty or contains characters other than ASCII letters.
    #[error("invalid type '{0}': expected ASCII letters only")]
    InvalidType(String),
    /// The name is empty or contains invalid characters.
    #[error(
        "invalid name '{0}': expected a letter followed by ASCII letters, digits, \
         '_' and '-', with '.' between non-empty segments"
    )]
    InvalidName(String),
    /// The revision is not a canonical decimal number up to [`MAX_REVISION`].
    #[error("invalid revision '{0}': expected a number without leading zeros, at most 2147483647")]
    InvalidRevision(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> ItemId {
        s.parse().unwrap()
    }

    #[test]
    fn parses_valid_ids() {
        let parsed = id("swr~motor.overtemp-k4f2~2");
        assert_eq!(parsed.typ().as_str(), "swr");
        assert_eq!(parsed.name(), "motor.overtemp-k4f2");
        assert_eq!(parsed.revision(), 2);

        for valid in [
            "req~x~1",
            "dsn~motor.temp-monitor-r5t1~1",
            "impl~fault_log-3fa2~0",
            "cap~A.b_c-D.9~2147483647",
            "dsn~x_~1",
            "dsn~x-~1",
            "dsn~x.-y~1",
            "dsn~x._~1",
            "UTest~x1.2~10",
        ] {
            assert_eq!(id(valid).to_string(), valid, "round trip of {valid}");
        }
    }

    #[test]
    fn rejects_wrong_number_of_parts() {
        for invalid in ["", "swr", "swr~x", "swr~x~1~2", "swr-x-1"] {
            assert_eq!(
                invalid.parse::<ItemId>(),
                Err(ParseIdError::Format(invalid.to_owned())),
            );
        }
    }

    #[test]
    fn rejects_invalid_types() {
        for typ in ["", "l3", "l3d", "3l", "l-x", "l_x", "swr ", "ü"] {
            let s = format!("{typ}~x~1");
            assert_eq!(
                s.parse::<ItemId>(),
                Err(ParseIdError::InvalidType(typ.to_owned())),
            );
        }
    }

    #[test]
    fn rejects_invalid_names() {
        for name in [
            "", "1x", ".x", "x.", "x..y", "-x", "_x", "x y", "x/y", "über", "xü",
        ] {
            let s = format!("swr~{name}~1");
            assert_eq!(
                s.parse::<ItemId>(),
                Err(ParseIdError::InvalidName(name.to_owned())),
            );
        }
    }

    #[test]
    fn rejects_invalid_revisions() {
        for revision in [
            "",
            "01",
            "00",
            "-1",
            "+1",
            "1.0",
            "x",
            " 1",
            "2147483648",
            "4294967296",
        ] {
            let s = format!("swr~x~{revision}");
            assert_eq!(
                s.parse::<ItemId>(),
                Err(ParseIdError::InvalidRevision(revision.to_owned())),
            );
        }
    }

    #[test]
    fn new_validates_name_and_revision() {
        let typ: ItemType = "swr".parse().unwrap();
        let created = ItemId::new(typ.clone(), "motor.x-k4f2", 3).unwrap();
        assert_eq!(created, id("swr~motor.x-k4f2~3"));
        assert_eq!(
            ItemId::new(typ.clone(), "a~b", 1),
            Err(ParseIdError::InvalidName("a~b".to_owned())),
        );
        assert_eq!(
            ItemId::new(typ, "x", MAX_REVISION + 1),
            Err(ParseIdError::InvalidRevision("2147483648".to_owned())),
        );
    }

    #[test]
    fn same_item_ignores_revision() {
        assert!(id("swr~x~1").same_item(&id("swr~x~2")));
        assert!(!id("swr~x~1").same_item(&id("dsn~x~1")));
        assert!(!id("swr~x~1").same_item(&id("swr~y~1")));
    }

    #[test]
    fn serializes_as_string() {
        let parsed = id("swr~x~2");
        let value = toml::Value::try_from(&parsed).unwrap();
        assert_eq!(value, toml::Value::String("swr~x~2".to_owned()));
        assert_eq!(value.try_into::<ItemId>().unwrap(), parsed);
        assert!(
            toml::Value::String("swr~x".to_owned())
                .try_into::<ItemId>()
                .is_err()
        );
    }
}
