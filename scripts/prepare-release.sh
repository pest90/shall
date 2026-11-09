#!/usr/bin/env bash
# Prepares a release commit: sets the workspace version, updates Cargo.lock
# and regenerates CHANGELOG.md. Run on a clean `chore/release-<version>`
# branch; the steps around it are in CONTRIBUTING.md, "Releases".
#
#   scripts/prepare-release.sh            # version from the commits
#   scripts/prepare-release.sh 0.3.0-rc.1 # or a given one
#
# Needs git-cliff, which the `dev` container has. Commits nothing.
set -euo pipefail
cd "$(dirname "$0")/.."

[ -z "$(git status --porcelain)" ] || {
  echo "the working tree is not clean" >&2
  exit 1
}

version=${1:-$(git cliff --bumped-version)}
version=${version#v}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || {
  echo "not a SemVer version: $version" >&2
  exit 1
}

# The version of the workspace, and those with which the crates depend on
# each other: crates.io needs them.
sed -i.bak \
  -e '/^\[workspace\.package\]/,/^\[/s/^version = ".*"/version = "'"$version"'"/' \
  -e '/^shall-[a-z]* = /s/version = "[^"]*"/version = "'"$version"'"/' \
  Cargo.toml
rm Cargo.toml.bak

cargo update --workspace --quiet
git cliff --tag "v$version" --output CHANGELOG.md

echo "prepared v$version: review with 'git diff', then commit"
