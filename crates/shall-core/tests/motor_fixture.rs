//! Reads the requirements of the motor fixture (`fixtures/motor`).

use std::fs;
use std::path::{Path, PathBuf};

use shall_core::config::Config;
use shall_core::{Item, check, markdown, trace};

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

#[test]
fn traces_requirement_defects() {
    let config = Config::parse(&fs::read_to_string(fixture().join("shall.toml")).unwrap()).unwrap();
    let trace = trace::trace(&read_requirements(), &config);
    let defects: Vec<_> = trace.defects.iter().map(ToString::to_string).collect();

    // The defects of the requirements from expected-defects.txt.
    let expected = fs::read_to_string(fixture().join("expected-defects.txt")).unwrap();
    let from_requirements: Vec<&str> = expected
        .lines()
        .filter(|line| line.starts_with("reqs/"))
        .collect();
    // Until code and test tags are read (M3), no design item is covered
    // by impl or utest.
    let mut expected: Vec<String> = from_requirements.iter().map(|s| s.to_string()).collect();
    for (line, id) in [
        (4, "dsn~motor.temp-monitor-r5t1~1"),
        (17, "dsn~motor.shutdown-signal-w9e4~1"),
        (30, "dsn~motor.relay-driver-b6j3~1"),
    ] {
        for typ in ["impl", "utest"] {
            expected.push(format!(
                "reqs/design.md:{line}: {id}: not covered by required type '{typ}'"
            ));
        }
    }
    expected.sort_by_key(|line| {
        let mut parts = line.splitn(3, ':');
        let path = parts.next().unwrap().to_owned();
        let number: u32 = parts.next().unwrap().parse().unwrap();
        (path, number)
    });
    assert_eq!(defects, expected);

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
    assert_eq!(status("swr~motor.estop-relay-c2v6~1"), (true, false));
}
