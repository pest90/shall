//! Output formats of `shall check` for CI systems.
//!
//! - `text`: one line `path:line: message` per defect, warnings as
//!   `path:line: warning: message`;
//! - `github`: workflow commands such as
//!   `::error file=reqs/a.md,line=4,title=shall::message`, which GitHub
//!   Actions shows as annotations in the diff of a pull request (Forgejo
//!   Actions accepts them, but shows them in the log only); with
//!   `$GITHUB_STEP_SUMMARY` set, a Markdown summary is appended to that
//!   file as well;
//! - `sarif`: one SARIF 2.1.0 document for code scanning views.
//!
//! Annotations and SARIF need paths relative to the repository, so they
//! get a prefix: the path of the project or workspace root below the top
//! of its Git work tree, or else below `$GITHUB_WORKSPACE`.

use std::fmt::Write as _;

use clap::ValueEnum;
use shall_core::{Diagnostic, Severity};

/// How `shall check` prints the defects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// `path:line: message`, as editors and terminals link it.
    #[default]
    Text,
    /// GitHub Actions annotations (Forgejo Actions: in the log), and a
    /// step summary.
    Github,
    /// A SARIF 2.1.0 document.
    Sarif,
}

/// A defect as a line of text.
pub fn text(diagnostic: &Diagnostic) -> String {
    match diagnostic.severity {
        Severity::Error => diagnostic.to_string(),
        Severity::Warning => format!("{}: warning: {}", diagnostic.location, diagnostic.message),
    }
}

/// The path of a defect below the repository, with `prefix` (the
/// project root's path there, empty for the repository root) in front.
fn repository_path(diagnostic: &Diagnostic, prefix: &str) -> String {
    let path = diagnostic.location.path();
    if prefix.is_empty() {
        path.to_owned()
    } else {
        format!("{}/{path}", prefix.trim_end_matches('/'))
    }
}

/// A defect as a GitHub workflow command.
pub fn github(diagnostic: &Diagnostic, prefix: &str) -> String {
    let command = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    format!(
        "::{command} file={},line={},title=shall::{}",
        property(&repository_path(diagnostic, prefix)),
        diagnostic.location.line(),
        data(&diagnostic.message)
    )
}

/// Escapes the message of a workflow command.
fn data(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Escapes a property of a workflow command, such as a file name.
fn property(text: &str) -> String {
    data(text).replace(':', "%3A").replace(',', "%2C")
}

/// The most defects listed in the step summary, which GitHub limits in
/// size.
const SUMMARY_ROWS: usize = 500;

/// The Markdown step summary: the summary lines and a table of the
/// defects.
pub fn step_summary(diagnostics: &[Diagnostic], summaries: &[String], prefix: &str) -> String {
    let mut out = String::from("### shall check\n\n");
    for summary in summaries {
        let _ = writeln!(out, "{}  ", cell(summary));
    }
    if diagnostics.is_empty() {
        out.push_str("\nNo defects.\n");
        return out;
    }
    out.push_str("\n| Severity | Location | Message |\n| --- | --- | --- |\n");
    for diagnostic in diagnostics.iter().take(SUMMARY_ROWS) {
        let severity = match diagnostic.severity {
            Severity::Error => "✗ error",
            Severity::Warning => "! warning",
        };
        let location = format!(
            "{}:{}",
            repository_path(diagnostic, prefix),
            diagnostic.location.line()
        );
        let _ = writeln!(
            out,
            "| {severity} | {} | {} |",
            cell(&location),
            cell(&diagnostic.message)
        );
    }
    if diagnostics.len() > SUMMARY_ROWS {
        let _ = writeln!(
            out,
            "\n{} more defects are not listed; see the log or the report.",
            diagnostics.len() - SUMMARY_ROWS
        );
    }
    out
}

/// Text for a Markdown table cell, shown as written: ASCII punctuation
/// that GitHub's Markdown would read as markup (strikethrough by the `~`
/// of IDs, emphasis, links, HTML, entities, table cells) backslash
/// escaped, line breaks as spaces.
pub fn cell(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\r' | '\n' => out.push(' '),
            '\\' | '`' | '*' | '_' | '~' | '[' | ']' | '<' | '>' | '|' | '&' | '#' | '!' | '$' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

/// A path as a relative URI reference: every byte but unreserved ones and
/// `/` percent-encoded.
fn uri(path: &str) -> String {
    let mut out = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// The defects as a SARIF 2.1.0 document; every defect is a result of the
/// rule `shall`, located by its path relative to the repository.
pub fn sarif(diagnostics: &[Diagnostic], prefix: &str) -> String {
    let results: Vec<serde_json::Value> = diagnostics
        .iter()
        .map(|diagnostic| {
            serde_json::json!({
                "ruleId": "shall",
                "level": match diagnostic.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                },
                "message": { "text": diagnostic.message },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": {
                            "uri": uri(&repository_path(diagnostic, prefix)),
                        },
                        "region": { "startLine": diagnostic.location.line() },
                    },
                }],
            })
        })
        .collect();
    let document = serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "shall",
                    "version": env!("CARGO_PKG_VERSION"),
                    "rules": [{
                        "id": "shall",
                        "shortDescription": { "text": "Traceability defect" },
                        "fullDescription": {
                            "text": "A defect that shall check reports: a requirement \
                                     without the coverage it needs, an invalid link, \
                                     a failed test or a problem with the files read."
                        },
                    }],
                },
            },
            "results": results,
        }],
    });
    let mut json = serde_json::to_string_pretty(&document).expect("SARIF serializes");
    json.push('\n');
    json
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use shall_core::Location;

    use super::*;

    fn defect(severity: Severity, path: &str, message: &str) -> Diagnostic {
        let location = Location::new(Path::new(path), 4).unwrap();
        match severity {
            Severity::Error => Diagnostic::error(location, message),
            Severity::Warning => Diagnostic::warning(location, message),
        }
    }

    #[test]
    fn escapes_workflow_commands() {
        assert_eq!(
            github(
                &defect(Severity::Error, "reqs/a,b:c.md", "50% done\nnext"),
                ""
            ),
            "::error file=reqs/a%2Cb%3Ac.md,line=4,title=shall::50%25 done%0Anext"
        );
        assert_eq!(
            github(&defect(Severity::Warning, "a.md", "x"), "sub/dir"),
            "::warning file=sub/dir/a.md,line=4,title=shall::x"
        );
    }

    #[test]
    fn summarises_in_markdown() {
        let summary = step_summary(
            &[defect(
                Severity::Error,
                "a|b.md",
                "dsn~a~1 covers *b* <c> &lt;",
            )],
            &["not ok: 1 error".to_owned()],
            "",
        );
        assert_eq!(
            summary,
            "### shall check\n\nnot ok: 1 error  \n\n| Severity | Location | Message |\n\
             | --- | --- | --- |\n\
             | ✗ error | a\\|b.md:4 | dsn\\~a\\~1 covers \\*b\\* \\<c\\> \\&lt; |\n"
        );
        assert!(step_summary(&[], &[], "").ends_with("No defects.\n"));
    }

    #[test]
    fn writes_sarif() {
        let json: serde_json::Value = serde_json::from_str(&sarif(
            &[defect(Severity::Warning, "my file#1.md", "x")],
            "sub",
        ))
        .unwrap();
        assert_eq!(json["version"], "2.1.0");
        let result = &json["runs"][0]["results"][0];
        assert_eq!(result["level"], "warning");
        let location = &result["locations"][0]["physicalLocation"];
        assert_eq!(location["region"]["startLine"], 4);
        assert_eq!(location["artifactLocation"]["uri"], "sub/my%20file%231.md");
    }
}
