# mdfu task

`mdfu task` is the command surface for checkbox tasks that live directly in
Markdown files. This document describes the on-disk task format and the
subcommands you can run against it. Nothing here is tied to a particular editor
or agent: the format is plain text and every operation is an ordinary CLI call.

## The task format

A task starts with a single line called the header and may be followed by an
indented body. The header looks like this:

```
- [ ] PREFIX-123 Title text
```

- The checkbox is `- [ ]` for an open task and `- [x]` for a done task. Only a
  lowercase `x` marks a task done.
- `PREFIX-123` is the identifier. The prefix is an uppercase letter followed by
  uppercase letters or digits, and the numeric part is unique across the whole
  scope.
- The rest of the line is the title.

Metadata is appended after the title. A header may separate the title from its
metadata with two tab characters, or simply with a space when the trailing words
are all valid tokens. Three token kinds are recognised:

- a tag, written `#name`;
- a priority, written `!high` (the explicit values are `crit`, `high`, and
  `low`; a task with no priority is treated as medium);
- a property, written `@key:value`. The built-in property `blocked_by` names one
  or more task identifiers, comma separated.

Any line following the header whose non-empty lines are all indented belongs to
the task body and is shown indented under the task. Blank lines inside the body
do not end it. A fenced code block masks header detection, so a checkbox line
inside a code fence is never a task.

A checkbox line that carries no identifier is either a *seed* line
(`- [ ] PREFIX- Title`) or an *unidentified* line (`- [ ] Title`). Both are
surfaced together so identifiers can be assigned in a later step.

## Configuration

An optional `.mdfurc` JSON file is discovered by walking up from the working
directory. It can set the base `path`, the `files.include` and `files.exclude`
globs, the `excludePrefixes` list, and the `archivePath`. The global `--path`
flag overrides the configured base for one invocation and accepts either a
directory or a single file.

## Command surface

- `mdfu task list` prints the open tasks. `--all` also shows done tasks,
  `--blocked` shows blocked ones, and `--tag <tag>`, `--priority <p>`,
  `--sort priority`, and `--json` shape the result.
- `mdfu task view <id>` (alias `show`) prints one task; `--json` adds its body.
- `mdfu task open <id>` opens the task's file at its line in `$EDITOR`.
- `mdfu task set <id...> <token...>` appends metadata to one or more tasks.
- `mdfu task ids` assigns identifiers to seed and unidentified lines.
- `mdfu task move <id> <file>` relocates a task block to another file.
- `mdfu task archive [id...]` moves done tasks into the archive file.
- `mdfu task validate` reports duplicate identifiers and metadata problems on
  stderr. Only duplicate identifiers make it exit non-zero.
- `mdfu task install-skills <dir>` writes the mdfu task skill documents to disk.

Success is silent for the mutating commands, so check the files or run
`mdfu task list` afterwards to confirm a change.
