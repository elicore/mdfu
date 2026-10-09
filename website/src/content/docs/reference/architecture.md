---
title: Architecture
description: Pipeline, Go packages, the Rust crate map, the widget/content boundary, constraints, and performance budgets.
---

Pipeline for both modes (filter and TUI):

```
scan → parse → query → search → output/tui
```

The Go tree under `cmd/` and `internal/` is the reference implementation; a
Rust port in the `rust/` Cargo workspace is developed alongside it and targets
[behavioral parity](/reference/cli/#rust-port) until cutover. This page covers
both.

## Packages

| Package | Responsibility | Key API |
|---|---|---|
| `internal/scan` | `*.md` discovery, sorted | `WalkMarkdown(Options) ([]string, error)` — see [Scan](/guide/scan/) |
| `internal/parse` | frontmatter split + YAML + normalizers → `Document` | `ParseFile`, `ParseContent`, `SplitFrontmatter` |
| `internal/model` | Unified `Document` contract + `SearchBlob` | `Document`, `BuildSearchBlob`, `FlattenScalars` — see [Frontmatter Model](/guide/frontmatter-model/) |
| `internal/query` | Tokenizer + `Query` AST + date-range parser | `Parse(input) (*Query, error)` — see [Query Syntax](/guide/query-syntax/) |
| `internal/search` | Hard-filter (`MatchesDoc`) + fuzzy `Rank` + `FilterArchived` | `Rank`, `MatchesDoc` — see [Search & Ranking](/guide/search-ranking/) |
| `internal/tui` | BubbleTea picker, `FilterFunc` hook | `Run`, `NewModel`, `SetFilter` — see [TUI](/guide/tui/) |
| `internal/output` | `paths/json/vimgrep` formatters + `Snippet` | `FormatPaths/JSON/Vimgrep` — see [CLI](/reference/cli/) |
| `cmd/mdfu` | Flags, `runFilter` / `runTUI` wiring | `main.go`, `filter.go`, `pipeline.go` — `loadDocuments`, `buildFilterFunc`, `runQuery*`, `reloadDocument` |

`internal/tui` never imports `internal/search`/`internal/query`; integration injects `tui.SetFilter(buildFilterFunc())` in `cmd/mdfu`. The picker's edit-and-reload path is injected too: `Config.Reload` is wired to `pipeline.go`'s `reloadDocument` (a `parse.ParseFile` wrapper), so the TUI re-parses an edited file without importing `internal/parse`.

## Rust crates

The Rust port is a three-crate workspace under `rust/`, drawn along the same
content/UI seam as the Go packages:

| Crate | Responsibility | Key modules |
|---|---|---|
| `mdfu-core` | Content and search logic, renderer-neutral | `scan`, `parse`, `model`, `query`, `search`, `output`, `theme`, `open`, `task` |
| `mdfu-tui` | ratatui interface built on `mdfu-core` | `view_model`, `widgets`, `preview`, `notes` (picker), `tasks` (board), `links` |
| `mdfu-cli` | The `mdfu` binary: flags, dispatch, exit codes | `main` (clap derive), `router` (`task`/`tasks` pre-parser), `pipeline` |

### Widget/content boundary

`mdfu-tui` splits cleanly into a model layer and a render layer:

- `view_model` is the plain-data boundary: the view structs the widgets render
  from. It carries no ratatui types, so the models can be tested headlessly.
- `widgets` is the reusable ratatui layer built on `view_model` (list, preview,
  status bar, task pane).
- `notes` and `tasks` are the Elm-style models. They hold state, translate key
  and message events into `Cmd`s, and render through the widget layer.

No ratatui in core is the rule: `mdfu-core` has no ratatui (or other UI)
dependency, and `mdfu-tui` depends on `mdfu-core`, never the reverse. `cargo
tree -p mdfu-core` has no ratatui in it. Keeping core renderer-neutral is what
lets the content logic be ported, tested, and reused without a terminal.

Filters stay injected in Rust too, exactly as in Go: the note picker takes a
`FilterFn` and the task model takes its own `TaskFilterFn`, both supplied by
`mdfu-cli`. There is no global filter, and an integration guard test fails the
build if one is reintroduced.

`rust/PARITY.md` is the parity contract: it freezes the observable Go behavior
at a baseline SHA, classifies every Go test file as normative, relaxed, or
dropped, and lists the exit-code and error-string matrices the port must keep.
`make parity` runs the Go-vs-Rust differential harness against that baseline.

## Constraints (locked)

- Single static Go binary, no daemon, no `cgo` by default.
- No persistent index (see [Backend Evaluation](/reference/backend-evaluation/) for the evaluated exception path).
- Deps: `gopkg.in/yaml.v3`, `charmbracelet/bubbletea+bubbles+lipgloss`, `sahilm/fuzzy`, stdlib rest.

## Performance budgets

- 5–10k files: cold `scan+parse` <500ms, per-keystroke `query+rank` <50ms.
- Current risk: `search.scoreDoc` runs `fuzzy.Find` per doc per bare word against the whole `SearchBlob` — `O(docs × words)`. See [Search & Ranking](/guide/search-ranking/) and the [Roadmap](/project/roadmap/) tasks.
