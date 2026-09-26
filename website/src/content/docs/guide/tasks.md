---
title: Tasks
description: Checkbox task files — format, configuration, discovery, and the mdfu task commands.
---

`mdfu task` reads and edits checkbox tasks stored directly in your Markdown
files. The format and command surface are compatible with the external `mdtask`
tool: an existing user can switch by changing the command name. The five
documented divergences below (D1–D5) are the only intentional differences.

Everything here is non-interactive; for the full-screen browser see the
[Tasks TUI](/guide/tasks-tui/).

## Task format

A task is a single line matching, anchored at the start of the line:

```text
^- \[([ x])\] ((?:[A-Z][A-Z0-9]*-\d+ )?)(.*)$
```

The checkbox MUST be a literal space or a lowercase `x`; an uppercase `X` is
not a task and is ignored. A header MUST begin at column 0 — an indented
`- [ ] …` line (a list continuation) is not a task.

Three header forms are recognised:

| Form | Example | Notes |
|---|---|---|
| Identified | `- [ ] EXMPL-42 Title` | The only form that participates in ID resolution, blockers, `list`, and `view`. |
| Seed | `- [ ] PRJ- Title` | Names a prefix for `ids`; not itself identified. |
| Unidentified | `- [ ] Title` | No ID candidate; collected for `ids` and reported by a trailing note. |

### IDs

- A prefix MUST match `^[A-Z][A-Z0-9]*$`; a full ID is `<PREFIX>-<n>` where
  `<n>` is one or more decimal digits.
- The numeric part MUST be **globally unique across all prefixes**. `EXMPL-7`
  and `OTHER-7` in the same scope are a duplicate-numeric-part condition (a
  `validate` warning and an extra `ids` line).
- The numeric part is preserved verbatim, leading zeros included (`EXMPL-007`).

### Title and metadata

Given the remainder after the checkbox and ID, the title and metadata split at
the first literal two-tab sequence (`\t\t`). When there is no such separator, a
trailing run of valid metadata tokens is peeled off the right; the first
non-token word ends the run. That is why `#123` stays in the title (it is not a
valid tag) while `#launch` does not.

Metadata is a whitespace-separated sequence of tokens:

| Token | Pattern | Meaning |
|---|---|---|
| Tag | `^#[A-Za-z][\w-]*$` | Zero or more tags; a tag MUST start with a letter. |
| Priority | `^![A-Za-z]\w*$` | A priority; valid values are `crit`, `high`, `low`. Absent means **medium**. |
| Property | `^@([\w-]+):(\S+)$` | A `key:value` property. |

Unknown words are not tokens. A malformed `@key` with no `:value` is an unknown
word. Properties preserve first-seen order; the only built-in property is
`blocked_by`.

### Bodies

A task body is the maximal run of following lines in which every non-empty line
is indented. Blank lines are permitted inside the body; the run ends immediately
before the first non-empty line that starts at column 0. The body is stored
dedented and re-indented on write (the body indent, default two spaces), so a
round-trip preserves spelling. A body is never re-parsed for tasks: a body line
shaped like a header is ordinary body text.

CommonMark-style fenced code blocks (```` ``` ```` or `~~~`) mask header
detection *and* body extraction, so a `- [ ] …` line inside a fence is not a
task. An unclosed fence masks to end of file.

### Blockers

`@blocked_by` names one or more task IDs, comma-separated
(`@blocked_by:EXMPL-1,EXMPL-2`). A blocker is **resolved** when the named ID
belongs to a done task; a blocker naming no task in scope is **unresolved**, and
an open task with at least one unresolved blocker is **blocked**.

- A done task never reports unresolved blockers.
- In `list` output, resolved blockers are stripped from the rendered properties
  and unresolved blockers are shown in the blocker color.
- In `view` output the verbatim header is shown, so resolved `@blocked_by`
  tokens remain visible.

## Configuration

Task configuration is JSON. Two file names are recognised: `.mdtaskrc` (the
original name) and `.mdfurc` (the mdfu name). Resolution walks upward from the
start directory, and **at each directory level `.mdtaskrc` is preferred;
`.mdfurc` is used only when that level has no `.mdtaskrc`**. The walk ends at
the filesystem root, so a nearer `.mdfurc` wins over a farther `.mdtaskrc`.

The schema is:

```json
{
  "path": "notes",
  "files": {
    "include": ["**/*.md"],
    "exclude": ["archive/**"]
  },
  "excludePrefixes": ["ARCH"],
  "archivePath": "_archive.md"
}
```

| Key | Type | Meaning |
|---|---|---|
| `path` | string | Base directory, resolved relative to the config file's directory. |
| `files.include` | array of string | Globs selecting files; empty means every `.md` file. |
| `files.exclude` | array of string | Globs removing files; applied after include. |
| `excludePrefixes` | array of string | Prefixes whose tasks `validate` MUST NOT report. |
| `archivePath` | string | Archive file, relative to the base directory. Defaults to `_archive.md`. |

Validation is defensive: a wrongly-typed value is dropped, a non-object `files`
or unknown key is ignored. Malformed JSON is fatal (`Invalid JSON in <path>:
<message>`), as is an unreadable or directory rc path (`mdfu task: cannot read
<path>: <err>`).

The base directory is the first of:

1. `--path <dir|file>` on the command line;
2. `$MDTASK_PATH`;
3. the config file's `path`, resolved relative to the config file's directory;
4. `.`.

When no configuration file exists, the base is the nearest ancestor containing
`.git`, falling back to the start directory. Globs are matched relative to the
base against the slash-separated relative path.

:::note
`.mdtaskrc`/`.mdfurc` are the **task** configuration only. They are unrelated to
the YAML **theme** configuration read from `$XDG_CONFIG_HOME/mdfu/config.yaml`
or passed via `--config` (see [Configuration](/guide/configuration/)). The two
files never merge and share no keys.
:::

## Discovery

Scope discovery walks the base directory and:

- **includes** hidden files and hidden directories (names beginning with `.`);
- **ignores `.gitignore` entirely**;
- **follows symlinks**, guarded by a visited set of realpaths so a cycle is
  traversed at most once (`ELOOP` tolerated; depth limited to 64);
- excludes `node_modules`, `.git`, and every directory named in
  `$MDTASK_EXCLUDE_DIRS` (colon- or comma-separated);
- accepts only files whose name ends in `.md` (case-insensitive);
- is ordered by `sort.Strings` and deduplicated by realpath, keeping the first
  spelling; and
- excludes the resolved `archivePath`, so archived tasks never re-enter `list`
  or `ids`.

An unreadable directory is skipped with exactly one stderr line, `mdfu task:
skipping <dir>: <err>`, which does not affect the exit code. Include globs are
applied first, then exclude globs. A file named by `--path` directly is the
whole scope, with no walk.

## Commands

`mdfu task` alone lists; `mdfu task <ID>` views that task (an ID is either
`PREFIX-<n>` or a bare number). Any other unknown token is a fuzzy search
instead of an error (divergence D2). The explicit `mdfu task -- <command>`
form dispatches to the command table and prints the branded usage block on an
unknown command (exit 1).

| Command | Synopsis |
|---|---|
| `list` | List tasks (`--all`, `--blocked`, `--json`, `--sort`, `--tag`, `--priority`). |
| `view` / `show` | Show one task by ID (`--json`). |
| `open` | Open a task at its line in `$EDITOR`. |
| `move` | Move a task block to another file. |
| `set` | Append metadata tokens to tasks. |
| `ids` | Assign IDs to seed and unidentified lines (`--prefix`). |
| `archive` | Archive done tasks. |
| `validate` | Validate the scope. |
| `install-skills` | Install the mdfu task skills into a directory. |

Every subcommand also accepts the global `--path <dir|file>`.

### list

`mdfu task list [--all] [--blocked] [--json] [--sort priority] [--tag T]… [--priority P]… [#tag…] [!priority…]`

By default only open, unblocked tasks are shown. `--all` includes done tasks;
`--blocked` includes tasks with unresolved blockers. `--tag` is repeatable and
AND-combined, `--priority` is repeatable and OR-combined; a positional `#tag`
or `!priority` adds to the corresponding filter. `--sort priority` orders
`crit`, `high`, `medium`, `low`; any other value is not an error and leaves file
order.

Three render modes are mutually exclusive: the TTY box table (columns `ID`,
`TITLE`, `TAGS`, `PRI`, `PROPS`), the non-TTY line format, and `--json` (one
indented array with keys `id`, `title`, `status`, `priority`, `tags`,
`properties`, `file`, `line`). Two trailing notes are written to stdout and
suppressed by `--json`: one naming hidden blocked tasks, and one reporting
seed/unidentified lines.

### view / show

`mdfu task view <id> [--json]` prints the location (TTY only), the **verbatim
original header**, and the body indented by six spaces. `--json` emits the
`list` keys plus `body` last, and never hides a resolved blocker.

### open

`mdfu task open <id>` runs `$EDITOR` with `+<line> <file>` appended after any
arguments already in `$EDITOR`, inheriting its stdio and exit code. An unset or
empty `$EDITOR` is `mdfu task: $EDITOR is not set` (exit 1).

### move

`mdfu task move <id> <file>` relocates the whole block (header, body, and the
blank lines separating it) to the end of the target, creating parent
directories. The target is written before the source is rewritten; moving onto
the same realpath is a silent no-op. A stale source is rejected.
`install-skills` and `archive` write nothing to stdout.

### set

`mdfu task set <id…> <token…>` appends metadata tokens to each named task
(IDs and tokens may be comma- or space-separated). An already-present tag is
skipped; a new priority replaces the existing one in place; a metadata-free
header receives a literal `\t\t` before the tokens. At least one ID and one
token are required.

### ids

`mdfu task ids [--prefix PREFIX]` assigns IDs to every seed and unidentified
line in scope. Prefix resolution is all-or-nothing: no file is written unless
every file's prefix resolves. Prefix sources, in order, are the most frequent
existing prefix in that file, a seed line, `--prefix` (uppercased before
validation), then an interactive prompt. New numeric parts start at
`GlobalMax+1`, zero-padded to `max(3, digits(GlobalMax))`. On success each
rewritten header is printed in assignment order; duplicate numeric parts warn on
stderr at exit 0.

### archive

`mdfu task archive [ids…]` archives every done task in scope, or exactly the
named ones. A named task that is not done is `mdfu task: task '<id>' is not
done` (exit 1). The destination is `<basePath>/<archivePath>`; an archive path
escaping the base, a directory destination, and an unwritable destination are
all rejected before any write. Archiving nothing is a silent exit 0.

### validate

`mdfu task validate` scans every file in scope, honouring `excludePrefixes`, and
writes to **stderr**: one `error: duplicate ID '<id>' in <file>:<line>,
<file>:<line>` per duplicate-ID group, plus `warning:` lines for a duplicate
numeric part, an empty tag, malformed metadata, and an unknown priority. The
exit code is 1 only when an `error:` line was emitted; warnings never change it.

### install-skills

`mdfu task install-skills <dir>` writes the three embedded skill documents to
`<dir>/<skill-name>/SKILL.md`, creating directories and overwriting any existing
file, then prints `Installed <name> into <dir>` per skill. The skill names are
`mdfu-task`, `mdfu-task-add`, and `mdfu-task-do`. Files are written, never
symlinked. A missing `<dir>` is a usage error (exit 2).

## Divergences (D1–D5)

Everything else matches the frozen specification; these five differences are
intentional:

| ID | Divergence | Rationale |
|---|---|---|
| D1 | The stderr error prefix is `mdfu task: `, not the original tool's. | Rebrand stderr. |
| D2 | `mdfu task <unknown-token>` performs a fuzzy search instead of exiting 1 with an unknown-command error. | Known-subcommand dispatch with fall-through, so existing searches keep working. |
| D3 | `install-skills` writes three mdfu-named skills and prints mdfu-branded lines. | A renamed asset cannot be byte-identical; the command exists for CLI-surface completeness. |
| D4 | `mdfu task --help` / `--version` print mdfu-branded text. | An mdfu binary must not emit a foreign tool's help. |
| D5 | The two `list` note sentences name `mdfu task`. | An mdfu binary must not instruct users to run another tool. |

## Compatibility

The format and command surface documented here are compatible with the external
`mdtask` tool so that an existing user can switch by changing the command name.
The specification was authored from public written documentation only; the
project claims no endorsement by, or verified equality against, that tool.

See also: the [Tasks TUI](/guide/tasks-tui/) for the interactive browser, and
the [CLI](/reference/cli/) reference for the root flag surface.
