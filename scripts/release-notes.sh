#!/usr/bin/env bash
# Prints the release notes of a tag: all commits since the last final
# release before it, whichever tags exist by now, so that a re-run gives the
# same notes. Pre-releases are no boundary: a final release lists everything
# that is new in it.
#
#   scripts/release-notes.sh v0.3.0
set -euo pipefail
cd "$(dirname "$0")/.."

tag=${1:?usage: release-notes.sh <tag>}
commit=$(git rev-parse "refs/tags/$tag^{commit}")
if previous=$(git describe --tags --abbrev=0 --match 'v[0-9]*' --exclude '*-*' "$commit^" 2>/dev/null); then
  scope=("$previous..$tag")
else
  # The first release has no start for a range: it takes all that HEAD has,
  # and a range from the first commit would leave that commit out.
  [ "$(git rev-parse HEAD)" = "$commit" ] || {
    echo "$tag: the first release needs its tag checked out" >&2
    exit 1
  }
  scope=(--ignore-tags '.*')
fi

notes=$(git cliff "${scope[@]}" --tag "$tag" --strip all)
grep -qF "## [${tag#v}] - " <<<"$notes" || {
  echo "$tag: no commits for the release notes" >&2
  exit 1
}
printf '%s\n' "$notes"
