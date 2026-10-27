//! Test results from JUnit XML files, as written by gtest
//! (`--gtest_output=xml`), `cargo nextest` and most other test runners.
//!
//! Every `<testcase>` gives one [`TestResult`] with its class name, name
//! and outcome. A `<failure>` or `<error>` child makes it failed, a
//! `<skipped>` child, gtest's `status="notrun"` (disabled tests) or
//! `result="skipped"` (`GTEST_SKIP` before gtest 1.11) makes it skipped;
//! otherwise it passed. Reruns that nextest records as `<flakyFailure>`
//! or `<rerunFailure>` do not change the outcome. A test case without or
//! with an empty `classname` takes the name of the enclosing
//! `<testsuite>`.
//!
//! Diagnostics: invalid XML, also a file that ends before its elements
//! are closed, is an error at its line; the results read up to there are
//! kept, in a file that ends early also the test case cut off if it
//! already failed or was skipped. A file without a `<testsuites>` or
//! `<testsuite>` root and an attribute that cannot be decoded get a
//! warning. A failure that gtest
//! records outside of any test (a test case with an empty name, e.g. for
//! a failing `SetUpTestSuite`) is an error, since no tag can match it.

use std::path::Path;

use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use serde::Serialize;

use crate::diagnostic::Diagnostic;
use crate::item::{Location, LocationError};

/// The result of one test case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestResult {
    /// The `classname` attribute, e.g. `Shutdown` for gtest's
    /// `TEST(Shutdown, DisablesPwm)` or the test binary for nextest.
    pub classname: String,
    /// The `name` attribute, e.g. `DisablesPwm` or `tests::shuts_down`.
    pub name: String,
    /// The `name` of the enclosing `<testsuite>`; for nextest the test
    /// binary like the class name, for libtest's own output `test`.
    pub suite: String,
    /// The `package` attribute of the enclosing `<testsuite>`, which
    /// libtest's own output sets to `test` and nextest leaves out.
    pub package: Option<String>,
    /// The `file` attribute that gtest writes: the source file of the
    /// test as passed to the compiler, possibly absolute.
    pub file: Option<String>,
    /// Whether the test passed.
    pub outcome: Outcome,
    /// Where the `<testcase>` element is in the XML file.
    pub location: Location,
}

/// Outcome of a test case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Outcome {
    /// The test ran without failure.
    Passed,
    /// The test failed (`<failure>`) or ended with an error (`<error>`).
    Failed {
        /// The `message` attribute of the failure, if any.
        message: Option<String>,
    },
    /// The test did not run (`<skipped>`, or gtest `status="notrun"`).
    Skipped,
}

/// Results and diagnostics read from one file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The test cases, in file order.
    pub results: Vec<TestResult>,
    /// Problems found in the file; after invalid XML, the results read
    /// up to the error are kept.
    pub diagnostics: Vec<Diagnostic>,
}

/// Reads the test results of a JUnit XML file. `path` is relative to the
/// project root.
pub fn parse(path: &Path, text: &str) -> Result<Parsed, LocationError> {
    let file = Location::new(path, 1)?;
    let mut parsed = Parsed::default();
    let mut reader = Reader::from_str(text);
    reader.config_mut().expand_empty_elements = true;
    // Name and package of the enclosing test suites.
    let mut suites: Vec<(String, Option<String>)> = Vec::new();
    let mut case: Option<TestResult> = None;
    let mut depth = 0usize;
    let mut root = false;
    loop {
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(error) => {
                let line = line_at(text, reader.error_position());
                parsed.diagnostics.push(Diagnostic::error(
                    file.with_line(line),
                    format!("invalid XML: {error}"),
                ));
                break;
            }
        };
        match event {
            Event::Start(start) => {
                let location =
                    file.with_line(line_at(text, start_of_tag(text, reader.buffer_position())));
                depth += 1;
                let tag = start.local_name();
                let tag = tag.as_ref();
                if depth == 1 {
                    if tag != "testsuites" && tag != "testsuite" {
                        parsed.diagnostics.push(Diagnostic::warning(
                            location,
                            format!("not a JUnit XML file: root element is <{tag}>"),
                        ));
                        return Ok(parsed);
                    }
                    root = true;
                }
                let mut get = |name: &str| attribute(&start, name, &location, &mut parsed);
                match tag {
                    "testsuite" => {
                        let name = get("name").unwrap_or_default();
                        suites.push((name, get("package")));
                    }
                    "testcase" if case.is_none() => {
                        let Some(name) = get("name") else {
                            parsed.diagnostics.push(Diagnostic::warning(
                                location,
                                "test case without a name is ignored",
                            ));
                            continue;
                        };
                        let (suite, package) = suites.last().cloned().unwrap_or_default();
                        let classname = get("classname")
                            .filter(|classname| !classname.is_empty())
                            .unwrap_or_else(|| suite.clone());
                        let skipped = get("status").as_deref() == Some("notrun")
                            || get("result").as_deref() == Some("skipped");
                        let file = get("file");
                        case = Some(TestResult {
                            classname,
                            name,
                            suite,
                            package,
                            file,
                            outcome: if skipped {
                                Outcome::Skipped
                            } else {
                                Outcome::Passed
                            },
                            location,
                        });
                    }
                    "failure" | "error" => {
                        let message = get("message");
                        if let Some(case) = &mut case
                            && !matches!(case.outcome, Outcome::Failed { .. })
                        {
                            case.outcome = Outcome::Failed { message };
                        }
                    }
                    "skipped" => {
                        if let Some(case) = &mut case
                            && case.outcome == Outcome::Passed
                        {
                            case.outcome = Outcome::Skipped;
                        }
                    }
                    _ => {}
                }
            }
            Event::End(end) => {
                depth = depth.saturating_sub(1);
                match end.local_name().as_ref() {
                    "testsuite" => {
                        suites.pop();
                    }
                    "testcase" => {
                        if let Some(case) = case.take() {
                            outside_failure(&case, &mut parsed.diagnostics);
                            parsed.results.push(case);
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => {
                if depth > 0 {
                    // Keep a failure or skip of the test that was cut off,
                    // e.g. by a crashed run; without one, its outcome is
                    // unknown.
                    if let Some(case) = case.take().filter(|case| case.outcome != Outcome::Passed) {
                        outside_failure(&case, &mut parsed.diagnostics);
                        parsed.results.push(case);
                    }
                    let line = line_at(text, text.len() as u64);
                    parsed.diagnostics.push(Diagnostic::error(
                        file.with_line(line),
                        "invalid XML: the file ends before its elements are closed",
                    ));
                } else if !root {
                    parsed.diagnostics.push(Diagnostic::warning(
                        file,
                        "not a JUnit XML file: no root element",
                    ));
                }
                break;
            }
            _ => {}
        }
    }
    Ok(parsed)
}

/// Reports a failure that is not part of a test, which gtest writes as a
/// test case with an empty name.
fn outside_failure(case: &TestResult, diagnostics: &mut Vec<Diagnostic>) {
    if !case.name.is_empty() {
        return;
    }
    if let Outcome::Failed { message } = &case.outcome {
        let first = message
            .as_deref()
            .and_then(|message| message.lines().next())
            .unwrap_or_default();
        let suite = if case.classname.is_empty() {
            String::new()
        } else {
            format!(" in {}", case.classname)
        };
        diagnostics.push(Diagnostic::error(
            case.location.clone(),
            format!("test failure outside of any test{suite}: {first}")
                .trim_end_matches([' ', ':'])
                .to_owned(),
        ));
    }
}

/// Value of an attribute, with entities replaced; an attribute that
/// cannot be decoded gets a warning and counts as missing.
fn attribute(
    start: &BytesStart,
    name: &str,
    location: &Location,
    parsed: &mut Parsed,
) -> Option<String> {
    let value = match start.try_get_attribute(name) {
        Ok(Some(attribute)) => attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map(|value| value.into_owned()),
        Ok(None) => return None,
        Err(error) => Err(error.into()),
    };
    match value {
        Ok(value) => Some(value),
        Err(error) => {
            parsed.diagnostics.push(Diagnostic::warning(
                location.clone(),
                format!("cannot read attribute '{name}': {error}"),
            ));
            None
        }
    }
}

/// Byte offset of the `<` that opens the tag ending before `end`.
fn start_of_tag(text: &str, end: u64) -> u64 {
    let end = usize::try_from(end).unwrap_or(text.len()).min(text.len());
    text.as_bytes()[..end]
        .iter()
        .rposition(|&byte| byte == b'<')
        .unwrap_or(0) as u64
}

/// 1-based line of a byte offset.
fn line_at(text: &str, offset: u64) -> u32 {
    let offset = usize::try_from(offset)
        .unwrap_or(text.len())
        .min(text.len());
    let newlines = text.as_bytes()[..offset]
        .iter()
        .filter(|&&byte| byte == b'\n')
        .count();
    u32::try_from(newlines + 1).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(text: &str) -> Parsed {
        parse(Path::new("results.xml"), text).unwrap()
    }

    fn summary(parsed: &Parsed) -> Vec<String> {
        parsed
            .results
            .iter()
            .map(|result| {
                let outcome = match &result.outcome {
                    Outcome::Passed => "passed".to_owned(),
                    Outcome::Failed { message } => format!("failed {message:?}"),
                    Outcome::Skipped => "skipped".to_owned(),
                };
                format!(
                    "{}: {} {} {outcome}",
                    result.location, result.classname, result.name
                )
            })
            .collect()
    }

    #[test]
    fn reads_gtest_output() {
        let parsed = parse_str(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites tests="4" name="AllTests">
  <testsuite name="Shutdown" tests="3">
    <testcase name="Passes" status="run" result="completed" classname="Shutdown" />
    <testcase name="Fails" status="run" classname="Shutdown">
      <failure message="a.cpp:9&#x0A;Value of: x" type=""><![CDATA[a.cpp:9]]></failure>
      <failure message="second" type=""/>
    </testcase>
    <testcase
        name="DISABLED_Later" status="notrun" result="suppressed" classname="Shutdown" />
    <testcase name="Skips" status="run" result="skipped" classname="Shutdown">
      <skipped message="not today"/>
    </testcase>
  </testsuite>
</testsuites>
"#,
        );
        assert_eq!(parsed.diagnostics, []);
        assert_eq!(
            summary(&parsed),
            [
                "results.xml:4: Shutdown Passes passed",
                "results.xml:5: Shutdown Fails failed Some(\"a.cpp:9\\nValue of: x\")",
                "results.xml:9: Shutdown DISABLED_Later skipped",
                "results.xml:11: Shutdown Skips skipped",
            ]
        );
    }

    #[test]
    fn reads_nextest_output() {
        let parsed = parse_str(
            r#"<testsuites name="nextest-run" tests="3" failures="1" errors="0">
<testsuite name="shall-core" tests="3">
<testcase name="id::tests::parses" classname="shall-core" timestamp="x" time="0.001">
<flakyFailure message="first run failed" type="test failure"/>
<system-out>out</system-out>
</testcase>
<testcase name="id::tests::panics" classname="shall-core"><error message="panicked" type="test failure"/></testcase>
<testcase name="id::tests::ignored" classname="shall-core"><skipped/></testcase>
</testsuite>
</testsuites>"#,
        );
        assert_eq!(
            summary(&parsed),
            [
                "results.xml:3: shall-core id::tests::parses passed",
                "results.xml:7: shall-core id::tests::panics failed Some(\"panicked\")",
                "results.xml:8: shall-core id::tests::ignored skipped",
            ]
        );
    }

    #[test]
    fn takes_the_suite_name_without_classname() {
        let parsed = parse_str(
            "<testsuite name=\"outer\"><testsuite name=\"inner\"><testcase name=\"a\"/></testsuite>\
             <testcase name=\"b\"/></testsuite>",
        );
        assert_eq!(
            summary(&parsed),
            [
                "results.xml:1: inner a passed",
                "results.xml:1: outer b passed"
            ]
        );
    }

    #[test]
    fn reports_invalid_files_and_keeps_results_so_far() {
        let parsed = parse_str(
            "<testsuite>\n<testcase name=\"a\" classname=\"S\"/>\n<testcase name=\"b\">\n</testsuit>\n",
        );
        assert_eq!(summary(&parsed), ["results.xml:2: S a passed"]);
        assert_eq!(parsed.diagnostics.len(), 1);
        let message = parsed.diagnostics[0].to_string();
        assert!(
            message.starts_with("results.xml:4: invalid XML: "),
            "{message}"
        );

        let parsed = parse_str("<html>\n<testcase name=\"a\"/></html>");
        assert_eq!(parsed.results, []);
        assert_eq!(
            parsed.diagnostics[0].to_string(),
            "results.xml:1: not a JUnit XML file: root element is <html>"
        );

        let parsed = parse_str("<testsuite>\n<testcase classname=\"S\"/></testsuite>");
        assert_eq!(parsed.results, []);
        assert_eq!(
            parsed.diagnostics[0].to_string(),
            "results.xml:2: test case without a name is ignored"
        );
    }

    #[test]
    fn reports_truncated_and_empty_files() {
        let parsed = parse_str(
            "<testsuites>\n<testsuite name=\"S\">\n<testcase name=\"a\" classname=\"S\"/>\n<testcase name=\"b\"><failure message=\"x\">",
        );
        let open =
            parse_str("<testsuite>\n<testcase name=\"b\" classname=\"S\">\n<system-out>partial");
        assert_eq!(open.results, []);
        assert_eq!(open.diagnostics.len(), 1);
        assert_eq!(
            summary(&parsed),
            [
                "results.xml:3: S a passed",
                "results.xml:4: S b failed Some(\"x\")"
            ]
        );
        assert_eq!(
            parsed.diagnostics[0].to_string(),
            "results.xml:4: invalid XML: the file ends before its elements are closed"
        );
        for text in ["", "  \n", "<?xml version=\"1.0\"?>\n"] {
            let parsed = parse_str(text);
            assert_eq!(
                parsed.diagnostics[0].to_string(),
                "results.xml:1: not a JUnit XML file: no root element",
                "{text:?}"
            );
        }
    }

    #[test]
    fn handles_old_gtest_skips_empty_classnames_and_bad_attributes() {
        let parsed = parse_str(
            "<testsuite name=\"S\">\n\
             <testcase name=\"a\" classname=\"\" status=\"run\" result=\"skipped\"/>\n\
             <testcase name=\"b\" classname=\"X&bogus;\"/>\n\
             <testcase name=\"&bogus;\"/>\n\
             </testsuite>",
        );
        assert_eq!(
            summary(&parsed),
            ["results.xml:2: S a skipped", "results.xml:3: S b passed"]
        );
        let messages: Vec<String> = parsed.diagnostics.iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 3, "{messages:?}");
        assert!(
            messages[0].starts_with("results.xml:3: cannot read attribute 'classname': "),
            "{messages:?}"
        );
        assert!(
            messages[1].starts_with("results.xml:4: cannot read attribute 'name': "),
            "{messages:?}"
        );
        assert_eq!(
            messages[2],
            "results.xml:4: test case without a name is ignored"
        );
    }

    #[test]
    fn reports_failures_outside_of_tests() {
        let parsed = parse_str(
            "<testsuite name=\"Fixture\">\n\
             <testcase name=\"\" classname=\"\"><failure message=\"SetUpTestSuite failed&#x0A;details\"/></testcase>\n\
             <testcase name=\"\" classname=\"\"/>\n\
             </testsuite>",
        );
        assert_eq!(parsed.results.len(), 2);
        assert_eq!(
            parsed
                .diagnostics
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["results.xml:2: test failure outside of any test in Fixture: SetUpTestSuite failed"]
        );
    }

    #[test]
    fn keeps_the_gtest_file() {
        let parsed = parse_str(
            "<testsuite><testcase name=\"a\" classname=\"S\" file=\"t/a.cpp\"/></testsuite>",
        );
        assert_eq!(parsed.results[0].file.as_deref(), Some("t/a.cpp"));
    }
}
