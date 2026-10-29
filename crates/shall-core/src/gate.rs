//! The check modes: which defects make `shall check` fail.
//!
//! `shall` never blocks by itself; it reports every defect, and the mode
//! decides the exit code:
//!
//! - `report`: no defect fails the check;
//! - `new`: an error fails it if an earlier run, such as the target
//!   branch, did not have it; defects are compared by severity, file and
//!   message, not by line and with line numbers in the message ignored,
//!   since lines move with every edit;
//! - `mature`: an error fails it if it belongs to a mature item, one whose
//!   status is in `mature_status` (`approved` by default) or a tag that
//!   covers a mature item, or to no item at all, such as an unreadable
//!   file; errors of other items are reported but do not fail it;
//! - `strict`: every error and warning fails it.
//!
//! A defect belongs to the item whose lines include it: a requirement from
//! its ID to its last line, a tag on its line. `[check.level.<type>]` sets
//! the mode for the defects of the items of one type; a mode given on the
//! command line replaces the configuration, the levels' modes included.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::config::{Config, Mode};
use crate::diagnostic::{Diagnostic, Severity};
use crate::item::{Item, Origin, Status};

/// The defects of an earlier run, to tell new defects apart in mode
/// `new`: how often each severity, file and message occurred.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Baseline {
    counts: HashMap<(Severity, String, String), usize>,
}

impl Baseline {
    /// Adds a defect of the earlier run.
    pub fn add(&mut self, severity: Severity, path: &str, message: &str) {
        *self.counts.entry(key(severity, path, message)).or_default() += 1;
    }

    /// Reads the `defects` of a `trace.json` of the variant `variant`, or
    /// of all items without one; a document of another variant or of
    /// another `schema_version` is an error.
    pub fn from_trace_json(json: &str, variant: Option<&str>) -> Result<Self, String> {
        #[derive(serde::Deserialize)]
        struct Document {
            schema_version: u32,
            variant: Option<VariantName>,
            defects: Vec<Defect>,
        }
        #[derive(serde::Deserialize)]
        struct VariantName {
            name: String,
        }
        #[derive(serde::Deserialize)]
        struct Defect {
            severity: Severity,
            message: String,
            location: Location,
        }
        #[derive(serde::Deserialize)]
        struct Location {
            path: String,
        }
        let document: Document = serde_json::from_str(json).map_err(|error| error.to_string())?;
        if document.schema_version != crate::export::SCHEMA_VERSION {
            return Err(format!(
                "schema_version {} is not {}",
                document.schema_version,
                crate::export::SCHEMA_VERSION
            ));
        }
        let other = document.variant.map(|variant| variant.name);
        if other.as_deref() != variant {
            let name = |variant: Option<&str>| {
                variant.map_or_else(|| "all items".to_owned(), |name| format!("variant {name}"))
            };
            return Err(format!(
                "it traces {}, but this check {}",
                name(other.as_deref()),
                name(variant)
            ));
        }
        let mut baseline = Self::default();
        for defect in document.defects {
            baseline.add(defect.severity, &defect.location.path, &defect.message);
        }
        Ok(baseline)
    }
}

/// The defects among `diagnostics` that fail the check, in their order.
///
/// `items` are all items read, also those outside a traced variant, to
/// find the item of a defect; `mode` replaces the modes of `config`;
/// `baseline` holds the defects of the earlier run for mode `new`, without
/// which every error counts as new.
///
/// Several items can cover a defect's line, e.g. the tags of one line;
/// those whose ID or tag text the message names decide, or else all of
/// them, and the defect fails if it fails for one of them. A tag is
/// mature if one of the items it covers is, or if it covers none that
/// exists.
pub fn failing<'a>(
    diagnostics: &'a [Diagnostic],
    items: &[Item],
    config: &Config,
    mode: Option<Mode>,
    baseline: Option<&Baseline>,
) -> Vec<&'a Diagnostic> {
    // Items by file, to find those a defect is in, and the statuses of
    // the requirements by type and name, for the maturity of tags.
    let mut by_file: HashMap<&str, Vec<&Item>> = HashMap::new();
    let mut statuses: HashMap<(&str, &str), Vec<Status>> = HashMap::new();
    for item in items {
        by_file.entry(item.location.path()).or_default().push(item);
        if item.origin == Origin::Requirement {
            statuses
                .entry((item.id.typ().as_str(), item.id.name()))
                .or_default()
                .push(item.status);
        }
    }
    let mature_status = &config.check.mature_status;
    let mature = |item: &Item| match item.origin {
        Origin::Requirement => mature_status.contains(&item.status),
        Origin::Tag { .. } => {
            let targets: Vec<Status> = item
                .covers
                .iter()
                .filter_map(|link| statuses.get(&(link.id.typ().as_str(), link.id.name())))
                .flatten()
                .copied()
                .collect();
            targets.is_empty() || targets.iter().any(|status| mature_status.contains(status))
        }
    };
    let mut left = baseline.map(|baseline| baseline.counts.clone());
    diagnostics
        .iter()
        .filter(|diagnostic| {
            let line = diagnostic.location.line();
            let covering: Vec<&Item> = by_file
                .get(diagnostic.location.path())
                .into_iter()
                .flatten()
                .copied()
                .filter(|item| item.location.line() <= line && line <= item.end_line)
                .collect();
            let named: Vec<&Item> = covering
                .iter()
                .copied()
                .filter(|item| names(&diagnostic.message, item))
                .collect();
            let candidates = if named.is_empty() { covering } else { named };
            let error = diagnostic.severity == Severity::Error;
            // Whether the defect is new is the same for every candidate, and
            // takes it from the baseline once.
            let new = error && {
                let key = key(
                    diagnostic.severity,
                    diagnostic.location.path(),
                    &diagnostic.message,
                );
                let known = left
                    .as_mut()
                    .and_then(|left| left.get_mut(&key))
                    .filter(|count| **count > 0)
                    .map(|count| *count -= 1)
                    .is_some();
                !known
            };
            let fails = |item: Option<&Item>| {
                let mode = mode.unwrap_or_else(|| {
                    item.and_then(|item| config.check.levels.get(item.id.typ()))
                        .map_or(config.check.mode, |level| level.mode)
                });
                match mode {
                    Mode::Report => false,
                    Mode::Strict => true,
                    Mode::Mature => error && item.is_none_or(mature),
                    Mode::New => new,
                }
            };
            if candidates.is_empty() {
                fails(None)
            } else {
                candidates.iter().any(|item| fails(Some(item)))
            }
        })
        .collect()
}

/// Whether the message names the item: its ID, or the tag as written.
fn names(message: &str, item: &Item) -> bool {
    match &item.origin {
        Origin::Tag { text, .. } => message.contains(text.as_str()),
        Origin::Requirement => message.contains(&item.id.to_string()),
    }
}

/// The key of a defect for mode `new`: its severity, file and message,
/// with line numbers in the message replaced, such as `x.md:6` or
/// `line 6`, so that a defect stays the same when lines move.
fn key(severity: Severity, path: &str, message: &str) -> (Severity, String, String) {
    static LINE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\S):\d+\b|\bline \d+\b").expect("valid regex"));
    let message = LINE.replace_all(message, |captures: &regex::Captures<'_>| {
        match captures.get(1) {
            Some(before) => format!("{}:#", before.as_str()),
            None => "line #".to_owned(),
        }
    });
    (severity, path.to_owned(), message.into_owned())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{markdown, trace};

    const REQUIREMENTS: &str = "\
`dsn~a~1`
Status: draft

`dsn~b~1`
Status: approved

`hlr~c~1`
Status: draft
";

    /// The lines of the defects that fail, for the requirements above with
    /// `config` plus levels dsn and hlr that need impl.
    fn failing_lines(config: &str, mode: Option<Mode>, baseline: Option<&Baseline>) -> Vec<String> {
        let config = Config::parse(&format!(
            "[[level]]\ntype = \"dsn\"\nname = \"D\"\nneeds = [\"impl\"]\n\n\
             [[level]]\ntype = \"hlr\"\nname = \"H\"\nneeds = [\"impl\"]\n\n{config}"
        ))
        .unwrap();
        let parsed = markdown::parse(Path::new("x.md"), REQUIREMENTS).unwrap();
        let mut diagnostics = trace::trace(&parsed.items, &config).defects;
        // A defect at no item, and a warning at an approved one.
        diagnostics.push(Diagnostic::error(
            crate::Location::new(Path::new("build/r.xml"), 1).unwrap(),
            "invalid XML",
        ));
        diagnostics.push(Diagnostic::warning(
            crate::Location::new(Path::new("x.md"), 5).unwrap(),
            "a warning",
        ));
        failing(&diagnostics, &parsed.items, &config, mode, baseline)
            .iter()
            .map(|diagnostic| diagnostic.to_string())
            .collect()
    }

    #[test]
    fn modes_decide_which_defects_fail() {
        assert_eq!(failing_lines("", None, None), Vec::<String>::new());
        assert_eq!(failing_lines("", Some(Mode::Strict), None).len(), 5);
        assert_eq!(
            failing_lines("[check]\nmode = \"mature\"\n", None, None),
            [
                "x.md:4: dsn~b~1: not covered by required type 'impl'",
                "build/r.xml:1: invalid XML",
            ]
        );
        assert_eq!(
            failing_lines(
                "[check]\nmode = \"mature\"\nmature_status = [\"draft\", \"approved\"]\n",
                None,
                None
            )
            .len(),
            4
        );
    }

    #[test]
    fn levels_can_be_stricter_and_the_command_line_wins() {
        let config = "[check.level.hlr]\nmode = \"strict\"\n";
        assert_eq!(
            failing_lines(config, None, None),
            ["x.md:7: hlr~c~1: not covered by required type 'impl'"]
        );
        assert_eq!(
            failing_lines(config, Some(Mode::Report), None),
            Vec::<String>::new()
        );
    }

    #[test]
    fn mode_new_ignores_known_defects_and_lines() {
        let mut baseline = Baseline::default();
        baseline.add(
            Severity::Error,
            "x.md",
            "dsn~a~1: not covered by required type 'impl'",
        );
        baseline.add(Severity::Error, "build/r.xml", "invalid XML");
        assert_eq!(
            failing_lines("[check]\nmode = \"new\"\n", None, Some(&baseline)),
            [
                "x.md:4: dsn~b~1: not covered by required type 'impl'",
                "x.md:7: hlr~c~1: not covered by required type 'impl'",
            ]
        );
        let json = r#"{"schema_version": 1, "variant": null, "defects": [{"severity": "error", "message": "invalid XML", "location": {"path": "build/r.xml", "line": 9}}]}"#;
        assert_eq!(
            Baseline::from_trace_json(json, None).unwrap().counts.len(),
            1
        );
        assert_eq!(
            Baseline::from_trace_json(json, Some("basis")).unwrap_err(),
            "it traces all items, but this check variant basis"
        );
        assert!(Baseline::from_trace_json(&json.replace(": 1,", ": 2,"), None).is_err());
        assert!(Baseline::from_trace_json("{}", None).is_err());
    }

    /// The failing defects of `requirements` and the tags in `code`, with
    /// levels dsn (needing impl) and utest as verification type.
    fn failing_with_code(config: &str, requirements: &str, code: &str) -> Vec<String> {
        let config = Config::parse(&format!(
            "[[level]]\ntype = \"dsn\"\nname = \"D\"\nneeds = [\"impl\"]\n\n\
             [verification]\ntypes = [\"utest\"]\n\n{config}"
        ))
        .unwrap();
        let mut items = markdown::parse(Path::new("x.md"), requirements)
            .unwrap()
            .items;
        items.extend(crate::tags::scan(Path::new("a.c"), code).unwrap().items);
        let diagnostics = trace::trace(&items, &config).defects;
        failing(&diagnostics, &items, &config, None, None)
            .iter()
            .map(|diagnostic| diagnostic.to_string())
            .collect()
    }

    #[test]
    fn the_tag_a_defect_names_decides_whatever_the_order() {
        let config = "[check.level.utest]\nmode = \"strict\"\n";
        let requirements = "`dsn~a~1`\n";
        for code in [
            "// [impl->dsn~a~1] [utest->dsn~zz~1]\n",
            "// [utest->dsn~zz~1] [impl->dsn~a~1]\n",
        ] {
            assert_eq!(
                failing_with_code(config, requirements, code),
                ["a.c:1: tag [utest->dsn~zz~1]: unknown target dsn~zz~1"],
                "{code}"
            );
        }
    }

    #[test]
    fn tags_are_as_mature_as_what_they_cover() {
        let config = "[check]\nmode = \"mature\"\n";
        let requirements = "`dsn~draft~1`\nStatus: draft\n\n`dsn~done~1`\n";
        let code =
            "// [impl->dsn~draft~1>>utest]\n// [impl->dsn~done~1>>utest]\n// [impl->dsn~gone~1]\n";
        assert_eq!(
            failing_with_code(config, requirements, code),
            [
                // The tag below the approved item and the tag of no existing
                // item count; the one below the draft does not.
                "a.c:2: tag [impl->dsn~done~1>>utest]: not covered by required type 'utest'",
                "a.c:3: tag [impl->dsn~gone~1]: unknown target dsn~gone~1",
            ]
        );
    }

    #[test]
    fn keys_ignore_line_numbers_in_messages() {
        assert_eq!(
            key(
                Severity::Error,
                "x.md",
                "duplicate ID a~b~1, already defined as a~b~1 at x.md:6"
            ),
            key(
                Severity::Error,
                "x.md",
                "duplicate ID a~b~1, already defined as a~b~1 at x.md:9"
            )
        );
        assert_eq!(
            key(
                Severity::Error,
                "x.md",
                "duplicate keyword 'Status:', first used in line 3"
            )
            .2,
            "duplicate keyword 'Status:', first used in line #"
        );
        // A revision is no line.
        assert_eq!(
            key(
                Severity::Error,
                "x.md",
                "covers outdated revision a~b~1 (current revision: 2)"
            )
            .2,
            "covers outdated revision a~b~1 (current revision: 2)"
        );
    }
}
