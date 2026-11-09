#!/usr/bin/env bash
# Tests the release scripts in a throwaway repository: a first release, a
# pre-release, the final release after it, a later one, and what they must
# refuse. The dry run of release.yml runs this, because nothing else runs
# these scripts before a real tag, which could not be taken back.
#
# Needs git-cliff and cargo, which the `dev` container has.
set -euo pipefail
source_dir=$(cd "$(dirname "$0")/.." && pwd)

bash -n "$source_dir"/scripts/*.sh

repo=$(mktemp -d)
trap 'rm -rf "$repo"' EXIT
cd "$repo"
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.org \
  GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.org

die() {
  echo "FAILED: $1" >&2
  exit 1
}
commit() {
  git add -A
  git commit --quiet --allow-empty -m "$1"
}
# Commits a prepared release, tags it and moves master to it.
release() {
  commit "chore(release): $1"
  git tag -a "$1" -m "shall $1"
  git update-ref refs/remotes/origin/master HEAD
  scripts/check-release-tag.sh "$1" >/dev/null || die "$1 is a good tag"
}
refuses() {
  local what=$1
  shift
  if "$@" >/dev/null 2>&1; then
    die "$what passed"
  fi
}

git init --quiet -b master
mkdir scripts crates crates/shall-core crates/shall-core/src
cp "$source_dir"/scripts/*.sh scripts/
cp "$source_dir"/cliff.toml "$source_dir"/rust-toolchain.toml .
cat >Cargo.toml <<'TOML'
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
shall-core = { path = "crates/shall-core", version = "0.1.0" }
other = { version = "1.0.0" }
TOML
cat >crates/shall-core/Cargo.toml <<'TOML'
[package]
name = "shall-core"
version.workspace = true
edition.workspace = true
TOML
: >crates/shall-core/src/lib.rs
sed -i.bak '/^other = /d' Cargo.toml && rm Cargo.toml.bak
cargo generate-lockfile --quiet
printf 'target/\n' >.gitignore
commit "feat(core): first feature"

# The first release takes the version that the workspace started with.
scripts/prepare-release.sh >/dev/null
grep -qF '## [0.1.0] - ' CHANGELOG.md || die "changelog of 0.1.0"
refuses "a dirty tree" scripts/prepare-release.sh
release v0.1.0
grep -qF 'first feature' <(scripts/release-notes.sh v0.1.0) || die "notes of 0.1.0"

# Nothing for users since then.
commit "ci: not for users"
refuses "a version without commits to release" scripts/prepare-release.sh
git diff --quiet || die "a refused release changed files"
refuses "a version without a changelog section" scripts/prepare-release.sh 0.1.1
git diff --quiet || die "a refused release changed files"

# A pre-release, then the final release without further changes.
commit "fix(cli): a fix"
commit "feat(cli): second feature"
scripts/prepare-release.sh 0.2.0-rc.1 >/dev/null
release v0.2.0-rc.1
notes=$(scripts/release-notes.sh v0.2.0-rc.1)
grep -qF 'second feature' <<<"$notes" || die "notes of the rc"
if grep -qF 'first feature' <<<"$notes"; then die "notes of the rc list 0.1.0"; fi

[ "$(git cliff --bumped-version 2>/dev/null)" = v0.2.0 ] || die "the final release after an rc"
scripts/prepare-release.sh >/dev/null
grep -q '^version = "0.2.0"$' Cargo.toml || die "version of the final release"
grep -qF 'version = "0.2.0"' <(grep '^shall-core' Cargo.toml) || die "dependency version"
release v0.2.0
notes=$(scripts/release-notes.sh v0.2.0)
grep -qF '## [0.2.0] - ' <<<"$notes" || die "section of the final release"
grep -qF 'second feature' <<<"$notes" || die "the final release lists what the rc had"
if grep -qF 'first feature' <<<"$notes"; then die "notes of 0.2.0 list 0.1.0"; fi

# A later tag does not change the notes of an earlier one, for a re-run.
commit "fix(cli): another fix"
scripts/prepare-release.sh >/dev/null
grep -q '^version = "0.2.1"$' Cargo.toml || die "a fix bumps the patch version"
release v0.2.1
grep -qF '## [0.2.0] - ' <(scripts/release-notes.sh v0.2.0) || die "re-run of the notes of 0.2.0"

# Tags that must not be released.
git tag v0.2.1-light
refuses "a lightweight tag" scripts/check-release-tag.sh v0.2.1-light
git tag -a v0.2.2 -m x
refuses "a tag with another version" scripts/check-release-tag.sh v0.2.2
commit "fix: off master"
git tag -a v0.2.1-off -m x
refuses "a tag off master" scripts/check-release-tag.sh v0.2.1-off

echo "release scripts: ok"
