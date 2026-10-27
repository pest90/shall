//! Reads and traces the motor fixture (`fixtures/motor`).

use std::fs;
use std::path::{Path, PathBuf};

use shall_core::config::Config;
use shall_core::results::Verdict;
use shall_core::{Item, Origin, check, junit, markdown, results, tags, trace};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/motor")
}

fn read_requirements() -> Vec<Item> {
    let root = fixture();
    let mut paths: Vec<PathBuf> = fs::read_dir(root.join("reqs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    paths.sort();
    let mut items = Vec::new();
    for path in paths {
        let text = fs::read_to_string(&path).unwrap();
        let relative = path.strip_prefix(&root).unwrap();
        let parsed = markdown::parse(relative, &text).unwrap();
        assert_eq!(
            parsed.diagnostics,
            [],
            "diagnostics in {}",
            relative.display()
        );
        items.extend(parsed.items);
    }
    items
}

#[test]
fn reads_all_items_with_location() {
    let items = read_requirements();
    let ids: Vec<_> = items
        .iter()
        .map(|item| format!("{} {}", item.location, item.id))
        .collect();
    assert_eq!(
        ids,
        [
            "reqs/design.md:4 dsn~motor.temp-monitor-r5t1~1",
            "reqs/design.md:17 dsn~motor.shutdown-signal-w9e4~1",
            "reqs/design.md:30 dsn~motor.relay-driver-b6j3~1",
            "reqs/software.md:4 swr~motor.overtemp-shutdown-k4f2~2",
            "reqs/software.md:20 swr~motor.overtemp-warning-p8n5~1",
            "reqs/software.md:33 swr~motor.estop-relay-c2v6~1",
            "reqs/system.md:4 sys~motor.thermal-protection-q7m2~1",
            "reqs/system.md:17 sys~motor.emergency-stop-h3x9~1",
        ]
    );
    assert!(check::duplicate_ids(&items).is_empty());
}

#[test]
fn parses_all_keywords() {
    insta::assert_yaml_snapshot!(read_requirements());
}

/// Items from the tags in the fixture's code and tests.
fn read_tags() -> Vec<Item> {
    let root = fixture();
    let mut paths = Vec::new();
    for dir in ["src", "tests"] {
        for entry in fs::read_dir(root.join(dir)).unwrap() {
            paths.push(entry.unwrap().path());
        }
    }
    paths.sort();
    let mut items = Vec::new();
    for path in paths {
        let text = fs::read_to_string(&path).unwrap();
        let scanned = tags::scan(path.strip_prefix(&root).unwrap(), &text).unwrap();
        assert_eq!(scanned.diagnostics, [], "{}", path.display());
        items.extend(scanned.items);
    }
    items
}

#[test]
fn reads_tags_with_test_names() {
    let tests: Vec<_> = read_tags()
        .iter()
        .map(|item| {
            let Origin::Tag { test, .. } = &item.origin else {
                panic!("not a tag")
            };
            format!(
                "{} {} {}",
                item.location,
                item.covers[0].id,
                test.as_ref().map_or("-".to_owned(), ToString::to_string)
            )
        })
        .collect();
    assert_eq!(
        tests,
        [
            "src/fault_log.cpp:3 dsn~motor.fault-log-z2d8~1 -",
            "src/relay_driver.cpp:3 dsn~motor.relay-driver-b6j3~1 -",
            "src/shutdown.cpp:3 dsn~motor.shutdown-signal-w9e4~1 -",
            "src/temp_monitor.cpp:3 dsn~motor.temp-monitor-r5t1~1 -",
            "tests/relay_driver_test.cpp:10 dsn~motor.relay-driver-b6j3~1 RelayDriverTest.OpensOnEstop",
            "tests/shutdown_test.cpp:5 dsn~motor.shutdown-signal-w9e4~1 Shutdown.DisablesPwmOnOvertemp",
            "tests/temp_monitor_test.cpp:5 dsn~motor.temp-monitor-r5t1~1 TempMonitor.RaisesFlagAbove85C",
        ]
    );
}

#[test]
fn traces_requirement_and_tag_defects() {
    let config = Config::parse(&fs::read_to_string(fixture().join("shall.toml")).unwrap()).unwrap();
    let mut items = read_requirements();
    items.extend(read_tags());
    let trace = trace::trace(&items, &config);
    let xml = Path::new("test-results/gtest.xml");
    let parsed = junit::parse(xml, &fs::read_to_string(fixture().join(xml)).unwrap()).unwrap();
    assert_eq!(parsed.diagnostics, []);
    let evaluation = results::evaluate(&items, &parsed.results, &config, &Default::default());
    let defects: Vec<_> = trace
        .defects
        .iter()
        .chain(&evaluation.defects)
        .map(ToString::to_string)
        .collect();

    let expected = fs::read_to_string(fixture().join("expected-defects.txt")).unwrap();
    assert_eq!(defects, expected.lines().collect::<Vec<_>>());

    let verdicts: Vec<_> = evaluation
        .runs
        .iter()
        .map(|run| (run.test.as_deref().unwrap(), run.verdict))
        .collect();
    assert_eq!(
        verdicts,
        [
            ("RelayDriverTest.OpensOnEstop", Verdict::Passed),
            ("Shutdown.DisablesPwmOnOvertemp", Verdict::Failed),
            ("TempMonitor.RaisesFlagAbove85C", Verdict::Passed),
        ]
    );

    let status = |id: &str| {
        let item = trace
            .items
            .iter()
            .find(|item| item.id.to_string() == id)
            .unwrap();
        (item.covered, item.deep_covered)
    };
    // Covered, but not deep: the uncovered warning requirement is below it.
    // The outdated link of the temperature monitor does not count at all.
    assert_eq!(status("sys~motor.thermal-protection-q7m2~1"), (true, false));
    assert_eq!(status("swr~motor.overtemp-warning-p8n5~1"), (false, false));
    // The emergency stop chain is complete down to code and tests.
    assert_eq!(status("sys~motor.emergency-stop-h3x9~1"), (true, true));
    assert_eq!(status("dsn~motor.temp-monitor-r5t1~1"), (true, true));
}
