---
title: TUI
description: Interactive picker — list/search modes, keybindings, preview pane.
---

Launch with no query:

```sh
mdfu [--root DIR] [--hidden] [--limit N] [--archived] [--no-hyperlinks]
```

The picker opens in **list mode**: the search box is hidden and the first line is a result. Press `/` to reveal and focus the search box, then type to narrow — bare words fuzzy, `key:value` hard-filters via the live `FilterFunc` (`scan → parse → query → rank` on every keystroke). Query terms are emphasized (yellow background) in both the result list and the preview pane; matches compose with the markdown syntax highlighting. Editing the query re-anchors the cursor: if the selected document survives into the new results it stays selected, otherwise the cursor resets to the top. `Enter` prints the selection (pipeable to editors/fzf flows).

## Interaction modes

- **List mode (default).** Only the result list, preview, and status bar render; the first line is a result. Plain letters and editing keys are ignored — they never touch the hidden query. `/` reveals and focuses the search box.
- **Search mode (`/`).** Typing filters live. `esc` returns to the list and **retains the query**, so the narrowed list stays put. This differs from the [tasks browser](/guide/tasks-tui/), whose filter `esc` clears the filter.
- **`esc` semantics.** In list mode, `esc` with a non-empty query clears it and restores the full list; once the query is empty, `esc` aborts with no output (no `q` key — there are no vim motions either). `ctrl+c` aborts from either mode.
- **Editing (`e`).** In list mode, `e` opens the focused document in `$EDITOR` (the picker suspends while the editor runs). After the editor exits, the file is re-parsed and the preview refreshes, then the list re-filters. With `$EDITOR` unset or blank, the status bar shows `$EDITOR is not set` and the picker stays open.

## Keybindings (`internal/tui/tui.go:Model.Update`)

| Key | Action |
|---|---|
| `/` | List mode only: reveal and focus the search box (in search mode a literal `/` is typed into the query). |
| `e` | List mode only: open the focused document in `$EDITOR` (suspends the picker; see [Interaction modes](#interaction-modes)). |
| `up` / `ctrl+k` | Cursor up (wraps). |
| `down` / `ctrl+j` | Cursor down (wraps). |
| `enter` | Confirm: multi-selection if any, else cursor item. |
| `esc` | Search: back to list (filter kept). List: clear the filter, then quit when it is empty. |
| `ctrl+c` | Abort (no output). |
| `tab` | Toggle multi-select on cursor item (the checkbox column appears only once a selection exists). |
| `ctrl+a` | Toggle archived visibility (default hidden). |
| `ctrl+p` | Toggle preview pane (glamour-highlighted markdown preview of the first 30 body lines + filename header, title, and frontmatter rows; side-by-side when ≥100 cols). |
| `ctrl+f` | Toggle the frontmatter block in the preview (the labeled rows between the title and the body). The startup default comes from `show_frontmatter` in the [Configuration](/guide/configuration/) file. |
| `ctrl+o` | Open the previewed document's first web link (its `Resource`, else the first link in the body) in the default browser. |
| other | Search mode: edit the query and refilter. List mode: ignored. |

Status bar: `matched/total • archived:hidden|shown • fm:shown|hidden`, then mode hints — `/:search • e:edit • tab:multi • enter:select` in list mode, `esc:back • tab:multi • enter:select` in search mode — plus a transient status message when there is one (for example `$EDITOR is not set`). The `fm:` field reports the `ctrl+f` frontmatter toggle.

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

Headless `Model.View()` captures:

List mode — the search bar is hidden, so the first line is a result:

```text
> Ship mdfu MVP [Task]  testdata/portent-task.md
  Monthly Active Users [Metric]  testdata/okf-v02-metric.md
portent-task.md
Ship mdfu MVP
Path: testdata/portent-task.md
Type: Task
Status: draft
Tags: │ tolaria │ │ launch │ │ Project Atlas │
  Finish the parser track so the search track can rank fixtures.
2/2 • archived:hidden • fm:shown • /:search • e:edit • tab:multi • enter:select
```

Search mode (`/`) — the search box is shown and the query has narrowed the list:

```text
> monthly
> Monthly Active Users [Metric]  testdata/okf-v02-metric.md
okf-v02-metric.md
Monthly Active Users
Path: testdata/okf-v02-metric.md
Type: Metric
Status: active
  Active users in the last 30 days, reported per workspace.
1/2 • archived:hidden • fm:shown • esc:back • tab:multi • enter:select
```

List mode hides only the search box; the preview stays on (toggle it with `ctrl+p`). The preview stacks, top to bottom: the filename header (base name, own color), the document title (own color, no `Title:` label), the frontmatter rows as `key: value` with list values rendered as colored pills (here `Tags:`), and the rendered markdown body. JSON-object frontmatter values flatten inline under their parent key as `k1: v1, k2: v2`. The old `Preview` heading and `---` separator are gone; the full `Path:` row stays in the frontmatter block so the complete path remains visible and copyable.

See also: [Query Syntax](/guide/query-syntax/) for what to type, and [CLI](/reference/cli/) for non-interactive filter mode.
