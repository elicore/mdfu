# mdfu task format and command specification

Status: **normative and frozen**. Every acceptance criterion in the
`mdtask-compat-tasks-tui` work plan is a string-literal equality against this
document. Output is **byte-identical to this document**; the five documented
divergences in §K are the only permitted differences from the external tool
whose surface this reimplements.

This specification is authored by the mdfu project. It was written from public
written documentation only. It is not a transcription of any source file, and
the project makes **no claim of verified byte equality against a running copy**
of any external tool: "byte-identical" always means byte-identical to *this*
document, which is compared by automated table tests using literal expected
strings.

Normative keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used in
the RFC-2119 sense. All sizes are display cells measured with
`lipgloss.Width`. All file paths in output are slash-separated and relative to
the current working directory.

---

## A. Task format

A *task* is a single line that matches, anchored at the start of the line:

```
^- \[([ x])\] ((?:[A-Z][A-Z0-9]*-\d+ )?)(.*)$
```

The checkbox character MUST be a literal space or a lowercase `x`. An uppercase
`X` in the checkbox position is **not** a task and MUST be ignored. Leading
indentation is not permitted on a header line: a header MUST begin at column 0.
(A header nested inside a list continuation is therefore not a task.)

Capture groups: group 1 is the checkbox (`" "` or `"x"`); group 2 is the ID
prefix and dash when present (e.g. `"EXMPL-42 "`); group 3 is the remainder,
which is split into title and metadata.

### A.1 ID grammar

- An ID prefix MUST match `^[A-Z][A-Z0-9]*$`.
- A full task ID is `<PREFIX>-<n>` where `<n>` is a decimal integer written
  with one or more digits.
- The numeric part MUST be globally unique across all prefixes. Two tasks
  `EXMPL-7` and `OTHER-7` in the same scope are a duplicate-numeric-part
  condition (a warning in `validate`, an additional line in `ids`).
- The numeric part of an existing ID is preserved verbatim, including leading
  zeros (`EXMPL-007`).

Three header forms are recognised:

- **Identified:** `- [ ] EXMPL-42 Title` — group 2 supplies a valid
  `PREFIX-<n>`.
- **Seed:** `- [ ] PRJ- Title` — the ID candidate is `PRJ-` immediately
  followed by a space. A seed line has no numeric part; it names a prefix for
  `ids` to use but is not itself an identified task.
- **Unidentified:** `- [ ] Title` — no ID candidate at all.

An identified task is the only form that participates in ID resolution,
blockers, `list` rows, and `view`. Seed and unidentified lines are collected
separately and reported by the trailing note §H and by `ids` §F.6.

### A.2 Title and metadata split

Given the remainder from group 3:

1. If it contains the literal two-tab sequence `\t\t` (U+0009 U+0009), split at
   the **first** occurrence. The title is the text before the separator,
   right-trimmed of spaces and tabs; the metadata is the text after it,
   left-trimmed of spaces and tabs.
2. Otherwise, peel a *trailing run of tokens* from the right: while the last
   whitespace-delimited word of the remainder is a metadata token (per §A.3),
   remove it and prepend it to the metadata run, stopping at the first
   non-token word. The remaining text is the title, right-trimmed. This rule is
   why `#123` stays in a title (it is not a valid tag, §A.3) while `#launch`
   does not.
3. If no tokens are peeled, the metadata is empty and the title is the whole
   remainder, right-trimmed.

A single-space separator is therefore only meaningful when the text after it is
a run of valid tokens; a single space followed by ordinary prose stays in the
title.

### A.3 Metadata tokens

Metadata is a whitespace-separated sequence of tokens. Each token MUST match
exactly one of:

| token | regex | meaning |
| --- | --- | --- |
| tag | `^#[A-Za-z][\w-]*$` | zero or more tags; a tag MUST start with a letter |
| priority | `^![A-Za-z]\w*$` | a priority; the valid set is `crit`, `high`, `low` |
| property | `^@([\w-]+):(\S+)$` | a `key:value` property |

- A word that is not one of the forms above is not a token. A tag whose body
  starts with a digit (`#123`) is **not** a tag and stays in the title.
- At most one priority token is meaningful. An absent priority means
  **medium**. The valid explicit values are `crit`, `high`, and `low`; any
  other value is an unknown priority (a `validate` warning) and is treated as
  medium for sorting.
- A priority or property token is written with no internal whitespace; a
  property value MUST match `\S+`.
- Properties preserve first-seen order. The only built-in property is
  `blocked_by` (§B).
- Malformed metadata (a bare `@key` with no `:value`) is not a property token;
  it is an unknown word and `validate` reports it as a warning.

### A.4 Body

The body of a task is the maximal run of lines following the header line in
which every non-empty line is indented (has one or more leading space or tab
characters). The run ends immediately before the first non-empty line that
begins at column 0. Blank lines are permitted inside the body and do not
terminate it; a run of blank lines followed by an indented line is still body.
Trailing blank lines that are followed by a non-indented line are not part of
the body.

The body is stored dedented by the minimum indent of its non-empty lines. On
serialization the original indent of the block is reapplied (the *body indent*,
default two spaces), so a round-trip preserves the block's spelling. A body
MUST NOT be re-parsed for tasks: a body line shaped like a header is body text.

A task at end of file with no body parses with an empty body; serialization
MUST NOT append a newline that was not present.

### A.5 Fence masking

CommonMark-style fenced code blocks mask task-header detection as well as body
extraction. An opening fence is a line with up to three leading spaces followed
by three or more backticks (```` ``` ````) or three or more tildes (`~~~`); a
closing fence is a line of at least as many of the same character, optionally
indented by up to three spaces. Every line from the opening fence through the
closing fence (inclusive) is masked: a `- [ ] …` line inside a fence is **not**
a task, and its body lines MUST NOT contribute to any task's body. An unclosed
fence masks to end of file.

---

## B. Blockers

`@blocked_by` is the only built-in property. Its value names one or more task
IDs, comma-separated (e.g. `@blocked_by:EXMPL-1,EXMPL-2`).

A blocker is **resolved** when the named ID belongs to a task that is done. A
blocker whose ID does not correspond to any task in scope is **unresolved**. An
open task with at least one unresolved blocker is *blocked*.

- A done task never reports unresolved blockers, even if it carries a
  `@blocked_by` value.
- In `list` output, resolved blockers are stripped from the rendered
  properties; unresolved blockers are shown and rendered in the blocker color
  under §G.
- In `view` output, the verbatim original header is shown, so resolved
  `@blocked_by` tokens remain visible.
- `DisplayProperties` renders every non-`blocked_by` property in original order
  first, then `blocked_by` last, in both cases preserving relative order.

---

## C. Configuration

The task configuration file is JSON. Two names are recognised: `.mdtaskrc`
(the original name) and `.mdfurc` (the mdfu name). `.mdfurc` is the **task**
configuration only; it is unrelated to the YAML theme configuration read from
`$XDG_CONFIG_HOME/mdfu/config.yaml` or passed via `--config`.

Resolution walks upward from the start directory. **At each directory level the
search prefers `.mdtaskrc` and uses `.mdfurc` only if that level has no
`.mdtaskrc`.** The walk terminates at the filesystem root. A nearer `.mdfurc`
therefore wins over a farther `.mdtaskrc`, but at any single level `.mdtaskrc`
wins.

Schema:

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

| key | type | meaning |
| --- | --- | --- |
| `path` | string | base directory, resolved relative to the config file's directory |
| `files.include` | array of string | globs selecting files; **empty means every `.md` file** |
| `files.exclude` | array of string | globs removing files; applied after include |
| `excludePrefixes` | array of string | prefixes whose tasks `validate` MUST NOT report |
| `archivePath` | string | archive file, relative to the base directory |

`archivePath` defaults to `_archive.md`.

Validation is defensive and never fails on a wrongly-typed value: a non-array
`files.include`/`files.exclude`/`excludePrefixes` entry is dropped; a
non-object `files` is ignored; a non-string `path` or `archivePath` is ignored;
an unknown key is ignored. Malformed JSON is fatal:
`Invalid JSON in <path>: <message>`. An rc path that is unreadable or is a
directory is fatal: `mdfu task: cannot read <path>: <err>`.

Base-path precedence, highest first:

1. `--path` (a flag or the `path` argument of a subcommand)
2. `$MDTASK_PATH`
3. the config file's `path`, resolved relative to the config file's directory
4. `.`

When no configuration file exists, the base directory is the nearest ancestor
containing `.git`, falling back to the start directory.

Globs are matched relative to the base directory against the slash-separated
relative path.

---

## D. Glob subset (`**`)

The matcher is hand-rolled. A third-party doublestar library was rejected to
keep the direct dependency set at the existing six modules. Exactly four rules
apply:

1. `**` matches zero or more path segments. `**/*.md` matches `a.md`,
   `x/a.md`, and `x/y/z.md`.
2. `*` matches any run of characters other than the path separator `/`.
   `*.md` matches `a.md` but **not** `x/a.md`.
3. `?` and `[...]` are **literal** characters; there is no single-character
   wildcard and no character class.
4. A pattern ending in `/**` requires at least one following segment:
   `archive/**` matches `archive/a.md` and `archive/a/b.md` but **not**
   `archive` itself.

A pattern that matches no separator still matches at any depth only via the
`**` rule; there is no implicit prefix matching.

---

## E. Discovery

Scope discovery walks the base directory and:

- **includes** hidden files and hidden directories (names beginning with `.`);
- **ignores `.gitignore` entirely**;
- **follows symlinks**, guarded by a visited-set of `filepath.EvalSymlinks`
  realpaths so a cycle is traversed at most once; `ELOOP` is tolerated; the
  walk depth is limited to 64;
- excludes `node_modules`, `.git`, and each directory named in
  `$MDTASK_EXCLUDE_DIRS` (colon- or comma-separated);
- accepts only files whose name ends in `.md` (case-insensitive);
- is ordered by `sort.Strings` and deduplicated by realpath, keeping the first
  spelling encountered;
- excludes the resolved `archivePath`, so archived tasks never re-enter `list`
  or `ids`.

An unreadable directory is skipped with exactly one line on stderr,
`mdfu task: skipping <dir>: <err>`, and does not affect the exit code.

Include globs are applied first (an empty include list means every `.md`
file), then exclude globs. A file named by `--path` directly is used as the
whole scope without a walk.

---

## F. Output byte rules

Three render modes exist for `list`: the TTY box table, the non-TTY line
format, and JSON. They are mutually exclusive. `--json` never emits ANSI and
never emits a note.

### F.1 `list` — JSON

`list --json` writes a single indented JSON array (2-space indent, produced by
`encoding/json` with `SetEscapeHTML(false)` and `SetIndent("", "  ")`). Every
object has exactly these keys, in exactly this order:

1. `id` — string
2. `title` — string
3. `status` — `"open"` or `"done"`
4. `priority` — `"crit"`, `"high"`, `"medium"`, or `"low"` (never empty; the
   absent priority is `"medium"`)
5. `tags` — array of strings (always an array; `[]` when empty)
6. `properties` — JSON object (always an object; `{}` when empty; keys are
   emitted in lexicographic order by `encoding/json`)
7. `file` — string, slash-separated path relative to the working directory
8. `line` — integer, 1-based header line number

An empty result emits `[]` followed by a newline. The encoder appends one
trailing newline. `SetEscapeHTML(false)` means a title containing `<b>&</b>` is
emitted literally and contains no `\u003c`.

### F.2 `list` — non-TTY line format

When `IsTTY` is false, each visible task is one line:

```
<checkbox> <ID>  <title>[  #tag…][  !priority][  @key:value…]
```

- Fields are joined by exactly two spaces.
- `<checkbox>` is `[ ]` or `[x]`.
- A field is omitted entirely (including its separating two spaces) when
  empty. Tags are space-separated and each begins with `#`; priority is present
  only when explicitly set; properties come from `DisplayProperties`.
- Output contains zero `\x1b` bytes.

### F.3 `list` — TTY box table

When `IsTTY` is true the rows are drawn as a rounded box with columns, in
order: `ID`, `TITLE`, `TAGS`, `PRI`, `PROPS`. Each cell's content width is the
maximum `lipgloss.Width` of the header and every row value in that column. Each
cell is rendered as one space, the value padded on the right with spaces to the
column width, and one space. The border runes are `╭ ╮ ╰ ╯ ─ │ ┬ ┴ ┼ ├ ┤`.

The `TITLE` value is truncated to 120 display cells: when its width exceeds
120, the first 119 cells are kept and `…` (U+2026) is appended.

For the task set

| id | checkbox | title | tags | priority | properties |
| --- | --- | --- | --- | --- | --- |
| EXMPL-001 | open | `Fix the thing` | `launch` | `high` | `status:doing` |
| EXMPL-002 | done | `Ship it` | `launch` | (none) | (none) |

the exact bytes of the box are:

```
╭───────────┬───────────────┬─────────┬───────┬───────────────╮
│ ID        │ TITLE         │ TAGS    │ PRI   │ PROPS         │
├───────────┼───────────────┼─────────┼───────┼───────────────┤
│ EXMPL-001 │ Fix the thing │ #launch │ !high │ @status:doing │
│ EXMPL-002 │ Ship it       │ #launch │       │               │
╰───────────┴───────────────┴─────────┴───────┴───────────────╯
```

The requested sort (`--sort priority`) orders `crit`, `high`, `medium`, `low`
and leaves file order untouched for any other value, which is not an error.

### F.4 `view` / `show`

Human output consists of:

1. when `IsTTY` is true, a location line `<file>:<line>` styled under §G;
2. the **verbatim original header line** of the task, including resolved
   `@blocked_by` tokens;
3. when the body is non-empty, each body line indented by exactly six spaces;
   blank body lines are emitted as empty lines.

`view --json` emits one object with the eight keys of §F.1 in order, plus
`body` **last**, whose value is the dedented body text. It emits neither the
location line nor the header line.

Errors are written to stderr as `mdfu task: <msg>` and exit 1 with **no**
stdout output.

### F.5 `open`, `move`

Neither writes to stdout on success. `open` resolves the ID and executes
`$EDITOR` with the argument list `+<line> <file>` appended after any arguments
present in `$EDITOR` itself, inheriting the child's stdio and propagating its
exit code. `$EDITOR` is split on whitespace: the first field is the program and
the remaining fields precede the `+<line> <file>` arguments. An unset or empty
`$EDITOR` is `mdfu task: $EDITOR is not set` at exit 1.

`move <id> <file>` relocates the whole block (header, body, and the blank lines
that separated it from its neighbours) to the end of the target, creating
parent directories as needed. Order is exact: validate the ID; validate the
target is not a directory; validate writability of both files; `Verify` the
source is not stale; **write the target first**; then rewrite the source with
the block removed. When source and target resolve to the same realpath the
command is a silent no-op exit 0 touching neither file.

### F.6 `set`, `ids`

`set <id…> <token…>` (IDs and tokens comma- or space-separated) appends tokens
to each named task's header. At least one ID and one token are required, else
`no task IDs provided` / `no metadata tokens provided` at exit 1. An
already-present tag is skipped. A new priority replaces any existing priority
token in place. When the header has no metadata the tokens are appended after a
literal `\t\t`. The title is preserved verbatim. Nothing is written to stdout.

`ids` assigns IDs to seed and unidentified lines using the pure planning
functions, honouring an all-or-nothing two-pass rule: no file is written unless
every file's prefix resolves. Prefix sources, in order: the most frequent
existing prefix in that file; a seed line; `--prefix <PREFIX>` (uppercased
before validation); the interactive prompt `Enter prefix for <relpath>: ` read
from stdin when it is a TTY. An invalid answer is `invalid prefix '<prefix>'`
at exit 1. New numeric parts start at `GlobalMax+1` and are zero-padded to
`max(3, digits(GlobalMax))`. On success one line per assignment is written to
stdout in assignment order, each being the task's rewritten header line (with
its new ID). Duplicate numeric parts additionally print
`warning: duplicate numeric part <n> across prefixes: <ids>` on stderr at exit
0. A failed run writes nothing to stdout.

### F.7 `archive`

`archive [ids…]` with no IDs archives every done task in scope; with IDs it
archives exactly those, and any named task that is not done is
`mdfu task: task '<id>' is not done` at exit 1. The destination is
`<basePath>/<archivePath>`. An archive path escaping the base is `archive path
'<path>' is outside the base directory`; a directory is `'<path>' is a
directory`; an unwritable file is `cannot write to '<path>': permission
denied`. All preconditions are validated before any write, no source may be
stale, all blocks are appended to the archive before any source is rewritten,
and archiving nothing is a silent exit 0. Nothing is written to stdout.

### F.8 `validate`

`validate` scans every file in scope, honouring `excludePrefixes`, and writes
to **stderr**:

- one `error: duplicate ID '<id>' in <file>:<line>, <file>:<line>` per
  duplicate-ID group;
- `warning: duplicate numeric part <n> across prefixes: <ids>` for a duplicate
  numeric part;
- `warning:` lines for an empty tag, malformed metadata (`@key` with no
  `:value`), and an unknown priority.

The exit code is 1 when any `error:` line was emitted and 0 otherwise; warnings
never change the exit code. Nothing is written to stdout, and a task whose
prefix is in `excludePrefixes` is not reported.

### F.9 `install-skills`

`install-skills <dir>` writes the three embedded skill files to
`<dir>/<skill-name>/SKILL.md`, creating directories as needed and overwriting
any existing file, then prints one line per skill to stdout in the form
`Installed <name> into <dir>` and exits 0. Files are **written, never
symlinked** (an embedded asset has no on-disk path, and this also avoids any
Windows symlink privilege). The skill names are `mdfu-task`, `mdfu-task-add`,
and `mdfu-task-do`; the literal names `mdtask`, `mdtask-add`, and `mdtask-do`
MUST NOT appear in the assets. A missing `<dir>` argument is a usage error at
exit 2. A write failure is `cannot write to '<path>': permission denied` at
exit 1. No pre-existing file other than those three `SKILL.md` paths is
touched, listed, or removed.

---

## G. ANSI enable rule

ANSI (SGR) sequences are emitted **if and only if** `Terminal.IsTTY` is true
**and** `Terminal.NoColor` is false. They are never emitted in `--json` mode
and never when `IsTTY` is false. The SGR parameters are:

| element | SGR |
| --- | --- |
| task ID | `38;5;245` |
| done row | `38;5;240` |
| priority | `38;5;212` |
| tag | `38;5;62` |
| unresolved blocker | `38;5;203` |
| selected row | `1;38;5;82` |
| `view` location line | `38;5;245` |

Each styled span is written as `ESC[` + parameters + `m`, and terminated by
`ESC[0m` at the end of the span. Non-TTY output contains zero `\x1b` bytes.

---

## H. The two trailing notes

Both notes are written to **stdout**, are unprefixed, and are suppressed
entirely by `--json`. Each paragraph ends with a newline.

**Note 1 — hidden blocked tasks.** Emitted when at least one open task in scope
has an unresolved blocker and `--blocked` was not given. Its text names the
`mdfu task` command (divergence D5):

```
Warning: <N> blocked task(s) hidden (use `mdfu task list --blocked` to show them):
<raw header line>  <file>:<line>
```

one detail line per hidden task, where `<raw header line>` is the verbatim
source line and the `<file>:<line>` field is aligned to the same column across
all detail lines (padded on the left of the location with spaces so that every
`<file>` begins at the column after the widest raw line plus two spaces).

**Note 2 — unidentified tasks.** Emitted when the scope contains at least one
seed or unidentified line:

```
<N> task(s) have no ID (run `mdfu task ids` to assign one).
```

---

## I. Exit codes

| code | condition |
| --- | --- |
| 0 | success, including `--sort` with an unrecognised value and `validate` with warnings only |
| 1 | any task, ID-resolution, IO, configuration, stale-file, archive, or validate-error condition; also an unknown subcommand reached through the explicit `mdfu task --` path |
| 2 | usage error: an unknown flag, a flag missing its value, a missing required argument, an unexpected positional argument, or a root flag appearing before the task subcommand |

A failure path writes nothing to stdout.

---

## J. Error strings

The following are the error strings of this specification. Runtime errors are
prefixed `mdfu task: ` (divergence D1) unless shown without a prefix, which
denotes a bare message returned by the engine.

| id | string |
| --- | --- |
| E1 | `mdfu task: unknown command '<cmd>'` |
| E2 | `mdfu task: --<flag> must come after the task subcommand` |
| E3 | `mdfu tasks: unexpected argument '<arg>'` |
| E4 | `mdfu task: --path '<x>' does not exist or is not a file/directory` |
| E5 | `mdfu task: cannot read <path>: <err>` |
| E6 | `Invalid JSON in <path>: <message>` |
| E7 | `mdfu task: skipping <dir>: <err>` |
| E8 | `mdfu task: $EDITOR is not set` |
| E9 | `invalid task ID '<input>'` |
| E10 | `task '<id>' not found` |
| E11 | `ambiguous numeric ID '<n>' matches: <id1>, <id2>` |
| E12 | `task '<id>' appears multiple times; expected exactly one match` |
| E13 | `invalid prefix '<prefix>'` |
| E14 | `no task IDs provided` |
| E15 | `no metadata tokens provided` |
| E16 | `<relpath>:<line>: no prefix found for task "<rawLine>" — add a task with an ID, use a seed line like '- [ ] PRJ- Task title', or pass --prefix PRJ` |
| E17 | `file changed, task '<id>' not at expected line` |
| E18 | `mdfu task: task '<id>' is not done` |
| E19 | `archive path '<path>' is outside the base directory` |

Additional filesystem errors referenced by §F: `cannot write to '<path>':
permission denied`, `'<path>' is a directory`, and the `validate` diagnostic
`error: duplicate ID '<id>' in <file>:<line>, <file>:<line>`.

---

## K. Documented divergences (exactly five)

| id | divergence | rationale |
| --- | --- | --- |
| D1 | the stderr error prefix is `mdfu task: `, not the original tool's | rebrand stderr |
| D2 | `mdfu task <unknown-token>` performs a fuzzy search instead of exiting 1 with an unknown-command error | known-subcommand dispatch with fall-through, so existing searches keep working |
| D3 | `install-skills` writes three mdfu-named skills and prints mdfu-branded lines | a renamed asset cannot be byte-identical; the command exists for CLI-surface completeness |
| D4 | `mdfu task --help` / `--version` print mdfu-branded text | an mdfu binary must not emit a foreign tool's help |
| D5 | the two `list` note sentences name `mdfu task` | an mdfu binary must not instruct users to run another tool |

Everything else MUST match this document.

---

## L. Failure modes

| case | chosen behavior |
| --- | --- |
| every mutation | write a temp file in the same directory, then `os.Rename`, preserving the original file mode |
| multi-file command where write N of M fails | validate ALL preconditions first, then write sequentially; earlier files stay committed, replicating validate-all-then-write-sequentially; report the failing path on stderr, exit 1 |
| source file changed under us | re-read and verify the task is still at its recorded line with an unchanged header, else `file changed, task '<id>' not at expected line`, exit 1, zero writes |
| symlink cycle in the walked tree | realpath visited-set; skip an already-visited realpath; tolerate `ELOOP`; depth limit 64 |
| unreadable directory during the walk | skip it, warn once on stderr as `mdfu task: skipping <dir>: <err>`, continue, exit code unaffected |
| CRLF line endings | detect per file; strip `\r` for parsing; restore the original ending on write |
| no trailing newline at EOF | preserved exactly |
| invalid UTF-8 bytes | raw bytes preserved on write; `strings.ToValidUTF8` applied only in the render layer |
| header containing more than one `\t\t` | split at the **first** occurrence |
| `.mdtaskrc`/`.mdfurc` unreadable or a directory | `mdfu task: cannot read <path>: <err>`, exit 1; walk-up terminates at the filesystem root |
| archive file rediscovered by the walker | the resolved `archivePath` is excluded from discovery, so archived tasks never re-enter `list --all` or `ids` |
| `$EDITOR` containing arguments | split on whitespace; the first field is the program, the rest precede the `+<line> <file>` arguments |
| `$EDITOR` unset or empty | `mdfu task: $EDITOR is not set`, exit 1 |
| task block at EOF with no body | parses with an empty body; serialization does not append a newline |
| a task header inside a fenced code block | is **not** a task — fence masking applies to header detection, not only to body extraction |

---

## M. Provenance

The format and command surface documented here are compatible with the
external `mdtask` tool so that an existing user can switch by changing the
command name. This document was authored by the mdfu project from public
written documentation only; no source file was read, translated, or ported, and
the external binary is never executed in any test, CI job, or acceptance
command. The project makes no claim of endorsement by, or verified equality
against, that tool.
