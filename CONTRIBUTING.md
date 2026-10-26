# Contributing to shall

Thanks for your interest in contributing! This document describes how
changes get into `shall`.

## Development environment

You can work with a local Rust toolchain (`rustup` picks the version
from `rust-toolchain.toml`) or entirely in Docker:

```sh
export UID GID=$(id -g)               # files you write belong to you
docker compose run --rm dev           # shell with the toolchain
docker compose run --rm dev cargo test
docker compose run --rm check         # all pull request checks
docker compose run --rm shall --version
```

`scripts/check.sh` runs the same checks as CI: formatting, clippy,
tests and a build on the MSRV. Without compose:

```sh
docker build --target check .         # fails if a check fails
docker build -t shall .               # minimal image with the binary
docker run --rm -u "$(id -u):$(id -g)" -v "$PWD:/project" shall check
```

## Pull requests

- Open an issue first for larger changes, so the design can be discussed.
- Keep one topic per pull request, and keep pull requests small.
- `cargo fmt`, `cargo clippy -- -D warnings` and `cargo test` must pass.
- `master` has a **linear history**. Pull requests are **squash-merged** only;
  merge commits and rebase-merging are disabled on `master`. The pull request
  title becomes the commit message on `master`, so it must follow the rules
  below. CI checks it.
- Inside your branch, work however you like: merging `master` into your
  branch, fixup commits and so on are fine, because they are squashed away
  when the pull request lands.

### Branch names

`<kind>/<short-description>`, lower case, words joined by `-`, optionally
with the issue number up front:

| Prefix | For | Example |
| --- | --- | --- |
| `feature/` | new functionality (`feat`) | `feature/typst-import` |
| `fix/` | bug fixes | `fix/42-gtest-param-macros` |
| `docs/` | documentation only | `docs/plugin-guide` |
| `refactor/` | restructuring without behaviour change | `refactor/tracer-graph` |
| `chore/` | tooling, dependencies, CI, housekeeping, release prep | `chore/release-0.3.0` |

Branches are created from `master` and deleted after their pull request is
merged. CI checks the name against
`^(feature|fix|docs|refactor|chore)/[a-z0-9][a-z0-9.-]*$`.

### No force pushes once review has started

Force pushes rewrite the history reviewers already looked at: comments can
become outdated and "changes since your last review" stops working. So:

- Before the first review you may rebase and force-push freely.
- Once a review has started, only add commits. To bring in changes from
  `master`, **merge** `master` into your branch instead of rebasing.

### Branches that build on other branches

If your branch `feature/report` builds on `feature/parser`, and
`feature/parser` was squash-merged as commit `S` on `master`, a plain
`git merge origin/master` can conflict: `master` contains the parser
changes as one squash commit, your branch contains them as the individual
commits of `feature/parser`.

Merge in four steps instead. Each is an ordinary merge, so no force push
is needed:

```sh
git fetch origin
git merge feature/parser    # 1. make sure you have its final state
git merge S~1               # 2. master as it was right before the squash
git merge -s ours S         # 3. mark the squash as merged; its changes
                            #    are already in your branch
git merge origin/master     # 4. everything that landed after S
git push
```

Step 3 is safe because `S` is exactly "master before the squash plus the
changes of `feature/parser`", and your branch already has both after
steps 1 and 2. Find `S` in the merged pull request or with
`git log origin/master --oneline`.

If the pull request for `feature/report` targeted `feature/parser`, change
its target to `master` after these steps. The merge commits disappear when
`feature/report` is squash-merged in turn.

Recommended setting, which remembers how you resolved a conflict and
reapplies it next time:

```sh
git config --global rerere.enabled true
```

## Commit messages

We use [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/).
The changelog and version numbers are generated from them. Because pull
requests are squash-merged, this applies above all to the **pull request
title**. Following it for the commits in your branch is welcome but not
required.

```
<type>(<scope>): <summary>

<body>

<footers>
```

### Type

| Type | Use for | Version bump |
| --- | --- | --- |
| `feat` | a new feature for users | minor |
| `fix` | a bug fix | patch |
| `perf` | a performance improvement | patch |
| `refactor` | code change without behaviour change | none |
| `docs` | documentation only | none |
| `test` | adding or fixing tests | none |
| `build` | build system, dependencies, packaging | none |
| `ci` | CI configuration | none |
| `chore` | anything else (tooling, housekeeping) | none |
| `revert` | reverting an earlier commit | depends |

A breaking change adds `!` after the scope, for example
`feat(cli)!: rename --out to --output`, and explains the change in a
`BREAKING CHANGE:` footer.

### Scope

The part of the project that changes:

`core`, `md`, `typst`, `scan`, `junit`, `trace`, `report`, `pdf`, `cli`,
`plugin`, `build-check`, `deps`, `ci`

Leave the scope out only if a change really spans the whole project.

### Summary

- Imperative mood: "add", not "added" or "adds".
- Lower case, no trailing period.
- At most 72 characters.
- Say what changes for the user, not which file you touched.

### Body

Optional for small changes. Explain **what** and **why**, not how. Wrap
lines at 72 characters.

### Footers

| Footer | Meaning |
| --- | --- |
| `Closes #42` / `Refs #42` | links an issue |
| `Plan: M2.3` | work package from `docs/konzept.md` |
| `BREAKING CHANGE: …` | describes an incompatible change |
| `Signed-off-by: Name <mail>` | Developer Certificate of Origin, see below |

### Examples

```
feat(trace): report outdated revisions as defects

A link to `l3~motor.overtemp-k4f2~1` now fails once the target is at
revision 2, so changed requirements cannot slip through unnoticed.

Plan: M2.1
Closes #17
Signed-off-by: Jane Doe <jane@example.org>
```

```
fix(scan): detect TEST_P macros in gtest files
```

```
feat(cli)!: rename --out to --output

BREAKING CHANGE: `shall report --out` is now `shall report --output`.
```

## Releases

Releases are annotated Git tags on `master` that follow
[Semantic Versioning](https://semver.org): `v<MAJOR>.<MINOR>.<PATCH>`, for
example `v0.3.0`, pre-releases as `v0.3.0-rc.1`. There are no release
branches.

The next version follows from the commits since the last tag:

| Commits since last tag | Bump | Before 1.0 |
| --- | --- | --- |
| only `fix` / `perf` | patch | patch |
| at least one `feat` | minor | minor |
| at least one breaking change (`!`) | major | minor |

How a release is made (maintainers):

1. Branch `chore/release-<version>` from `master`: bump the versions in
   `Cargo.toml`, regenerate `CHANGELOG.md` with `git-cliff`.
2. Open a pull request titled `chore(release): v<version>` and squash-merge it.
3. Tag that commit on `master` and push the tag:
   ```sh
   git tag -a v0.3.0 -m "shall v0.3.0"
   git push origin v0.3.0
   ```
4. The tag triggers CI: binaries for Linux, macOS and Windows, crates.io,
   container image and the release page with the changelog section.

Tags are never moved or deleted once pushed; a broken release is fixed by
a new patch release. Until 1.0, only the latest release receives fixes.

## Developer Certificate of Origin (DCO)

Every commit must be signed off, which certifies that you wrote the change
or have the right to submit it under the project's MIT license
([developercertificate.org](https://developercertificate.org)). Use
`git commit -s`, which adds:

```
Signed-off-by: Your Name <your@email>
```

The sign-off is a statement by you as a person. If you use an AI coding
assistant, you are still the one signing off and responsible for the change.
