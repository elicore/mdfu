---
title: TUI
description: Interactive picker — live filtering, keybindings, preview pane.
---

Launch by omitting `--filter`:

```sh
mdfu [--root DIR] [--hidden] [--limit N] [--archived]
```

Type to narrow — bare words fuzzy, `key:value` hard-filters via the live `FilterFunc` (`scan → parse → query → rank` on every keystroke). Query terms are emphasized (yellow background) in both the result list and the preview pane; matches compose with the markdown syntax highlighting. `Enter` prints the selection (pipeable to editors/fzf flows).

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
| other | Edit query and refilter. |

Status bar: `matched/total • archived:hidden|shown • tab:multi • enter:select`.

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
