---
title: Architecture
description: Pipeline, package responsibilities, constraints, and performance budgets.
---

Pipeline for both modes (filter and TUI):

```
scan → parse → query → search → output/tui
```

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
| `cmd/mdfu` | Flags, `runFilter` / `runTUI` wiring | `main.go`, `filter.go` |

`internal/tui` never imports `internal/search`/`internal/query`; integration injects `tui.SetFilter(buildFilterFunc())` in `cmd/mdfu`.

## Constraints (locked)

- Single static Go binary, no daemon, no `cgo` by default.
- No persistent index (see [Backend Evaluation](/reference/backend-evaluation/) for the evaluated exception path).
- Deps: `gopkg.in/yaml.v3`, `charmbracelet/bubbletea+bubbles+lipgloss`, `sahilm/fuzzy`, stdlib rest.

## Performance budgets

- 5–10k files: cold `scan+parse` <500ms, per-keystroke `query+rank` <50ms.
- Current risk: `search.scoreDoc` runs `fuzzy.Find` per doc per bare word against the whole `SearchBlob` — `O(docs × words)`. See [Search & Ranking](/guide/search-ranking/) and the [Roadmap](/project/roadmap/) tasks.
