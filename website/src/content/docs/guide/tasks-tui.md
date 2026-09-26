---
title: Tasks TUI
description: Interactive task browser — pane layout, keybindings, edit modes, and the non-TTY fallback.
---

`mdfu tasks` opens the full-screen task browser:

```sh
mdfu tasks [--all] [--blocked] [--sort priority] [--tag T]… [--priority P]… [#tag…] [!priority…]
```

The selection flags match [`mdfu task list`](/guide/tasks/#list): by default
only open, unblocked tasks are listed, `--all` includes done tasks, `--blocked`
starts with blocked tasks shown, `--tag` is repeatable and AND-combined,
`--priority` is repeatable and OR-combined, and positional `#tag` / `!priority`
arguments add to those filters. `--path` selects the scope. Everything the
browser reads and writes is the same [task format](/guide/tasks/) the CLI uses.

## Pane layout

The browser is built from:

- a **filter line**, shown above the list only while filter mode is active;
- a **list pane** of tasks — one row per task, showing the checkbox, ID, title,
  tags, explicit priority, and display properties;
- a **detail pane** for the focused task — its rendered header, labelled
  metadata rows (`ID`, `Status`, `Title`, `Priority`, `Tags`, each `@property`,
  and `File:line`), then its Markdown body with clickable links; and
- a **status/help footer**.

At 100 columns or wider the list and detail panes sit side by side; below that
they stack. On the list, the cursor row is drawn in the selected style, done
rows are dimmed, and an unresolved `@blocked_by` value is drawn in the blocker
color. The footer reads `matched/total • blocked:hidden|shown • x:done • t:edit
• ?:help`, followed by the last status message.

## Keybindings (`internal/tui/tasks_model.go` `DefaultTaskKeyMap`)

| Key | Action |
|---|---|
| `up` / `k` | Cursor up (wraps). |
| `down` / `j` | Cursor down (wraps). |
| `pgup` / `ctrl+b` | Page up. |
| `pgdn` / `ctrl+f` | Page down. |
| `home` / `g` | Top of list. |
| `end` / `G` | Bottom of list. |
| `/` | Enter filter mode. |
| `esc` | Browse: clear the filter, or quit when it is empty. Filter: clear and return to browse. Edit: cancel. |
| `tab` | Toggle focus between the list and detail panes. |
| `x` | Toggle the focused task's done checkbox. |
| `b` | Toggle showing blocked tasks. |
| `o` | Open the focused task in `$EDITOR`. |
| `t` | Edit the focused task's title/metadata. |
| `e` | Edit the focused task's body. |
| `n` | Create a new task. |
| `m` | Move the focused task to another file. |
| `a` | Archive the focused task. |
| `?` | Show full help. |
| `q` / `ctrl+c` | Quit. |
| `ctrl+s` | Save (edit modes only). |

## Modes

**Browse.** Keys act on the focused task. `x` writes the checkbox flip through
the same file editor the CLI uses and reloads; `b` flips blocked visibility and
refilters; `o` suspends the TUI to run `$EDITOR +<line> <file>` and reloads on
return (a missing `$EDITOR` sets the status line). `a` archives only a done
task — an open task sets the status `task is not done` and writes nothing.

**Filter.** `/` focuses the filter box. Only `esc`, `q`/`ctrl+c`, and
`up`/`down` are intercepted; every other printable key is typed into the query,
so `x` and `b` do not toggle anything while filtering. The query is matched
case-insensitively against ID, title, priority, tags, and property keys and
values, with whitespace-separated tokens AND-combined.

**Help.** `?` shows the full grouped help; `?` or `esc` dismisses it.

## Edit modes

- **Title/metadata (`t`).** A textarea seeded with the raw header, plus a live
  side panel showing the parsed ID, status, title, tags, priority, and
  properties. `ctrl+s` saves; an unparseable header stays open with the status
  `invalid task header` and writes nothing. `esc` cancels.
- **Body (`e`).** A full-width textarea seeded with the dedented body. On save
  the body is re-indented with the task's existing body indent (default two
  spaces); blank lines stay blank. `ctrl+s` saves, `esc` cancels.
- **New task (`n`).** Seeds `- [ ] ` in the title editor. Saving appends the
  block to the `--path` file when the scope is a single file, otherwise to the
  focused task's file. A header without an ID is written verbatim and the status
  points at `mdfu task ids`.
- **Move (`m`).** A prompt seeded with the focused task's current file. `enter`
  moves the block to the destination (creating parent directories), `esc`
  cancels.

Every write goes through the same engine primitives as the CLI, so a stale file
is detected and reported instead of overwriting.

## Non-TTY fallback

When stdout is not a terminal, `mdfu tasks` does not start the full-screen
program. It runs `mdfu task list` with the same arguments, byte-identical to the
CLI command, with no alternate screen and no ANSI output. This keeps pipeable
use (`mdfu tasks | …`) predictable.

## Divergences (D1–D5)

The task system as a whole has exactly five documented divergences from the
external `mdtask` surface: **D1** the `mdfu task: ` stderr prefix, **D2** an
unknown `mdfu task <token>` performing a fuzzy search instead of erroring, **D3**
mdfu-named skills and branded `install-skills` output, **D4** mdfu-branded help
and version text, and **D5** the two `list` notes naming `mdfu task`. They are
described in full on the [Tasks](/guide/tasks/#divergences-d1d5) page; the
browser itself follows the same format and writes the same files, so it adds no
further divergence.

See also: [Tasks](/guide/tasks/) for the file format and `mdfu task` commands,
and [TUI](/guide/tui/) for the Markdown note picker.
