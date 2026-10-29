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
| `Features:` | features the item belongs to, comma-separated on the same line or as a bullet list, see [Variants](#variants) |
| `Covers:` | bullet list of the IDs this item refines (its parents) |
| `Depends:` | bullet list of IDs this item depends on, without effect on coverage |
| `Satisfied-by:` | bullet list of the IDs of reused items that meet this item, see [Reused items](#reused-items) |
| `Params:` | bullet list of `name = value` entries: the parameter values with which the items of `Satisfied-by:` meet this item |
| `Needs:` | types that must cover this item, comma-separated on the same line or as a bullet list |

Bullet lists use `-`, `*` or `+`. Entries of `Covers:`, `Depends:`,
`Satisfied-by:` and `Needs:` may carry text after the ID or be a
Markdown link, as in OFT: `- sys~x~1 (parent)` or
`- [sys~x~1](system.md#x)`. Tags are taken as written, without
backticks; so are the names and values of `Params:` and features, and
a `Params:` entry may be in backticks as a whole. `Satisfied-by:`,
`Params:` and `Features:` are extensions of `shall` that OFT does not
know.

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
- `Params:` entries that are not `name = value`, with a name without
  spaces and a value; a parameter set twice to different values
- `Params:` without `Satisfied-by:` (a warning)
- invalid feature names: as in Cargo, letters, digits, `_`, `-`, `+`
  and `.`, starting with a letter, a digit or `_`
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
| `needs` | optional; types that must cover items of this level, unless an item has its own `Needs:` or a `Satisfied-by:` of an allowed type |

The `needs` of the levels must not form a cycle, e.g. `swr` needing
`dsn` and `dsn` needing `swr`, or a level needing its own type: every
item would then need a covering item without end. `shall.toml` with
such levels is an error, and so is an import whose levels close such a
cycle. For requirements of one type that refine each other, give the
level the needs of the leaves, e.g. `needs = ["dsn"]`, and write
`Needs: req` on the items that are refined by others.

### `[verification]`

| Key | Meaning |
| --- | --- |
| `types` | types that verify, e.g. `utest` or `vrev`; their tags are matched with test results, their items in requirement files are reviews or analyses (see [Reviews and analyses](#reviews-and-analyses)) |
| `not-executed` | `error` (default) or `warning`: severity of a test without a result |

### `[report]`

| Key | Meaning |
| --- | --- |
| `code_url` | address of a line of code at a commit, with the placeholders `{sha}`, `{path}` and `{line}`, e.g. `https://git.example.org/team/project/src/commit/{sha}/{path}#L{line}`; must start with `https://` or `http://` |

With `code_url`, every `path:line` in the report links to the code,
at the commit given with `shall report --build-info` if the address
contains `{sha}` (without a commit, such locations are not linked).
`{path}` is relative to the project root, or to the workspace root in
a workspace, and percent-encoded except for `/`; put the path of the
project in the repository into the address if they differ.

### `[satisfied_by]`

| Key | Meaning |
| --- | --- |
| `targets` | types that `Satisfied-by:` may name; default `["cap"]` |
| `fallback` | types that `Satisfied-by:` may name with a warning; default none |

A type cannot be both. See [Reused items](#reused-items).

### `[variant.<name>]`

| Key | Meaning |
| --- | --- |
| `features` | the features the variant selects; default none |
| `junit` | JUnit XML files with the test results of the variant's build; without it, those of `[sources]`; `[]` for none |

The name starts with a letter or digit, followed by letters, digits,
`_` or `-`, and differs from the other names in more than case; reports
use it as a directory name. Other keys get a warning, since they are
often typos such as `feature`. See
[Variants](#variants).

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
there are `[[level]]` tables, also in the imports, a key that is no
known type (a level, a type needed by a level or a verification type,
also of the imports) gets a warning, since it
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

`[[import]]` and `[workspace]` are described in
[Workspaces and imports](#workspaces-and-imports). Tables and keys that
are not described here are ignored.

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

For Rust, the module path comes from the file path below the project
root, also in a workspace. The crate is the directory before the last
`src` component, or before the last `tests` component if there is no
`src`:

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

## Reviews and analyses

An item of a verification type written in a requirement file is a
review or an analysis, by convention of type `vrev` or `vana`. It
covers what it verifies with `Covers:` like any other item, so a level
or an item can need it:

```markdown
## Review of the relay driver
`vrev~motor.relay-driver-review-m3q8~1`

The relay driver was reviewed against the emergency stop timing.

Status: approved

Covers:
- dsn~motor.relay-driver-b6j3~1
```

with `vrev` in `[verification] types` and, for example,
`needs = ["impl", "utest", "vrev"]` for the design level. A review is evidence
when it is approved; as for every item, that is also the default without
`Status:`, so give a review that is still under way `Status: draft`. One
in `draft` or `proposed` still covers its target, but verifies nothing.
That is no defect, like a requirement in draft. In the summaries of
`shall check` and `shall report`, reviews and analyses count as
requirements.

## Verified

An item is *verified* when everything below it is implemented and
successfully validated:

- it is deep covered, not rejected, not a copy of a duplicate ID and
  not of an unknown type;
- if its type is a verification type: a tag's test passed, a review or
  analysis is approved;
- every item covering it with a valid link is verified.

Without `junit` in `[sources]` no test has passed, so nothing that
needs a test is verified. The status of other items does not matter: a
requirement in draft can be verified. `shall report` shows which items
are verified; `shall check` reports only defects.

### Light of a chain

For the Green Graph, every item also gets a light that sums up the item
and everything below it, the worst of its own and those of the items
covering it:

| Light | Sign | Own light of an item |
| --- | --- | --- |
| red | ✗ | a missing type, an invalid link of its own (e.g. an outdated revision), a failed test, a test not executed if `[verification] not-executed` is `error`, a copy of a duplicate ID, an unknown type, or part of or above a coverage cycle |
| yellow | ! | missing evidence: a test skipped, not executed (by default), without a test definition or without `junit` results; a review that is not approved; an item in `draft` or `proposed` |
| green | ✓ | none of these |
| grey | – | rejected; not counted |

So a requirement is green only when its whole chain is complete,
tested, reviewed and approved, and the light tells a gap or defect
(red) from missing evidence (yellow). A green item is always
[verified](#verified); a verified item is red if one of its own links
is invalid, and yellow if it is in draft. A `Covers:` link to an item
outside the traced variant does not make an item red; showing items
that are provided but not requested in grey is planned (M8.6).

## Reused items

A platform shared by several projects describes what it can do as
items of a stable level, by convention *capabilities* of type `cap`.
A customer requirement names the capabilities that meet it with
`Satisfied-by:`, so the platform does not have to know its customers:

```markdown
## Shut down the drive on overtemperature
`hlr~customer-a.overtemp-c3x9~1`

The drive shuts down within 100 ms on overtemperature.

Satisfied-by:
- cap~motor.overtemp-protection-h6w2~1

Params:
- motor.shutdown_time_ms = 100
```

```toml
[[level]]
type  = "hlr"
name  = "Customer Requirements"
needs = ["swr"]

[[level]]
type  = "cap"
name  = "Platform Capabilities"
needs = ["swr"]

[[level]]
type  = "swr"
name  = "Software Requirements"
needs = ["impl", "utest"]

[satisfied_by]
targets  = ["cap"]
fallback = ["swr"]
```

- `Satisfied-by:` is a `Covers:` written the other way round: the
  capability covers the customer requirement. The link is checked like
  a `Covers:` link, against an unknown ID, the revision and so on.
- An item with `Satisfied-by:` needs the types of its targets and those
  of its own `Needs:`, but not the `needs` of its level: here,
  `hlr~customer-a.overtemp-c3x9~1` needs `cap`, not `swr`, so a
  `Covers:` link from an `swr` item to it is an error. If the platform
  meets a requirement only in part, add `Needs: swr` and refine the
  rest in the project. `shall new` writes the level's needs into a new
  item as `Needs:`; remove that line when you add `Satisfied-by:`. A
  target of a type that is not allowed is no need; if no target is
  allowed, the item needs what its level needs.
- The target's type must be in `[satisfied_by] targets`; a type in
  `fallback` gets a warning, any other type is an error. Without
  `[[level]]` tables, every type is allowed.
- The item is deep covered and verified when its targets are.
- `Params:` records the parameter values with which the platform meets
  the requirement. They are part of `trace.json`, and the report shows
  them in the requirement's details under "Satisfied by".

In a repository with several projects, the platform is a project of
its own that customer projects import, see
[Workspaces and imports](#workspaces-and-imports).

## Workspaces and imports

A repository with several projects has a `shall.toml` with a
`[workspace]` at its root that lists them; each member has its own
`shall.toml`:

```text
repo/
  shall.toml            # [workspace]
  platform/
    shall.toml          # levels cap, swr, dsn; sources
  projects/
    customer-a/
      shall.toml        # level hlr; [[import]] of the platform
```

```toml
# repo/shall.toml
[workspace]
members = ["platform", "projects/*"]
```

```toml
# repo/projects/customer-a/shall.toml
[[level]]
type  = "hlr"
name  = "Customer Requirements"
needs = ["swr"]

[[import]]
name = "plat"
path = "../../platform"
```

- `members` are directories relative to the workspace root; `*` matches
  one directory level. Every member needs a `shall.toml`; a pattern that
  matches none, for example because `.gitignore` ignores the directory,
  is an error, and so is a member that a symbolic link leads out of the
  workspace, and so is a `[workspace]` without members. A project below
  a workspace must be a member; the search for a workspace above a
  project stops at the top of its Git work tree, so `shall check` in a
  submodule or a nested repository checks it on its own; as a member, it
  is still checked with the workspace. The workspace `shall.toml` is no
  project: it has only `[workspace]`.
- An `[[import]]` makes the items, levels and verification types of
  another project of the repository part of this project's trace, so
  that customer requirements can name platform items in
  `Satisfied-by:`. Its test results come from its own `[sources]
  junit`. A level that both define must need the same types. The
  `targets` and `fallback` types of `[satisfied_by]` are joined: a type
  is a target if the project or an import makes it one, otherwise a
  fallback if one of them does. The
  features of imported items are selected by the variants of the
  importing project; only the imported project's own variants check
  their names.
- `path` is relative to the importing project and must name a member
  of the workspace; it always follows the current state of the files, so a
  `rev` is an error. Imports from other repositories (`git`, `rev`)
  and the imports of an import (a warning for now) are planned (M8.2,
  M8.3).
- IDs are not renamed by an import; a duplicate ID in a project and
  its imports is an error as within one project. Members that do not
  import each other are not compared.
- Paths in diagnostics and in `trace.json` are relative to the
  workspace root, e.g. `platform/reqs/cap.md:7`, wherever `shall` runs.
  Files of a project nested in another one belong to the inner project
  only.
- `shall check` in the workspace root, or `shall check --workspace` in
  a member, checks every member with its imports; the requirement and
  code files of each project are read once, also if several projects
  import it, its test results once per importing project. Defects of a
  shared project are printed once, followed by one summary per member
  and one for the workspace. `shall new` checks the IDs of its own project only.
- `shall report` reports one project; reports of a whole workspace are
  planned.

## Variants

Items that belong to an optional part of the product name its features
with `Features:`. A variant in `shall.toml` selects features, with the
names of the Cargo features or CMake options, so that one list drives
both the build and the trace:

```markdown
## Read out the fault memory over UDS
`cap~diag.uds-readout-t4m1~1`

The platform reads out the fault memory over UDS.

Features: diag_uds
```

```toml
[[level]]
type  = "cap"
name  = "Platform Capabilities"
needs = ["impl"]

[variant.basis]
features = ["motor_ctrl", "can"]

[variant.premium]
features = ["motor_ctrl", "can", "diag_uds"]
junit = ["build/premium/test-results/*.xml"]
```

- An item with `Features:` belongs to a variant that selects all of
  them.
- An item without `Features:`, such as a code or test tag, belongs to a
  variant when one of the items it covers does, or when it covers no
  existing item. So the design, code and tests below a capability
  follow it into or out of a variant.
- IDs are global: a duplicate ID is an error in every variant, even if
  only one copy belongs to it, and links to it are ambiguous. A link to
  a wrong revision of an item outside the variant is outdated as
  usual.
- `Satisfied-by:` and `Depends:` pass no features on: a customer
  requirement belongs to every variant, and a `Satisfied-by:` or
  `Depends:` link to an item outside the variant is an error of that
  variant. A `Covers:` link to such an item is no defect.
- `shall check --variant <name>` and `shall report --variant <name>`
  trace only the items of the variant, with its test results. A result
  that names the file of a tag outside the variant (a gtest `file`
  attribute, the module path of a Rust test) never counts for a test
  inside; one that names no file goes to every tag of that test, as
  without a variant. The summary counts all results read;
  `shall report --all-variants` writes a report for every variant. Without
  these options, all items are traced, whatever their features.
- A feature that no variant selects gets a warning, since it is often a
  typo; without `[variant.*]` tables, features are not checked.

Expressions such as `Features: ota && !eu_market` are a later
extension; showing the items that a variant provides but no customer
requirement asks for is planned (M8.6).

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
shall check [PATH] [--mode <report|strict>] [--variant <NAME> | --workspace]
```

Reads the project, traces it and prints one line per problem, sorted by
file and line, followed by a summary on stderr:

```text
reqs/design.md:12: dsn~motor.temp-monitor-r5t1~1: covers outdated revision swr~motor.overtemp-shutdown-k4f2~1 (current revision: 2)
reqs/software.md:20: swr~motor.overtemp-warning-p8n5~1: not covered by required type 'dsn'
src/fault_log.cpp:3: tag [impl->dsn~motor.fault-log-z2d8~1]: unknown target dsn~motor.fault-log-z2d8~1
tests/shutdown_test.cpp:5: test Shutdown.DisablesPwmOnOvertemp failed
not ok: 4 errors, 0 warnings (9 requirements, 7 tags, 3 test results)
```

- `PATH` is a directory in the project, by default the current one; the
  project root is the closest directory at or above it with a
  `shall.toml`. Paths in the output are relative to the project root,
  or to the workspace root in a workspace.
- In a workspace root, and with `--workspace`, every member is checked
  (see [Workspaces and imports](#workspaces-and-imports)).
- Errors are printed as `path:line: message`, warnings as
  `path:line: warning: message`.
- `--variant` checks only the items of a variant of `shall.toml`, with
  its test results (see [Variants](#variants)); the summary then starts
  with `variant <NAME>: `. An unknown variant is an error (exit code 2).
- The summary counts tags, not tag items: a tag with several targets
  counts once.

Reported are the problems found while reading (see above), and:

| Problem | Severity |
| --- | --- |
| an item does not have a valid link from every type it needs | error |
| a link to an unknown ID, an outdated or a not yet existing revision, a rejected item, a duplicate ID, or of a type the target does not need | error |
| a `Satisfied-by:` link to a type that is no target or fallback of `[satisfied_by]` | error |
| a `Satisfied-by:` link to a fallback type | warning |
| a coverage cycle | error |
| with `--variant`: a `Satisfied-by:` or `Depends:` link to an item outside the variant | error |
| a feature that no `[variant.*]` selects | warning |
| an item of a type that is no level, no type needed by a level and no verification type | error; a warning if only `Needs:` or `Satisfied-by:` of items name the type |
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
shall report [PATH] [--out <DIR>] [--variant <NAME> | --all-variants]
             [--build-info <INFO>]
```

Reads and traces the project like `shall check` and writes the result
into the output directory, by default `site` in the current directory;
the directory is created if missing:

- `index.html`, the report: one self-contained file with embedded CSS
  and a small script for the filter
  that opens from disk without a server;
- `trace.json`, the whole traced project as JSON, from which the report
  is made (see below);
- `defects.txt`, the defects as `shall check` prints them, one line
  `path:line: message` each, warnings as `path:line: warning: message`;
- `junit.xml`, the defects as JUnit XML for the test reports of CI
  systems: an error is a failed test case, a warning a skipped one,
  named by the whole line and with `file` and `line`; without defects
  there is one passed case `no defects`;
- `badge.svg`, a badge for a README such as `requirements | 2/3 green`:
  how many requirements at the top of a chain (not rejected) are green;
  green if all are, red if one is red, yellow otherwise, grey without
  any;
- `graph.dot` and `graph.mmd`, the project as a Graphviz and a Mermaid
  diagram from left to right: every requirement once with an edge to
  every requirement covering it, code and tests bundled per type, a
  dashed node for every missing type, each node with the sign and the
  colour of its [light](#light-of-a-chain). Mermaid renders at most 500
  edges and 50,000 characters by default, which a few hundred
  requirements exceed; use `graph.dot` with Graphviz for large
  projects.

With `--variant`, all files contain only the items of the variant (see
[Variants](#variants)), and the report names the variant and its
features at the top. With `--all-variants`, every variant gets all
files in `<DIR>/<variant>/`, and `<DIR>/index.html` is an overview that
links them, with the verdict and the numbers of errors, warnings,
requirements and tags of each; stderr ends with one summary per
variant. Files of earlier runs in `<DIR>`, such as a `trace.json` or
`junit.xml` of all items, are left as they are; in CI, collect
`<DIR>/<variant>/junit.xml`, or clear `<DIR>` first.

stderr shows the written files and the summary of `shall check`;
defects do not change the exit code, `shall check` decides that.

`index.html` starts with a summary: the number of requirements and tags,
the test verdicts of the verification tags (with `junit` sources; a tag
with several targets counts once), the number of errors and warnings,
and the coverage per level, as the number of items that are covered,
deep covered and [verified](#verified), also in percent, rounded down so
that 100 % means complete. Rejected items are not counted, and test
results do not count as coverage. Then follows one table per level with
every requirement (tag items are not listed): its coverage, whether it
is verified (`yes`, `no`, `–` if rejected), ID, title, status and
location; copies of a duplicate ID are marked. Coverage has a sign and a
text for screen readers besides its color, explained in a legend: `✓`
deep covered, `!` covered but with a gap further down, `✗` not covered
(with the missing types), `–` rejected. The levels are those of
`shall.toml`, in their order; types of requirements without a level
follow, named by their type, e.g. those of reviews. If such a type is
unknown, which `shall check` reports as an error, its items get `?`
(with missing types, if they have `Needs:`) and their level no coverage.

The ID in a table links to the requirement's details further down, whose
anchor is the ID, e.g. `index.html#swr~motor.overtemp-shutdown-k4f2~2`
(copies of a duplicate ID after the first get `-2`, `-3`, ...). The
details show:

- coverage, level, status, verified, the light of its chain and
  location, and the import, source,
  tags and features if there are any;
- the description, rationale and comment, rendered from Markdown, with
  headings placed below the requirement's own heading. Raw HTML is shown as
  text, and links and images keep only addresses without a scheme or
  with `http`, `https` or `mailto`, so a requirement file cannot run
  anything in the report; an image from `http` or `https` is loaded
  when the report is opened;
- the links upwards as written, `Covers:`, `Satisfied-by:` with the
  parameters of `Params:`, and `Depends:`, each linked to its target if
  that is a requirement of the report, and with the reason if it is not
  valid; those of rejected items are not traced and have no reason;
- the requirements that the item satisfies or that depend on it, and
  every invalid link to it with its source and reason, except links to
  a duplicate ID, which name no single target and show their reason at
  their source;
- downwards, the requirements covering the item, the missing types, the
  implementing code tags and the tests with their verdict (`✓ passed`,
  `✗ failed`, `! skipped`, `! not executed`, `! no test`, or `? no
  results read` without `junit` sources).

Locations are shown as `path:line`, linked to the code with `[report]
code_url`.

`--build-info` tells where the traced files come from, as
`sha=<commit>,date=<date>,pipeline=<id>` with every part optional, or
as the commit alone, e.g. `--build-info "$GITHUB_SHA"`. The commit
consists of letters, digits, `.`, `_` and `-`. The report shows them at
the top and `trace.json` under `build`. `shall` reads neither the
commit nor the time itself, so the same files give the same output.
With `--all-variants`, the report of every variant shows them, the
overview does not.

A section `Defects` lists every problem of `shall check`, errors first,
then warnings, each in the order of `shall check`, with sign and
severity in words; a location in a requirement file links to the
requirement whose lines include it, from its ID to the next ID or
heading that ends it, and the error and warning counts at the top link
to the list.

A section `Tree` shows, collapsible without JavaScript, every
requirement that covers no other item with a valid `Covers:` and is no
target of a valid `Satisfied-by:`, rejected ones included, and below
each node what covers it: requirements, missing types, code and tests,
each with sign and words. A code tag that needs coverage of its own
(`>>`) opens like a requirement. A node that appears again shows what
covers it only where it appears first; requirements that no root
reaches, such as those of a coverage cycle, follow as roots of their
own.

Above the level tables, a filter narrows the tables and the details by
coverage, status, type and tag, and a search finds requirements whose
ID and title contain all the words typed, ignoring case; a counter
says how many are shown, and a level without any shown row is hidden.
A link to a requirement that the filter hides clears the filter. It
needs JavaScript, which the report embeds like its CSS; without
JavaScript the filter is not shown and the report shows everything.

A section `Green Graph` has, for every requirement at the top of a
chain (the same roots as the tree), a collapsible drawing of its chain
from left to right: the requirements below it, each once even if
several paths lead there, and code and tests bundled per item and type
into one node such as `impl ×2` or `utest 2/3` (passed of all), with a
dashed red node for every missing type. A requirement drawn with its
chain in an earlier graph appears once more only as one node marked
`↗ shown above`, so that the report grows with the number of items.
Every node shows the
[light of its chain](#light-of-a-chain) as sign, word in its tooltip
and colour, and links to the requirement's details. The drawing is SVG
inside `index.html`, laid out by `shall`, so it needs no JavaScript.
Above the filter, a section `Overview` shows these roots as tiles in
the colour and with the sign of their light, grouped by component, the
part of their names before the first `.` (e.g. `motor` for
`sys~motor.estop-h3x9~1`), with the number of green, yellow, red and
grey tiles per group; a tile jumps to the root's graph, which the
embedded script opens (without JavaScript, click it open). Grouping by
tags and zoom are planned (M5.12).

The output directory is not excluded from the project's files. Add it
to `.gitignore` (e.g. `/site/`), or make sure no pattern in `[sources]`
matches it; otherwise a pattern such as `**/*.json` reads the tags
quoted in `trace.json` back in on the next run.

Both files are deterministic: the same project gives the same bytes,
with paths relative to the project root, separated by `/`, and without
timestamps. The fields of `trace.json`:

| Field | Content |
| --- | --- |
| `schema_version` | version of this layout, currently `1`; from the first release on, increased on every change readers must know about; until then, fields and values are added without a new version |
| `generator` | `name` (`shall`) and `version` of the tool |
| `variant` | the traced variant, `name` and `features`; `null` without `--variant` |
| `build` | `sha`, `date` and `pipeline` from `--build-info`, each `null` if not given; `null` without `--build-info` |
| `code_url` | `[report] code_url`, or `null` |
| `levels` | the `[[level]]` tables: `type`, `name`, `needs`; also those of the imports |
| `verification_types` | `[verification] types`, also those of the imports |
| `items` | every item, ordered by file and line (see below) |
| `links` | every `Covers:`, `Depends:` and `Satisfied-by:` link of items that are not rejected: `kind` (`covers`, `depends`, `satisfied_by`), `source` and `source_location` (the item that contains the link), `target` as written, `target_location` (`null` if no item or several have the target's type and name), `location` of the link itself, `status` |
| `test_results` | every test case of the JUnit files in the order they were read, also those without a tag: `classname`, `name`, `suite`, `outcome` (`passed`, `skipped`, or `failed` with a `message`, which may be `null`), `location`; `null` without `junit` in `[sources]` |
| `defects` | every problem `shall check` prints, in its order: `severity` (`error`, `warning`), `message`, `location` |

A `location` is `{"path": "reqs/design.md", "line": 12}`. A link's
`status` is `valid`, `unknown`, `outdated` or `predated` (with the
target's `current` revision), `ambiguous`, `rejected`, `unwanted`
(the target does not need the source's type), `disallowed` (the
target's type is no `[satisfied_by]` target), or `outside_variant` (the
target exists, but not in the traced variant); only `valid` links count
as coverage. A valid `Satisfied-by:` link covers its source, not its
target.

Each item has:

- `id`, and its parts `type`, `name` and `revision`;
- `title`, `description` (Markdown as written), `rationale`, `comment`,
  `status`, `tags` and `source` from the requirement file; `rationale`,
  `comment` and `source` are `null` and `tags` is `[]` if absent; tag
  items have an empty title and description;
- `params`: the entries of `Params:` with `name`, `value` and `line`,
  `[]` if absent; `features`: those of `Features:`, `[]` if absent;
- `covers`, `depends` and `satisfied_by`: the links as written, each
  with `id` and `line`, also for rejected items, whose links are not in
  `links`;
- `location`, the line of the ID or tag, and `end_line`, the last line
  of the item in its file (the tag's line for tag items);
- `import`: the name of the `[[import]]` the item comes from, `null`
  for the project's own items;
- `origin`: `{"kind": "requirement"}`, or `{"kind": "tag", "text": ...,
  "test": ...}` with the tag as written and the test definition after
  it, `null` if there is none: `kind` (`gtest`, `gtest_parameterized`,
  `gtest_typed` or `rust`), `suite` (`null` for Rust), `name`, `module`
  (the inline Rust modules around the test, outermost first) and
  `line`;
- `needs`: the types the item needs, from `Needs:` and the targets of
  `Satisfied-by:`, or from its level; empty for rejected items;
- `covered_by`: `id` and `location` of the items covering it with a
  valid link; `missing`: the needed types without one;
- `covered`, `deep_covered` (covered all the way down) and `duplicate`
  (another item has the same type and name); rejected items need
  nothing, so they are always `covered`;
- `unknown_type`: whether the item's type is unknown, which is an
  error: `shall.toml` has levels, but the type is no level, no type
  needed by a level, no verification type and not needed by any item;
  always `false` for rejected items;
- `verified`: whether the item is [verified](#verified);
- `light`: the light of the item and everything below it,
  see [Light of a chain](#light-of-a-chain): `green`, `yellow`, `red` or
  `grey`;
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
