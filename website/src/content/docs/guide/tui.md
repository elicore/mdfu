---
title: TUI
description: Interactive picker — live filtering, keybindings, preview pane.
---

Launch by omitting `--filter`:

```sh
mdfu [--root DIR] [--hidden] [--limit N] [--archived] [--no-hyperlinks]
```

Type to narrow — bare words fuzzy, `key:value` hard-filters via the live `FilterFunc` (`scan → parse → query → rank` on every keystroke). Query terms are emphasized (yellow background) in both the result list and the preview pane; matches compose with the markdown syntax highlighting. Editing the query re-anchors the cursor: if the selected document survives into the new results it stays selected, otherwise the cursor resets to the top. `Enter` prints the selection (pipeable to editors/fzf flows).

## Keybindings (`internal/tui/tui.go:Model.Update`)

| Key | Action |
|---|---|
| `up` / `ctrl+k` | Cursor up (wraps). |
| `down` / `ctrl+j` | Cursor down (wraps). |
| `enter` | Confirm: multi-selection if any, else cursor item. |
| `esc` / `ctrl+c` | Abort (no output). |
| `tab` | Toggle multi-select on cursor item (the checkbox column appears only once a selection exists). |
| `ctrl+a` | Toggle archived visibility (default hidden). |
| `ctrl+p` | Toggle preview pane (glamour-highlighted markdown preview of the first 30 body lines + Title/Path/Type/Tags/Status; side-by-side when ≥100 cols). |
| `ctrl+o` | Open the previewed document's first web link (its `Resource`, else the first link in the body) in the default browser. |
| other | Edit query and refilter. |

Status bar: `matched/total • archived:hidden|shown • tab:multi • enter:select`.

## Links

In the preview, markdown links render as their label only; the target is carried
as an OSC 8 terminal hyperlink so `Ctrl`/`Cmd`-click opens it without cluttering
the pane. The document's `Resource` frontmatter is shown as a clickable
`Resource:` row. `ctrl+o` opens the same link from the keyboard.

Only `http`, `https` and `mailto` targets are made clickable — relative paths
and anchors render as plain label text. Links inside code spans and fenced code
blocks are left literal.

On terminals without OSC 8 support, pass `--no-hyperlinks`: links fall back to
the usual `label url` rendering so the target stays visible and copyable.

## Screenshot

Headless `Model.View()` capture:

```text
> Search... (bare words fuzzy, key:value hard-fil…
> Ship mdfu MVP [Task]  testdata/portent-task.md
  Monthly Active Users [Metric]  testdata/okf-v02-metric.md
  Legacy Retention Claim [Claim]  testdata/okf-v01-legacy.md
  Generic Note  testdata/generic.md
Preview
Title: Ship mdfu MVP
Path: testdata/portent-task.md
Type: Task
Tags: tolaria, launch, Project Atlas
Status: draft
---
Finish the parser track so the search track can rank fixtures.
4/5 • archived:hidden • tab:multi • enter:select
```

See also: [Query Syntax](/guide/query-syntax/) for what to type, and [CLI](/reference/cli/) for `--filter` non-interactive mode.
