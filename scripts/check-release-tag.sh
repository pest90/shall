#!/usr/bin/env bash
# Fails unless a tag may be released: an annotated SemVer tag on `master`
# whose version is the one in Cargo.toml and CHANGELOG.md. The release
# workflow runs this before anything is built or published.
#
#   scripts/check-release-tag.sh v0.3.0
set -euo pipefail
cd "$(dirname "$0")/.."

tag=${1:?usage: check-release-tag.sh <tag>}
failed=0
fail() {
  echo "$tag: $1" >&2
  failed=1
}

[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || fail "not of the form v1.2.3 or v1.2.3-rc.1"
version=${tag#v}

[ "$(git cat-file -t "refs/tags/$tag" 2>/dev/null)" = tag ] || fail "not an annotated tag"

git merge-base --is-ancestor "refs/tags/$tag^{commit}" origin/master 2>/dev/null ||
  fail "not on origin/master"

workspace=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
[ "$workspace" = "$version" ] || fail "Cargo.toml has version $workspace"

# Every version with which the crates depend on each other.
while read -r line; do
  [[ $line == *"version = \"$version\""* ]] || fail "Cargo.toml has $line"
done < <(grep '^shall-[a-z]* = ' Cargo.toml)

grep -qF "## [$version] - " CHANGELOG.md 2>/dev/null || fail "no section for $version in CHANGELOG.md"

[ "$failed" = 0 ] && echo "$tag: ok"
exit "$failed"
