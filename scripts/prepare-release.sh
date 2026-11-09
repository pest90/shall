#!/usr/bin/env bash
# Prepares a release commit: sets the workspace version, updates Cargo.lock
# and regenerates CHANGELOG.md. Run on a clean `chore/release-<version>`
# branch; the steps around it are in CONTRIBUTING.md, "Releases".
#
#   scripts/prepare-release.sh            # version from the commits
#   scripts/prepare-release.sh 0.3.0-rc.1 # or a given one
#
# Needs git-cliff, which the `dev` container has. Commits nothing. A refusal
# or a failing cargo leaves the files as they were.
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
# Whether SemVer version $1 is lower than $2. Right for pre-releases like
# `rc.2` and `beta.10`; `sort -V` is not SemVer for ones that mix them with
# words, like `alpha.beta`.
lower_than() {
  local a=${1%%-*} b=${2%%-*}
  if [ "$a" != "$b" ]; then
    [ "$(printf '%s\n%s\n' "$a" "$b" | sort -V | head -n 1)" = "$a" ]
  elif [ "$1" = "$a" ]; then
    false # a final release is not lower than its pre-releases
  elif [ "$2" = "$b" ]; then
    true
  else
    [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -n 1)" = "$1" ]
  fi
}

# Whether the current version is released: a clone can lack the tags, but a
# release commit always leaves its changelog section.
released=$(git tag --list 'v[0-9]*')$(grep -sF "## [$current] - " CHANGELOG.md || true)
# Without a commit to release, git-cliff repeats the current version. The
# first release has the version that the workspace started with.
if [ "$version" = "$current" ] && [ -n "$released" ]; then
  echo "$version is the current version: nothing to release" >&2
  exit 1
fi
if git rev-parse --quiet --verify "refs/tags/v$version" >/dev/null; then
  echo "v$version exists already" >&2
  exit 1
fi
# git-cliff looks at final releases only, so after a pre-release with a version
# of your choice, or in a clone without tags, it can suggest a lower one. An
# unreleased current version can still get pre-releases of its own.
if lower_than "$version" "$current" && { [ -n "$released" ] || [ "${version%%-*}" != "$current" ]; }; then
  echo "$version is lower than the current version $current: give a higher one" >&2
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

cargo update --workspace --quiet || {
  git checkout -- Cargo.toml Cargo.lock
  exit 1
}
cp "$changelog" CHANGELOG.md

echo "prepared v$version: review with 'git diff', then commit"
