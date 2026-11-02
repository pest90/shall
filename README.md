# shall

**Specifications Hooked to Actual Lines of Logic** – requirements as code.

`shall` traces requirements written in Markdown through all levels of a
project, from high-level requirements down to the code that implements
them and the tests that verify them, and reports the gaps: uncovered
requirements, outdated links, orphaned code and failing tests.

- Requirements live in Markdown files next to the code, in Git, and are
  reviewed in merge requests like any other change.
- The syntax is compatible with [OpenFastTrace](https://github.com/itsallcode/openfasttrace).
- Levels are configurable, e.g. system → software → design, and a
  single item can skip a level by forwarding.
- Planned: a single binary for Linux, macOS and Windows, deterministic
  output as static files, no server.

> **Status:** early development, not released yet. Today `shall` traces
> requirements, code and test tags and gtest or `cargo nextest` results,
> also across a workspace of projects that import a shared platform or
> external components at a fixed `rev`, and per product variant; prints the defects and fails by check mode
> (`shall check`, also as CI annotations or SARIF); writes an HTML report
> with coverage, verified status, a traffic light per chain, requirement
> details, a tree, the Green Graph and a defect list, plus `trace.json`,
> a badge and diagrams (`shall report`); creates new items with
> generated IDs (`shall new`) and projects from templates
> (`shall init`); shows the requirement changes between Git states
> (`shall diff`, `shall upgrade-check`); and exports the requirements for a cross-check with
> OpenFastTrace (`shall export --oft`). The PDF report,
> plugins and the build check are being built; see
> [the plan](docs/konzept.md) (German).

## Example

A software requirement that refines a system requirement and must be
covered by a design item:

```markdown
## Shut down the motor on overtemperature
`swr~motor.overtemp-shutdown-k4f2~2`

The software shuts down the motor within 100 ms when the winding
temperature exceeds 85 °C.

Covers:
- sys~motor.thermal-protection-q7m2~1

Needs: dsn
```

Code and tests point to the design items they implement or verify with
tags; test results come from JUnit XML:

```cpp
// [impl->dsn~motor.temp-monitor-r5t1~1]
void TempMonitor::run(float winding_temp_c) { /* ... */ }

// [utest->dsn~motor.temp-monitor-r5t1~1]
TEST(TempMonitor, RaisesFlagAbove85C) { /* ... */ }
```

## Getting started

Build from source with a Rust toolchain ([rustup](https://rustup.rs))
and Git, which `shall new` uses at runtime:

```sh
git clone <repository URL> shall
cd shall
cargo install --locked --path crates/shall-cli
```

Start a project from a template, with `shall.toml` and an example
chain of requirements (`--template simple`, `automotive`, `platform` or
`project`):

```sh
shall init --template simple
```

or describe your levels in `shall.toml` at the root of your project
yourself:

```toml
[[level]]
type  = "sys"
name  = "System Requirements"
needs = ["swr"]

[[level]]
type  = "swr"
name  = "Software Requirements"
needs = ["dsn"]

[[level]]
type  = "dsn"
name  = "Design"
needs = ["impl", "utest"]

[verification]
types = ["utest"]

[sources]
requirements = ["reqs/**/*.md"]
impl  = ["src/**/*.{cpp,hpp}"]
utest = ["tests/**/*.cpp"]
junit = ["build/test-results/**/*.xml"]
```

Create items with generated IDs instead of inventing them:

```sh
shall new swr "Warn before overtemperature" --component motor --file reqs/software.md
# reqs/software.md:16: created swr~motor.warn-before-overtemperature-p8n5~1
```

Check the project; every defect is a line `path:line: message`. For the
sample project in [`fixtures/motor`](fixtures/motor/README.md):

```sh
shall check
# reqs/design.md:12: dsn~motor.temp-monitor-r5t1~1: covers outdated revision swr~motor.overtemp-shutdown-k4f2~1 (current revision: 2)
# reqs/software.md:20: swr~motor.overtemp-warning-p8n5~1: not covered by required type 'dsn'
# src/fault_log.cpp:3: tag [impl->dsn~motor.fault-log-z2d8~1]: unknown target dsn~motor.fault-log-z2d8~1
# tests/shutdown_test.cpp:5: test Shutdown.DisablesPwmOnOvertemp failed
# not ok: 4 errors, 0 warnings (9 requirements, 7 tags, 3 test results)
```

`shall check` exits with 0 by default, so it can report from the start;
`[check] mode` makes it fail on new defects, on defects of mature items,
or, like `--mode strict` in a release job, on every defect.
`shall report --out site` writes the report `site/index.html`, a single
file that opens without a server, the whole traced project as
`site/trace.json`, the defects as `site/defects.txt` and, for CI test
reports, `site/junit.xml`, a badge `site/badge.svg`, and the project as
Graphviz and Mermaid diagrams.

The [user guide](docs/user-guide.md) describes the requirement syntax, the
configuration and the commands.

## Documentation

- [User guide](docs/user-guide.md): syntax, configuration, commands
- [Concept and plan](docs/konzept.md) (German): design decisions and work packages
- [Contributing](CONTRIBUTING.md): development environment, commit rules, releases,
  license of contributions
- API documentation: `cargo doc --open`

## License

MIT, see [LICENSE](LICENSE). Contributions are licensed under MIT as
well, see [CONTRIBUTING.md](CONTRIBUTING.md).
