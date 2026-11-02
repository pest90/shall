#!/usr/bin/env bash
# Runs every check a pull request has to pass: formatting, clippy, tests
# and rustdoc without warnings on the pinned toolchain, clippy for Windows
# and macOS, so code behind `cfg(unix)` cannot break those builds, and a
# build on the MSRV.
set -euo pipefail
cd "$(dirname "$0")/.."

msrv=$(sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml)
# The cross-platform targets, from `targets = [...]` in rust-toolchain.toml.
targets=$(sed -n 's/^targets = \[\(.*\)\]$/\1/p' rust-toolchain.toml | tr -d '",')
[ -n "$targets" ] || { echo "no targets in rust-toolchain.toml" >&2; exit 1; }

set -x
# The report embeds a copy of the logo; it must not drift from the asset.
cmp assets/logo/shall-logo.svg crates/shall-report/templates/logo.svg
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
for target in $targets; do
  cargo clippy --workspace --all-targets --locked --target "$target" -- -D warnings
done
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo "+$msrv" check --workspace --all-targets --locked
