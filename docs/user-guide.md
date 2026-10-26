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
to the old revision become outdated and get reviewed; `shall check` will
report them (planned, M2). Never rename an ID and never reuse one; let
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

The parser detects the following problems. No command shows them yet;
`shall check` will report them as `path:line: message` (planned, M2).

- invalid IDs, types and statuses
- near-miss keywords that differ only in case or, for keywords longer
  than four letters, by one letter, such as `Neds:` (a warning; the line
  stays text)
- wrong keyword values, duplicate single keywords, duplicate list entries
- text in a bullet list that is no list entry
- code blocks that are never closed
- duplicate IDs across files, whatever their revision

## Configuration: `shall.toml`

`shall.toml` marks the root of a project and describes its levels:

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
```

| Key | Meaning |
| --- | --- |
| `type` | required; type of the level's items, ASCII letters only |
| `name` | required; readable name |
| `needs` | optional; types that must cover items of this level |

Today the levels are used by `shall new`. Other tables and unknown keys
are ignored.

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

### `shall check`, `shall report`

Not implemented yet.
