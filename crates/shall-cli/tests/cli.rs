//! Smoke tests for the `shall` binary.

use std::process::Command;

fn shall() -> Command {
    Command::new(env!("CARGO_BIN_EXE_shall"))
}

#[test]
fn prints_version() {
    let output = shall().arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.trim(),
        format!("shall {}", env!("CARGO_PKG_VERSION"))
    );
}
