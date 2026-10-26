//! Reads the requirements of the motor fixture (`fixtures/motor`).

use std::fs;
use std::path::{Path, PathBuf};

use shall_core::{Item, check, markdown};

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
