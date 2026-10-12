---
title: Architecture
description: Pipeline, the Rust crate map, the widget/content boundary, constraints, and performance budgets.
---

Pipeline for both modes (filter and TUI):

```
scan → parse → query → search → output/tui
```

`mdfu` is a Rust program. The Go implementation it replaced was removed after the
parity gate; its source survives only in git history at a frozen baseline SHA,
recorded in `PARITY.md`. This page describes the Rust implementation.

## Crates

The Cargo workspace root is the repository root (`Cargo.toml`), with three
members under `crates/`:

| Crate | Responsibility | Key modules |
|---|---|---|
| `mdfu-core` | Content and search logic, renderer-neutral | `scan`, `parse`, `model`, `query`, `search`, `output`, `theme`, `open`, `task` |
| `mdfu-tui` | ratatui interface built on `mdfu-core` | `view_model`, `widgets`, `preview`, `notes` (picker), `tasks` (board), `links` |
| `mdfu-cli` | The `mdfu` binary: flags, dispatch, exit codes | `main` (clap derive), `router` (`task`/`tasks` pre-parser), `pipeline` |

## Modules

| Module | Responsibility | Key API |
|---|---|---|
| `crates/mdfu-core/src/scan.rs` | `*.md` discovery, sorted, gitignore-aware | `walk_markdown(Options) -> Result<Vec<String>, ignore::Error>` — see [Scan](/guide/scan/) |
| `crates/mdfu-core/src/parse.rs` | frontmatter split + YAML + normalizers → `Document` | `parse_file`, `parse_content`, `split_frontmatter` |
| `crates/mdfu-core/src/model.rs` | Unified `Document` contract + search blob | `Document`, `build_search_blob`, `flatten_scalars` — see [Frontmatter Model](/guide/frontmatter-model/) |
| `crates/mdfu-core/src/query.rs` | Tokenizer + `Query` + date-range parser | `parse(input) -> Result<Query, Error>` — see [Query Syntax](/guide/query-syntax/) |
| `crates/mdfu-core/src/search.rs` | Hard-filter + fuzzy `rank` | `matches_doc`, `rank`, `filter_archived` — see [Search & Ranking](/guide/search-ranking/) |
| `crates/mdfu-core/src/output.rs` | `paths`/`json`/`vimgrep` formatters + `snippet` | `format_paths`, `format_json`, `format_vimgrep` — see [CLI](/reference/cli/) |
| `crates/mdfu-tui/src/notes/` | ratatui picker (state, keys, view) | `app`, `view`, `editor` — see [TUI](/guide/tui/) |
| `crates/mdfu-tui/src/tasks/` | ratatui task board | `model`, `actions`, `edit` — see [Tasks TUI](/guide/tasks-tui/) |
| `crates/mdfu-cli/src/` | Flags, `task`/`tasks` routing, wiring | `main.rs` (clap), `router.rs`, `pipeline.rs` |

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
lets the content logic be tested and reused without a terminal.

Filters are injected, never reached for: the note picker takes a `FilterFn` and a
reload function, and the task model takes its own task filter, all supplied by
`mdfu-cli`. The TUI therefore never imports the search engine, and it re-parses an
edited file through the injected reload function instead of importing the
parser. There is no global filter, and an integration guard test
(`crates/mdfu-tui/tests/guards.rs`) fails the build if one is reintroduced.

### Parity contract

`PARITY.md` is the parity contract: it freezes the observable behavior of the
removed Go implementation at a baseline SHA, classifies every Go test file as
normative, relaxed, or dropped, and lists the exit-code and error-string
matrices the Rust implementation keeps. `make parity` runs the Go-vs-Rust
differential harness against that baseline.

## Constraints (locked)

- Single static Rust binary, no daemon.
- No persistent index (see [Backend Evaluation](/reference/backend-evaluation/) for the evaluated exception path).
- Core deps: `serde`/`serde_json`, `yaml_serde`, `ignore`, `regex`, `etcetera`.
  TUI deps: `ratatui` + `ratatui-textarea`/`tui-input`/`tui-markdown`. The CLI
  adds `clap`. The fuzzy matcher is vendored in `search.rs` as `pub mod fuzzy`
  (a port of `sahilm/fuzzy` v0.1.3), so there is no fuzzy crate dependency.

## Performance budgets

- 5–10k files: cold `scan+parse` <500ms, per-keystroke `query+rank` <50ms.
- Current risk: `search::score_doc` runs a fuzzy match per doc per bare word
  against the whole search blob — `O(docs × words)`. See
  [Search & Ranking](/guide/search-ranking/) and the [Roadmap](/project/roadmap/)
  tasks.