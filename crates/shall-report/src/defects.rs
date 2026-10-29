//! The defects as plain text and as JUnit XML, for CI.
//!
//! `defects.txt` has one line per defect as `shall check` prints it:
//! `path:line: message`, warnings as `path:line: warning: message`.
//!
//! `junit.xml` makes every defect a test case, so that CI systems that
//! show test reports show the defects: an error is a failed case, a
//! warning a skipped one, since warnings do not fail a check by
//! themselves. Without defects, there is one passed case. The class name
//! of a case is the defect's file, its name the whole line; times are 0,
//! for readers that expect them and to keep the file reproducible.

use std::fmt::Write;

use shall_core::Severity;
use shall_core::export::{DefectEntry, Document};

/// The defects as lines of text, each ending with a newline.
pub fn text(document: &Document<'_>) -> String {
    document
        .defects
        .iter()
        .map(|defect| line(defect) + "\n")
        .collect()
}

/// A defect as `shall check` prints it.
fn line(defect: &DefectEntry<'_>) -> String {
    match defect.severity {
        Severity::Error => format!("{}: {}", defect.location, defect.message),
        Severity::Warning => format!("{}: warning: {}", defect.location, defect.message),
    }
}

/// The defects as a JUnit XML report.
pub fn junit(document: &Document<'_>) -> String {
    let errors = document
        .defects
        .iter()
        .filter(|defect| defect.severity == Severity::Error)
        .count();
    let warnings = document.defects.len() - errors;
    let tests = document.defects.len().max(1);
    let suite = match &document.variant {
        Some(variant) => format!("shall check, variant {}", variant.name),
        None => "shall check".to_owned(),
    };
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let counts = format!(
        "tests=\"{tests}\" failures=\"{errors}\" errors=\"0\" skipped=\"{warnings}\" time=\"0\""
    );
    let _ = writeln!(out, "<testsuites name=\"shall\" {counts}>");
    let _ = writeln!(out, "  <testsuite name=\"{}\" {counts}>", escape(&suite));
    if document.defects.is_empty() {
        out.push_str("    <testcase classname=\"shall\" name=\"no defects\" time=\"0\"/>\n");
    }
    for defect in &document.defects {
        let path = escape(defect.location.path());
        let text = escape(&line(defect));
        let _ = writeln!(
            out,
            "    <testcase classname=\"{path}\" name=\"{text}\" file=\"{path}\" line=\"{}\" time=\"0\">",
            defect.location.line()
        );
        let message = escape(defect.message);
        let _ = match defect.severity {
            Severity::Error => writeln!(
                out,
                "      <failure type=\"error\" message=\"{message}\">{text}</failure>"
            ),
            Severity::Warning => writeln!(out, "      <skipped message=\"warning: {message}\"/>"),
        };
        out.push_str("    </testcase>\n");
    }
    out.push_str("  </testsuite>\n</testsuites>\n");
    out
}

/// Escapes text for XML content and attributes; characters that XML 1.0
/// does not allow become `?`.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c if c < ' ' || c == '\u{fffe}' || c == '\u{ffff}' => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use shall_core::config::Config;
    use shall_core::export::Input;
    use shall_core::junit::{self, Outcome};
    use shall_core::{markdown, trace};

    use super::*;

    fn document_texts(requirements: &str) -> (String, String) {
        let config =
            Config::parse("[[level]]\ntype = \"dsn\"\nname = \"D\"\nneeds = [\"impl\"]\n").unwrap();
        let parsed = markdown::parse(Path::new("reqs/a.md"), requirements).unwrap();
        let trace = trace::trace(&parsed.items, &config);
        let mut diagnostics = parsed.diagnostics.clone();
        diagnostics.extend(trace.defects.iter().cloned());
        // As `shall check` sorts them.
        diagnostics.sort_by(|a, b| a.location.cmp(&b.location));
        let document = Document::new(Input {
            config: &config,
            items: &parsed.items,
            trace: &trace,
            tests: None,
            diagnostics: &diagnostics,
            variant: None,
            build: None,
        });
        (text(&document), junit(&document))
    }

    #[test]
    fn writes_errors_and_warnings_as_shall_check_does() {
        let (text, xml) = document_texts("# A <&>\n`dsn~a-k4f2~1`\nNeds: x\n");
        assert_eq!(
            text,
            "reqs/a.md:2: dsn~a-k4f2~1: not covered by required type 'impl'\n\
             reqs/a.md:3: warning: 'Neds:' is not a keyword, did you mean 'Needs:'? \
             The line is read as text\n"
        );
        // The report reads back as JUnit XML.
        let parsed = junit::parse(Path::new("junit.xml"), &xml).unwrap();
        assert_eq!(parsed.diagnostics, []);
        let outcomes: Vec<&Outcome> = parsed
            .results
            .iter()
            .map(|result| &result.outcome)
            .collect();
        assert!(matches!(outcomes[0], Outcome::Failed { .. }), "{xml}");
        assert!(matches!(outcomes[1], Outcome::Skipped), "{xml}");
        assert!(
            xml.contains("tests=\"2\" failures=\"1\" errors=\"0\" skipped=\"1\" time=\"0\""),
            "{xml}"
        );
    }

    #[test]
    fn without_defects_one_case_passes() {
        let (text, xml) = document_texts("");
        assert_eq!(text, "");
        let parsed = junit::parse(Path::new("junit.xml"), &xml).unwrap();
        assert_eq!(parsed.results.len(), 1);
        assert_eq!(parsed.results[0].outcome, Outcome::Passed);
    }

    #[test]
    fn escapes_for_xml() {
        assert_eq!(
            escape("a<b & \"c\" 'd'\n\u{1}"),
            "a&lt;b &amp; &quot;c&quot; &apos;d&apos;&#10;?"
        );
    }
}
