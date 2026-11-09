#!/usr/bin/env bash
# Tests the release scripts in a throwaway repository: a first release, a
# pre-release, the final release after it, a later one, and what they must
# refuse. Every run of release.yml runs this before it builds anything, because
# nothing else runs these scripts before a real tag, which could not be taken
# back.
#
# Needs git-cliff and cargo, which the `dev` container has.
set -euo pipefail
source_dir=$(cd "$(dirname "$0")/.." && pwd)

for script in "$source_dir"/scripts/*.sh; do
  bash -n "$script"
done

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
  cargo metadata --locked --format-version 1 >/dev/null || die "Cargo.lock of $1"
}
clean() {
  [ -z "$(git status --porcelain)" ] || die "$1"
}
# Runs a command that must fail with the given message.
refuses() {
  local message=$1 output
  shift
  if output=$("$@" 2>&1); then
    die "passed, but should say '$message': $*"
  fi
  grep -qF "$message" <<<"$output" || die "'$*' should say '$message': $output"
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
cargo generate-lockfile --quiet
printf 'target/\n' >.gitignore
commit "feat(core): first feature"

# A pre-release as the very first release, then the final one. Undone at the
# end, because the first release below starts without any tag.
scripts/prepare-release.sh 0.1.0-rc.1 >/dev/null
release v0.1.0-rc.1
grep -qF 'first feature' <(scripts/release-notes.sh v0.1.0-rc.1) || die "notes of the first rc"
[ "$(git cliff --bumped-version 2>/dev/null)" = v0.1.0 ] || die "the final release after the first rc"
scripts/prepare-release.sh >/dev/null
git tag -d v0.1.0-rc.1 >/dev/null
git reset --quiet --hard HEAD~1
refuses "lower than" scripts/prepare-release.sh 0.0.9

# The first release takes the version that the workspace started with.
scripts/prepare-release.sh >/dev/null
grep -qF '## [0.1.0] - ' CHANGELOG.md || die "changelog of 0.1.0"
refuses "not clean" scripts/prepare-release.sh
release v0.1.0
grep -qF 'first feature' <(scripts/release-notes.sh v0.1.0) || die "notes of 0.1.0"

# Nothing for users since then.
commit "ci: not for users"
refuses "nothing to release" scripts/prepare-release.sh
clean "a refused release changed files"
refuses "no commits for the changelog" scripts/prepare-release.sh 0.1.1
clean "a refused release changed files"

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
grep -qF 'other = { version = "1.0.0" }' Cargo.toml || die "a third-party version changed"
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

# Tags that must not be released, each for the reason named. The state here:
# v0.2.1 is a good tag on origin/master.
retag() {
  git tag -d v0.2.1 >/dev/null
  git tag "$@" v0.2.1
}

retag
refuses "not an annotated tag" scripts/check-release-tag.sh v0.2.1
retag -a -m x

commit "fix: off master"
retag -a -m x
refuses "not on origin/master" scripts/check-release-tag.sh v0.2.1
git reset --quiet --hard HEAD~1
retag -a -m x

# v0.2.0 has its changelog section, but Cargo.toml is at 0.2.1.
refuses "Cargo.toml has version 0.2.1" scripts/check-release-tag.sh v0.2.0

sed -i.bak 's/^## \[0\.2\.1\]/## Removed/' CHANGELOG.md && rm CHANGELOG.md.bak
commit "docs: lose a changelog section"
retag -a -m x
git update-ref refs/remotes/origin/master HEAD
refuses "no section for 0.2.1" scripts/check-release-tag.sh v0.2.1
git reset --quiet --hard HEAD~1
retag -a -m x
git update-ref refs/remotes/origin/master HEAD

sed -i.bak 's/^\(shall-core = .*version = \)"0\.2\.1"/\1"0.2.0"/' Cargo.toml && rm Cargo.toml.bak
commit "chore: lose a dependency version"
retag -a -m x
git update-ref refs/remotes/origin/master HEAD
refuses "Cargo.toml has shall-core" scripts/check-release-tag.sh v0.2.1
git reset --quiet --hard HEAD~1
retag -a -m x
git update-ref refs/remotes/origin/master HEAD

git tag -a v1.2 -m x
refuses "not of the form" scripts/check-release-tag.sh v1.2

# Versions that must not be prepared.
commit "fix(cli): one more fix"
refuses "exists already" scripts/prepare-release.sh 0.2.0
refuses "lower than" scripts/prepare-release.sh 0.1.5
refuses "lower than" scripts/prepare-release.sh 0.2.1-rc.1
scripts/prepare-release.sh 0.3.0-rc.2 >/dev/null
release v0.3.0-rc.2
refuses "lower than" scripts/prepare-release.sh 0.3.0-rc.1
# git-cliff sees only a fix since v0.2.1 and suggests 0.2.2.
refuses "lower than" scripts/prepare-release.sh
clean "a refused release changed files"
scripts/prepare-release.sh 0.3.0-rc.10 >/dev/null
git checkout --quiet -- .
scripts/prepare-release.sh 0.3.0 >/dev/null
git checkout --quiet -- .

# Numbers, not text: 0.10.0 follows 0.9.0.
commit "fix(cli): a fix for 0.9"
scripts/prepare-release.sh 0.9.0 >/dev/null
release v0.9.0
commit "fix(cli): a fix for 0.10"
scripts/prepare-release.sh 0.10.0 >/dev/null
git checkout --quiet -- .

# A failing cargo leaves nothing behind, even if it changed Cargo.lock.
mkdir fake-bin
printf '#!/bin/sh\necho "# changed" >>Cargo.lock\necho "fake cargo failed" >&2\nexit 101\n' >fake-bin/cargo
chmod +x fake-bin/cargo
printf 'fake-bin/\n' >>.gitignore
commit "chore: ignore the fake cargo"
PATH="$PWD/fake-bin:$PATH" refuses "fake cargo failed" scripts/prepare-release.sh 0.10.0
clean "a failing cargo left changes"

echo "release scripts: ok"
