# mdfu — plan (active)

## Goal

Single static Go binary `mdfu` for fuzzy-finding markdown by body free text, frontmatter values as free text, and attribute qualifiers. OKF v0.1/v0.2 + Portent/Tolaria via a unified normalized model.

Details moved out of this file into the docs site (`website/src/content/docs/`,
live at <https://elicore.github.io/mdfu/>):

- `guide/` — query syntax, frontmatter model, search & ranking, scan, TUI
- `reference/` — architecture, CLI, backend evaluation (bleve vs bluge vs SQLite)
- `project/` — roadmap (this file's tasks) and history (completed Tracks A–H, M1–M4)

## Constraints (locked)

- Interactive TUI (default) + `--filter` non-interactive (fzf-compatible).
- Single-box syntax: bare words fuzzy-match; `key:value` hard-filter; AND-combined.
- Unified normalized model, not strict per-format keys.
- No persistent index, no daemon, no `cgo` by default.
- Budgets: 5–10k files, cold `scan+parse` <500ms, per-keystroke `query+rank` <50ms.

## Roadmap

### Now (P0 — indexless perf, must hold budgets)

- [ ] **T1: Benchmark + profile current path.** 5k synthetic-file corpus; measure cold `scan+parse` and per-keystroke `query+rank`; CPU profile `search.scoreDoc` (`fuzzy.Find` per doc per word vs whole `SearchBlob`). Acceptance: numbers + p50/p95 in PR; bottleneck confirmed or refuted.
- [ ] **T2: Indexless rank optimization.** Cache lowered `SearchBlob`/tokens at parse time; avoid per-doc filter-map rebuilds; early-exit bare-word AND; reuse query parse per keystroke. Acceptance: T1 corpus meets <50ms/keystroke with no behavior change (`go test ./...` green).
- [ ] **T3: Facet pre-index (roaring bitmaps).** Pre-build `value → bitmap` for `tag/type/status` + hot custom keys; `MatchesDoc` becomes bitmap `And/AndNot` before fuzzy scoring. Acceptance: filter-only queries scale with result size, not corpus size; generic `key:value` still falls back to `lookupField`.

### Next (P1 — only pay if T1/T2 miss)

- [ ] **T4: Fuzzy-core bake-off.** Compare `sahilm/fuzzy` (current) vs `lithammer/fuzzysearch` vs `sajari/fuzzy` (SymDelete) on recall + latency over real titles/bodies. Acceptance: matrix + keep/swap decision; no swap without TUI blind-test win.
- [ ] **T5: Persistent-index spike (gated).** Prototype behind a flag: (a) `bleve` `NewMemOnly` dynamic mapping, (b) `bluge`, (c) SQLite FTS5 (`modernc` vs `cgo`). Compare binary size, cold-start, query latency, fuzzy+facet parity per the backend-evaluation page. Acceptance: decision record + throwaway branches only; no new default dep.
- [ ] **T6: Scan hardening.** Implement `RespectGitignore` (currently ignored, `internal/scan/scan.go:31`) + parallel file load. Acceptance: gitignored files excluded unless `--no-ignore`; 5k cold scan still <500ms.

### Later (v2 — language + display)

- [ ] **T7: Query v2.** `|` alternation within a token group, negation beyond tags, per-field boosting. Acceptance: query-syntax docs page updated + table tests.
- [ ] **T8: Facet counts + highlighting.** Expose facet counts to TUI/`--format json` and match-fragment snippets. Acceptance: works indexless; aligns with T5 API if an index lands.

## Working agreements

- `internal/model` is the shared contract — changes need cross-package sign-off.
- Each task: `go build ./... && go test ./...` green before merge.
