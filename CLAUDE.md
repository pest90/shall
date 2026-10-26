# shall

`shall` ("Specifications Hooked to Actual Lines of Logic") is an open-source
requirements-as-code traceability tool written in Rust. Requirements live as
Markdown (later also Typst) files in Git; `shall` links them across levels down
to code and tests and produces an HTML/PDF report.

The full concept and implementation plan (German) is in `docs/konzept.md`.
Read the relevant section before starting a work package, and tick off
work packages there (`- [x]`) when they are done.

## Fixed decisions

- Language: Rust, edition 2024 (newest stable edition). Single static binary.
- Toolchain pinned in `rust-toolchain.toml` (currently `1.99`), bumped
  deliberately in its own commit. MSRV in `Cargo.toml` via `rust-version`,
  policy: latest stable minus two releases; CI tests both MSRV and stable.
- Git is the single source of truth. No DOORS integration.
- Requirement syntax is OpenFastTrace-compatible (`typ~name~revision`,
  `Covers:`, `Needs:`, `Depends:`, tags like `[impl->dsn~x~1]`), extended by
  `Satisfied-by:`, `Params:`, `Features:`, `Source:`.
- IDs are generated (`shall new`), format `typ~slug-random~rev`, never renamed,
  never derived from content or position, never reused.
- Levels are configurable in `shall.toml`; L1/L2/L3/L3D and
  capability/L3/L3D are templates, not hard-coded.
- Monorepo by default; internal imports always follow the current state.
  Fixed `rev` only for external components.
- `shall check` never blocks by itself; check modes `report` (default),
  `new`, `mature`, `strict` decide the exit code.
- Output is deterministic static files (`site/`); hosting is the user's choice.
- Extensibility: configuration, process plugins (JSON over stdin/stdout),
  `shall-core` as a library. No native dynamic plugins.
- License: MIT. Contributions via DCO (`Signed-off-by`).
- Project language: English (code, CLI messages, docs, commit messages).

## Workspace layout (target, see work package 10.1)

```
crates/
  shall-core/     # model, parsers, graph, tracer (no CLI/report deps)
  shall-report/   # HTML/PDF report
  shall-plugin/   # plugin protocol types
  shall-cli/      # binary `shall`
fixtures/motor/   # sample project with deliberate defects (work package 0.2)
```

## Where to start

M0 in `docs/konzept.md`: 0.1 skeleton together with 10.1 crate layout, then
0.2 fixture, 0.3 data model. Then M1 (Markdown import) and M2 (linker,
coverage, console report = MVP).

## Conventions

- `cargo fmt`, `cargo clippy -- -D warnings` and `cargo test` must pass;
  `scripts/check.sh` runs all of them plus the MSRV build. Docker:
  `docker compose run --rm check` (see `CONTRIBUTING.md`).
- Keep the `Dockerfile` in sync with the local environment: every system
  package or toolchain installed locally to build, run or test the
  project (apt packages, Rust toolchains or components, `cargo install`
  tools) goes into the `Dockerfile` in the same change. A toolchain bump
  needs no change there: the image reads `rust-toolchain.toml` and
  `rust-version` itself.
- Snapshot tests with `insta`; integration test against
  `fixtures/motor/expected-defects.txt`.
- Diagnostics format: `path:line: message`.
- Keep `shall-core` free of I/O-heavy dependencies where possible.

## Commit messages

Conventional Commits 1.0.0, see `CONTRIBUTING.md` for the full rules.

```
<type>(<scope>): <summary>

<body: what and why, wrapped at 72>

Plan: M2.3
Closes #42
```

- Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`,
  `chore`, `revert`. Breaking change: `!` after the scope plus a
  `BREAKING CHANGE:` footer.
- Scopes: `core`, `md`, `typst`, `scan`, `junit`, `trace`, `report`, `pdf`,
  `cli`, `plugin`, `build-check`, `deps`, `ci`.
- Summary: imperative, lower case, no trailing period, at most 72 characters.
- `Plan:` footer names the work package from `docs/konzept.md`.
- One logical change per commit. Do not add `Signed-off-by` yourself: the
  human author signs off (DCO) with `git commit -s`.
- Pull requests are squash-merged only, so the PR title must follow these
  rules. Never force-push a branch whose PR is under review; merge `master`
  into it instead (stacked branches: see `CONTRIBUTING.md`).
- The default branch is `master`. Branch names: `<kind>/<short-description>`
  with kind `feature`, `fix`, `docs`, `refactor` or `chore`, lower case and
  kebab-case, e.g. `feature/typst-import`, `fix/42-gtest-param-macros`.
  Branch off `master`.
- Releases are annotated SemVer tags on `master` (`v0.3.0`, `v0.3.0-rc.1`),
  no release branches. Never move or delete a pushed tag. Release steps:
  `CONTRIBUTING.md`.
