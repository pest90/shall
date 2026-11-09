#!/usr/bin/env bash
# Prepares a release commit: sets the workspace version, updates Cargo.lock
# and regenerates CHANGELOG.md. Run on a clean `chore/release-<version>`
# branch; the steps around it are in CONTRIBUTING.md, "Releases".
#
#   scripts/prepare-release.sh            # version from the commits
#   scripts/prepare-release.sh 0.3.0-rc.1 # or a given one
#
# Needs git-cliff, which the `dev` container has. Commits nothing, and
# changes nothing if it fails before the version is set.
set -euo pipefail
cd "$(dirname "$0")/.."

command -v git-cliff >/dev/null || {
  echo "git-cliff is missing: run this in the dev container" >&2
  exit 1
}

# In two steps: inside the test of an `if`, a failing git would pass.
status=$(git status --porcelain)
[ -z "$status" ] || {
  echo "the working tree is not clean" >&2
  exit 1
}

current=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
version=${1:-$(git cliff --bumped-version)}
version=${version#v}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || {
  echo "not a SemVer version: $version" >&2
  exit 1
}
# Without a commit to release, git-cliff repeats the current version. The
# first release has the version that the workspace started with.
if [ "$version" = "$current" ] && [ -n "$(git tag --list 'v[0-9]*')" ]; then
  echo "$version is the current version: nothing to release" >&2
  exit 1
fi

changelog=$(mktemp)
trap 'rm -f "$changelog"' EXIT
git cliff --tag "v$version" --output "$changelog"
grep -qF "## [$version] - " "$changelog" || {
  echo "no commits for the changelog of $version" >&2
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
cp "$changelog" CHANGELOG.md

echo "prepared v$version: review with 'git diff', then commit"
