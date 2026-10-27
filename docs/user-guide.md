# shall user guide

This guide describes what is implemented today. Planned features are
listed in [the plan](konzept.md) (German).

## Requirement files

Requirements are items in Markdown files. The syntax follows
[OpenFastTrace](https://github.com/itsallcode/openfasttrace) (OFT).

```markdown
# Software requirements

## Shut down the motor on overtemperature
`swr~motor.overtemp-shutdown-k4f2~2`

The software shuts down the motor within 100 ms when the winding
temperature exceeds 85 °C.

Rationale:
Protects the winding insulation.

Status: approved
Source: Customer specification ch. 4.2
Tags: safety, thermal

Depends:
- swr~motor.temp-sensor-c2v6~1

Covers:
- sys~motor.thermal-protection-q7m2~1

Needs: dsn
```

### Items

- An item starts with a line that begins with its ID at column 0,
  optionally in backticks. After the ID, the line may continue after a
  space or a closing backtick; that text is ignored.
- A line at column 0 that starts like an ID (`word~word...`) but is not
  a valid one is reported, and the lines up to the next item are
  ignored, so they cannot end up in the previous item. Avoid starting a
  prose line with something like `foo~bar`.
- The heading directly above the ID is the item's title; blank lines in
  between are fine, other text is not. Only ATX headings (`#` to
  `######`) are recognised; headings underlined with `===` or `---` are
  not.
- An item with a title ends at the next ID or at a heading of the same
  or a higher level. A deeper heading directly followed by an ID starts
  a nested item with that heading as its title; any other deeper heading
  is part of the text. An item without a title ends at any heading.
- IDs and keywords inside fenced code blocks (```` ``` ```` or `~~~`)
  are ignored, so examples stay examples.

### IDs

An ID has the form `type~name~revision`, e.g. `swr~motor.overtemp-k4f2~2`.

| Part | Rules |
| --- | --- |
| type | ASCII letters only, a level or artifact type such as `swr`, `dsn`, `impl`, `utest` |
| name | starts with an ASCII letter, then letters, digits, `_` and `-`; `.` separates non-empty segments |
| revision | number without leading zeros, at most 2147483647; requirements start at 1, `0` is reserved for items generated from code and test tags |

Every ID that `shall` accepts is also valid in OFT. The convention is to
increase the revision when the content of an item changes, so that links
to the old revision become outdated and get reviewed; `shall check`
reports them. Never rename an ID and never reuse one; let
`shall new` create them.

### Keywords

Keywords start at column 0 and are case-sensitive (`Needs:`, not
`needs:` or `  Needs:`).

| Keyword | Value |
| --- | --- |
| `Description:` | optional; text between the ID and the first keyword is the description anyway |
| `Rationale:` | text on the same line and the following lines |
| `Comment:` | text on the same line and the following lines |
| `Status:` | exactly `draft`, `proposed`, `approved` (default) or `rejected`, on the same line |
| `Source:` | origin, e.g. a chapter of a customer specification, on the same line |
| `Tags:` | comma-separated on the same line, or a bullet list |
| `Covers:` | bullet list of the IDs this item refines (its parents) |
| `Depends:` | bullet list of IDs this item depends on, without effect on coverage |
| `Needs:` | types that must cover this item, comma-separated on the same line or as a bullet list |

Bullet lists use `-`, `*` or `+`. Entries of `Covers:`, `Depends:` and
`Needs:` may carry text after the ID or be a Markdown link, as in OFT:
`- sys~x~1 (parent)` or `- [sys~x~1](system.md#x)`. Tags are taken as
written, without backticks.

A bullet list continues, across blank lines, until the next keyword or
the end of the item; other text in between is an error. Put lists last
in an item, or follow them with a keyword.

Unlike OFT, `shall` accepts keywords anywhere within an item. Known
differences that matter when cross-checking with OFT are listed in the
API documentation of `shall_core::markdown`.

### Checks while reading

The parser detects the following problems; `shall check` reports them.

- invalid IDs, types and statuses
- near-miss keywords that differ only in case or, for keywords longer
  than four letters, by one letter, such as `Neds:` (a warning; the line
  stays text)
- wrong keyword values, duplicate single keywords, duplicate list entries
- text in a bullet list that is no list entry
- code blocks that are never closed
- duplicate IDs across files, whatever their revision

## Configuration: `shall.toml`

`shall.toml` marks the root of a project and describes its levels, its
verification types and the files to read:

```toml
[[level]]
type  = "sys"
name  = "L2 – System Requirements"
needs = ["swr"]

[[level]]
type  = "swr"
name  = "L3 – Software Requirements"
needs = ["dsn"]

[[level]]
type  = "dsn"
name  = "L3D – Design Decisions"
needs = ["impl", "utest"]

[verification]
types = ["utest"]

[sources]
requirements = ["reqs/**/*.md"]
impl  = ["src/**/*.{cpp,hpp}"]
utest = ["tests/**/*.cpp"]
junit = ["build/test-results/**/*.xml"]
```

### `[[level]]`

| Key | Meaning |
| --- | --- |
| `type` | required; type of the level's items, ASCII letters only; at most one level per type |
| `name` | required; readable name |
| `needs` | optional; types that must cover items of this level, unless an item has its own `Needs:` |

### `[verification]`

| Key | Meaning |
| --- | --- |
| `types` | types that verify, e.g. `utest`; their tags are matched with test results |
| `not-executed` | `error` (default) or `warning`: severity of a test without a result |

### `[sources]`

Glob patterns relative to the project root. `*` stays within a
directory, `**` spans directories, `{cpp,hpp}` gives alternatives.

| Key | Meaning |
| --- | --- |
| `requirements` | requirement files; without it, all Markdown files (`.md`, `.markdown`) of the project |
| `junit` | JUnit XML files with test results; without it, tests are not matched |
| any other key | code and test files to scan for tags |

The keys for code and test files only group patterns; the type of an item
comes from its tag, and a file matched by several keys is read once. If
there are `[[level]]` tables, a key that is no known type (a level, a
type needed by a level or a verification type) gets a warning, since it
is probably a typo. A pattern that matches no file gets a warning, an
invalid pattern is an error.

Symbolic links are followed. A file that the patterns select through
several paths, via symbolic or hard links, is read once: under the first
path without a symbolic link on the way, otherwise under the first. A
broken link that a pattern selects and a link loop get a warning. Files
matched by `.gitignore` are skipped: in a Git work tree as Git does,
with the `.gitignore` files up to the top of the work tree, and nested
repositories and submodules with their own; outside of one, such as in
an archive, with those of the project. Global excludes and
`.git/info/exclude` do not count, so the result does not depend on the
machine. A JUnit pattern
that starts with a directory, such as `build/test-results/**/*.xml`,
also finds ignored files below that directory, since test results are
usually in an ignored build directory; a pattern such as `**/*.xml`
follows `.gitignore`.

Tables and keys that are not described here are ignored.

## Tags in code and tests

Code and tests point to the items they implement or verify with tags in
OFT syntax, usually in a comment:

```cpp
// [impl->dsn~motor.temp-monitor-r5t1~1]
void TempMonitor::run(float winding_temp_c) { /* ... */ }

// [utest->dsn~motor.temp-monitor-r5t1~1]
TEST(TempMonitor, RaisesFlagAbove85C) { /* ... */ }
```

A tag creates an item of its type, here `impl` and `utest`, that covers
the target. Its ID is generated from the target's name and a hash of the
file path and the tag, e.g. `impl~motor.temp-monitor-r5t1-3fa2c1d0~0`;
it stays the same when other tags are inserted or the target's revision
changes. Defects of tag items name the tag and point at its line.

| Form | Meaning |
| --- | --- |
| `[impl->dsn~a~1]` | an `impl` item covering `dsn~a~1` |
| `[impl->dsn~a~1, dsn~b~1]` | one item per target |
| `[impl->dsn~a~1>>utest]` | an item named like the target, without hash, that needs `utest` |
| `[impl~name~2->dsn~a~1, dsn~b~1]` | one item `impl~name~2` covering both targets |
| `[impl~~2->dsn~a~1]` | like the first form, with revision 2 |

A text is only a tag if its first target starts like an ID (`type~name`),
so code such as `values[p->len]` is not mistaken for one. A malformed tag
that does not start like that, e.g. `[impl->dsn ~a~1]`, is ignored
without a diagnostic and shows as missing coverage. Short tags
`[[name:1]]` are not supported.

### Tests

After a tag, `shall` records the test defined right after it: gtest's
`TEST`, `TEST_F`, `TEST_P`, `TYPED_TEST`, `TYPED_TEST_P` and
`GTEST_TEST`, or a Rust function with a test attribute (`#[test]`,
`#[tokio::test]` and similar, `#[rstest]`, `#[test_case(…)]`). Comments,
attributes, preprocessor lines and further tags in between are skipped;
a tag in a trailing comment belongs to the code before it on its line.

```rust
#[cfg(test)]
mod tests {
    // [utest->dsn~motor.temp-monitor-r5t1~1]
    #[test]
    fn raises_flag_above_85c() { /* ... */ }
}
```

For tags of a verification type, `shall check` looks up the test's
results in the JUnit files:

| Test | Result in JUnit XML (`classname`, `name`) |
| --- | --- |
| gtest `TEST(S, N)`, `TEST_F`, `GTEST_TEST` | `S`, `N` |
| gtest `TEST_P(S, N)` | `S` or `Instance/S`, `N/…` (all instances) |
| gtest `TYPED_TEST(S, N)`, `TYPED_TEST_P` | `S/…` or `Instance/S/…`, `N` (all instances) |
| Rust `fn n` in `src/a/b.rs`, `mod tests` | test binary, `a::b::tests::n` |
| Rust `#[rstest]` with `#[case]`, `#[test_case]` | test binary, `…::n::…` (all cases; also the tests of a module `n` next to the function) |

gtest writes JUnit XML with `--gtest_output=xml:<file>`. Its results
carry a `file` attribute with the test's source file; a result belongs to
the file of a verification tag that it names (equal, or ending with `/`
and the path; the longest such file). Results of another tagged file
never count, and if some results name the tag's file, only those count,
so that tests with the same name in other test programs do not match.

For Rust, the module path comes from the file path. The crate is the
directory before the last `src` component, or before the last `tests`
component if there is no `src`:

- `src/a/b.rs` and `src/a/b/mod.rs` are `a::b`, also below `src/tests/`;
- `src/lib.rs`, `src/main.rs`, `src/bin/x.rs`, `src/bin/x/main.rs`,
  `tests/x.rs` and `tests/x/main.rs` add nothing;
- `tests/x/a.rs` is module `a` of the integration test `x` or, as shared
  code, module `x::a` of some test; `tests/common/mod.rs` is the shared
  module `common`;
- `#[path]` attributes are not followed.

The results of `cargo nextest` (JUnit output, e.g. in
`target/nextest/<profile>/junit.xml`) carry the test binary, which must
fit the file: the library for `src/lib.rs`, the library or the package's
binary for `src/main.rs` and other modules under `src/`, `crate::bin/x`
for `src/bin/x.rs`, `crate::x` for `tests/x.rs`; the crate must be the
package of the `Cargo.toml` in the crate directory (the directory with
`src/` or `tests/`). nextest leaves ignored tests out of its output, so
they count as not executed. libtest's own JUnit output (`cargo test --
-Z unstable-options --format junit`, nightly only) names a test by its
module as `classname` and its function as `name`, in a
`<testsuite package="test">`; a result whose `classname::name` is the
full path matches as well, unless its name already contains `::` or its
`<testsuite>` is named like its class without `package="test"`, as in
nextest's output.

A test failed if one of its results failed (`<failure>` or `<error>`),
was skipped if all of them were skipped (`<skipped>`, or gtest's
disabled tests), and was not executed if it has no result. Results that
match no tag are not reported, except failures that gtest records
outside of any test.

## Commands

### `shall new`

```text
shall new <TYPE> <TITLE> [--file <FILE>] [--component <COMPONENT>] [--no-history]
```

Creates a new ID `type~base-random~1` and prints it.

- `<TYPE>` must be the `type` of a `[[level]]` in `shall.toml`. The
  project is the closest directory with a `shall.toml`, starting at the
  current directory.
- The base is the title turned into a slug: transliterated to ASCII,
  lower case, words joined by `-`, at most 32 characters. A slug that
  would not start with a letter gets the prefix `item-` (`10 ms task`
  becomes `item-10-ms-task`). The title must not be empty or contain line
  breaks.
- `--component` puts a component in front of the slug, separated by `.`.
  It is lower-cased and must start with a letter: `Motor` gives
  `motor.<slug>`.
- The random part is 4 characters of lower-case Crockford Base32
  (`0-9a-z` without `i`, `l`, `o`, `u`); if 20 attempts are all taken, it
  grows to 5 characters.

```sh
shall new swr "Shut down on overtemperature" --component motor
# swr~motor.shut-down-on-overtemperature-k4f2~1
```

With `--file`, the skeleton of the item is appended to that Markdown
file and its location is printed. The file is given relative to the
current directory and created if missing (its directory must exist); the
printed path is relative to the project root.

```sh
shall new dsn "Temperature monitor" --file reqs/design.md
# reqs/design.md:30: created dsn~temperature-monitor-7qz3~1
```

The skeleton contains the title, the ID, a placeholder description, a
`Covers:` placeholder (if some level needs this type) and the level's
`needs` as `Needs:`. Replace the placeholder `TODO-parent-id`; it is not
a valid ID.

The target file must be a Markdown file (`.md` or `.markdown`) inside
the project, also after resolving symbolic links, not inside `.git` and
not ignored by Git. Nothing is written if the new item would not be
recognised, e.g. after an unclosed code block.

#### Which IDs count as taken

A new ID is never one that is

- an item in a Markdown file of the project (files ignored by Git and
  fenced code blocks excluded), or
- anywhere in the Git history of this clone: all branches, tags, stashes
  and the reflog.

Deleted IDs are therefore not reused as long as they are in the history
of this clone. Not covered are branches of others that were never
fetched, reflog entries that Git has already expired, and IDs that only
appear in other files such as source code. In a shallow clone, or when
Git cannot read the history, `shall new` stops with an error; pass
`--no-history` to skip the history deliberately. Outside of a Git
repository it only warns that deleted IDs cannot be excluded.

For VSCode, copy [`contrib/vscode/tasks.json`](../contrib/vscode/tasks.json)
to `.vscode/tasks.json` and run the task "shall: new item" in an open
requirements file.

### `shall check`

```text
shall check [PATH] [--mode <report|strict>]
```

Reads the project, traces it and prints one line per problem, sorted by
file and line, followed by a summary on stderr:

```text
reqs/design.md:12: dsn~motor.temp-monitor-r5t1~1: covers outdated revision swr~motor.overtemp-shutdown-k4f2~1 (current revision: 2)
reqs/software.md:20: swr~motor.overtemp-warning-p8n5~1: not covered by required type 'dsn'
src/fault_log.cpp:3: tag [impl->dsn~motor.fault-log-z2d8~1]: unknown target dsn~motor.fault-log-z2d8~1
tests/shutdown_test.cpp:5: test Shutdown.DisablesPwmOnOvertemp failed
not ok: 4 errors, 0 warnings (8 requirements, 7 tags, 3 test results)
```

- `PATH` is a directory in the project, by default the current one; the
  project root is the closest directory at or above it with a
  `shall.toml`. Paths in the output are relative to the project root.
- Errors are printed as `path:line: message`, warnings as
  `path:line: warning: message`.
- The summary counts tags, not tag items: a tag with several targets
  counts once.

Reported are the problems found while reading (see above), and:

| Problem | Severity |
| --- | --- |
| an item does not have a valid link from every type it needs | error |
| a link to an unknown ID, an outdated or a not yet existing revision, a rejected item, a duplicate ID, or of a type the target does not need | error |
| a coverage cycle | error |
| an item of a type that is no level, no type needed by a level and no verification type | error; a warning if only `Needs:` of items name the type |
| a test failed | error |
| a test was skipped | warning |
| a test was not executed | `[verification] not-executed` |
| a verification tag without a test definition after it | warning |
| invalid XML in a JUnit file, also a truncated one; a failure outside of any test | error |
| a JUnit file without `<testsuites>` or `<testsuite>`, an unreadable attribute | warning |
| an invalid pattern in `[sources]`; an unreadable or non-UTF-8 file or directory | error |
| a broken symbolic link, a symbolic link loop, a path that is not UTF-8 | warning |
| other problems with `[sources]` (see above) | warning |

Only root causes are reported: an item that is covered, but whose
coverage has a gap further down, is not deep covered, which is no
defect of its own. Items with `Status: rejected` need no coverage.

| `--mode` | Exit code |
| --- | --- |
| `report` (default) | 0, whatever the defects |
| `strict` | 1 if there is any error or warning, otherwise 0 |

The modes `new` (fail only on new defects) and `mature` (fail on
defects of mature items), and setting the mode in `shall.toml`, are
planned (M7.5).

### `shall report`

```text
shall report [PATH] [--out <DIR>]
```

Reads and traces the project like `shall check` and writes the result
into the output directory, by default `site` in the current directory;
the directory is created if missing:

- `index.html`, the report: one self-contained file with embedded CSS
  that opens from disk without a server;
- `trace.json`, the whole traced project as JSON, from which the report
  is made (see below).

stderr shows the written files and the summary of `shall check`;
defects do not change the exit code, `shall check` decides that.

`index.html` starts with a summary: the number of requirements and
tags, the test verdicts of the verification tags (with `junit`
sources; a tag with several targets counts once), the number of errors
and warnings, and the coverage per level, as the number of items that
are covered and deep covered, also in percent, rounded down so that
100 % means complete. Rejected items are not counted, and test results
do not count as coverage. Then follows one table per level with every
requirement (tag items are not listed): its coverage, ID, title,
status and location; copies of a duplicate ID are marked. Coverage has
a sign and a text for screen readers besides its color, explained in a
legend: `✓` deep covered, `!` covered but with a gap further down, `✗`
not covered (with the missing types), `–` rejected. The levels are
those of `shall.toml`, in their order; types of requirements without a
level follow, named by their type. If such a type is unknown, which
`shall check` reports as an error, its items get `?` (with missing
types, if they have `Needs:`) and their level no coverage. Requirement details with
their links, the defect list, filters and the graph view are planned
(M5).

The output directory is not excluded from the project's files. Add it
to `.gitignore` (e.g. `/site/`), or make sure no pattern in `[sources]`
matches it; otherwise a pattern such as `**/*.json` reads the tags
quoted in `trace.json` back in on the next run.

Both files are deterministic: the same project gives the same bytes,
with paths relative to the project root, separated by `/`, and without
timestamps. The fields of `trace.json`:

| Field | Content |
| --- | --- |
| `schema_version` | version of this layout, currently `1`; increased on every change readers must know about |
| `generator` | `name` (`shall`) and `version` of the tool |
| `levels` | the `[[level]]` tables: `type`, `name`, `needs` |
| `verification_types` | `[verification] types` |
| `items` | every item, ordered by file and line (see below) |
| `links` | every `Covers:` and `Depends:` link of items that are not rejected: `kind` (`covers`, `depends`), `source` and `source_location` (the item that contains the link), `target` as written, `target_location` (`null` if no item or several have the target's type and name), `location` of the link itself, `status` |
| `test_results` | every test case of the JUnit files in the order they were read, also those without a tag: `classname`, `name`, `suite`, `outcome` (`passed`, `skipped`, or `failed` with a `message`, which may be `null`), `location`; `null` without `junit` in `[sources]` |
| `defects` | every problem `shall check` prints, in its order: `severity` (`error`, `warning`), `message`, `location` |

A `location` is `{"path": "reqs/design.md", "line": 12}`. A link's
`status` is `valid`, `unknown`, `outdated` or `predated` (with the
target's `current` revision), `ambiguous`, `rejected`, or `unwanted`
(the target does not need the source's type); only `valid` links count
as coverage.

Each item has:

- `id`, and its parts `type`, `name` and `revision`;
- `title`, `description` (Markdown as written), `rationale`, `comment`,
  `status`, `tags` and `source` from the requirement file; `rationale`,
  `comment` and `source` are `null` and `tags` is `[]` if absent; tag
  items have an empty title and description;
- `location`, the line of the ID or tag;
- `origin`: `{"kind": "requirement"}`, or `{"kind": "tag", "text": ...,
  "test": ...}` with the tag as written and the test definition after
  it, `null` if there is none: `kind` (`gtest`, `gtest_parameterized`,
  `gtest_typed` or `rust`), `suite` (`null` for Rust), `name`, `module`
  (the inline Rust modules around the test, outermost first) and
  `line`;
- `needs`: the types the item needs, from `Needs:` or its level; empty
  for rejected items;
- `covered_by`: `id` and `location` of the items covering it with a
  valid link; `missing`: the needed types without one;
- `covered`, `deep_covered` (covered all the way down) and `duplicate`
  (another item has the same type and name); rejected items need
  nothing, so they are always `covered`;
- `unknown_type`: whether the item's type is unknown, which is an
  error: `shall.toml` has levels, but the type is no level, no type
  needed by a level, no verification type and not needed by any item;
  always `false` for rejected items;
- `test`, for verification tags with `junit` sources: `name` as runners
  report it (`null` without a test definition), `verdict` (`passed`,
  `failed`, `skipped`, `not_executed`, `no_test`) and `results`, the
  ascending positions of its results in `test_results`, starting at 0
  (several results can share a location in a JUnit file written on one
  line); otherwise `null`.

### Exit codes

All commands exit with 2 if they cannot run, e.g. without a
`shall.toml`, with an invalid configuration or with an invalid
argument.
