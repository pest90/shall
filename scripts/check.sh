#!/usr/bin/env bash
# Runs every check a pull request has to pass: formatting, clippy, tests
# on the pinned toolchain and a build on the MSRV.
set -euo pipefail
cd "$(dirname "$0")/.."

msrv=$(sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml)

set -x
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo "+$msrv" check --workspace --all-targets --locked
