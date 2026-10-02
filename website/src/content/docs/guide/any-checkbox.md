---
title: Any Checkbox
description: How the mdfu tasks browser recognises and edits every markdown checkbox — any bullet, any indent, [ ]/[x]/[X] — while the CLI stays strict.
---

The `mdfu tasks` browser lists **every markdown checkbox** in scope, not only
mdtask-formatted tasks. This page documents what qualifies, what you can do with
an item, and how the browser stays out of the way of the strict
[`mdfu task`](/guide/tasks/) commands.

## Recognised forms

A line is shown as a checkbox item when it has all of:

| Part | Accepted |
|---|---|
| Indentation | Any run of spaces or tabs (nested items are included). |
| Bullet | `-`, `*`, or `+`. |
| Checkbox | `[ ]` (open), `[x]` (done), or `[X]` (done, uppercase). |
| Separator | A single space after the closing bracket. |
| Text | Any remainder: a plain title, or an mdtask ID, metadata, and body. |

```markdown
- [ ] plain item with no ID
  * [x] indented sub-item
+ [X] another bullet and an uppercase X
- [ ] PRJ-42 an mdtask-identified task #launch
```

All four appear in the list. An item may carry an mdtask ID, metadata tokens
(`#tag`, `!priority`, `@key:value`), and an indented body, exactly as the
[mdtask format](/guide/tasks/#task-format) describes — but none of that is
required.

## Ignored lines

The browser deliberately does **not** treat these as items:

- lines inside a fenced code block (```` ``` ```` or `~~~`), which are masked
  exactly as they are for the CLI;
- block quotes (`> - [ ] …`); and
- ordered-list items (`1. [ ] …`).

Plain, non-checkbox list items are never shown.

## Operations

Every recognised item supports the full set of browser actions, whether or not
it has an ID:

| Key | Action | Behaviour on a non-mdtask item |
|---|---|---|
| `x` | Toggle done | Flips `[ ]` ↔ `[x]`, preserving the original bullet and indentation. An uppercase `[X]` flips to `[ ]`. |
| `t` | Edit title/metadata | Seeds the raw line; the bullet, indent, and checkbox are preserved unless you change them. |
| `e` | Edit body | Re-indents by the item's existing body indent (default two spaces). |
| `o` | Open in `$EDITOR` | Jumps to the item's line. |
| `m` | Move to file | Moves the item's whole block to the target file. |
| `a` | Archive | Archives only a **done** item; an open item writes nothing. |
| `n` | New item | Seeds `- [ ] `; saved to the `--path` file or the focused item's file. |

The selection flags (`--all`, `--blocked`, `--sort`, `--tag`, `--priority`) are
unchanged; see the [Tasks TUI](/guide/tasks-tui/) page for the full keymap and
pane layout.

## Identity and nesting

An item without an ID is identified by its **file and line**, which stays
correct even when two lines have identical text. An item **with** an ID keeps
that ID as its identity, so the cursor follows it across a move.

A nested checkbox is its own item and **ends its parent's block**. A sub-item's
text is therefore never absorbed into the parent's body:

```markdown
- [ ] parent            ← item; body is "parent note"
  parent note
  - [ ] child           ← separate item; body is "child note"
    child note
```

Moving or archiving a parent moves or archives only that parent; the nested
items stay where they are and can be handled individually.

## The CLI stays strict

This breadth is a **browser-only** behaviour. The non-interactive `mdfu task …`
subcommands keep the strict mdtask grammar exactly, so their output remains
byte-compatible with the [specification](/guide/tasks/#task-format). In
particular:

- `mdfu task list`, `view`, `open`, `move`, `set`, `archive`, and `validate`
  see only mdtask-formatted tasks;
- when stdout is not a terminal, `mdfu tasks` falls back to
  `mdfu task list`, so a piped `mdfu tasks` is byte-identical to the CLI and
  does not show arbitrary checkboxes.

To promote ID-less items into first-class tasks, run
[`mdfu task ids`](/guide/tasks/#ids) to assign IDs.

See also: [Tasks](/guide/tasks/) for the strict format and commands, and
[Tasks TUI](/guide/tasks-tui/) for the browser's layout and keybindings.
