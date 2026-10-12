---
title: Roadmap
description: Active roadmap — indexless perf (T1–T3), gated spikes (T4–T6), v2 language (T7–T8).
---

Active plan. Completed Tracks A–H / M1–M4 live in [History](/project/history/).

## Constraints (locked)

- Interactive TUI (default) + positional-query non-interactive filter (fzf-compatible).
- Single-box syntax: bare words fuzzy-match; `key:value` hard-filter; AND-combined.
- Unified normalized model, not strict per-format keys.
- No persistent index, no daemon — one statically linked binary.
- Budgets: 5–10k files, cold `scan+parse` \<500ms, per-keystroke `query+rank` \<50ms.

## Now (P0 — indexless perf, must hold budgets)

- [ ] **T1: Benchmark + profile current path.** 5k synthetic-file corpus; measure cold `scan+parse` and per-keystroke `query+rank`; CPU profile `search::score_doc` (`fuzzy::find` per doc per word vs the whole search blob). Acceptance: numbers + p50/p95 in PR; bottleneck confirmed or refuted.
- [ ] **T2: Indexless rank optimization.** Cache lowered search blob/tokens at parse time; avoid per-doc filter-map rebuilds; early-exit bare-word AND; reuse query parse per keystroke. Acceptance: T1 corpus meets \<50ms/keystroke with no behavior change (`cargo test --workspace` green).
- [ ] **T3: Facet pre-index (roaring bitmaps).** Pre-build `value → bitmap` for `tag/type/status` + hot custom keys; `matches_doc` becomes a bitmap `And/AndNot` before fuzzy scoring. Acceptance: filter-only queries scale with result size, not corpus size; generic `key:value` still falls back to `lookup_field`.

## Next (P1 — only pay if T1/T2 miss)

- [ ] **T4: Fuzzy-core bake-off.** Compare the vendored matcher (a port of `sahilm/fuzzy` v0.1.3, in `search::fuzzy`) vs `lithammer/fuzzysearch` vs `sajari/fuzzy` (SymDelete) on recall + latency over real titles/bodies. Acceptance: matrix + keep/swap decision; no swap without TUI blind-test win.
- [ ] **T5: Persistent-index spike (gated).** Prototype behind a flag: (a) `bleve` `NewMemOnly` dynamic mapping, (b) `bluge`, (c) SQLite FTS5 (`modernc` vs `cgo`). Compare binary size, cold-start, query latency, fuzzy+facet parity per [Backend Evaluation](/reference/backend-evaluation/). Acceptance: decision record + throwaway branches only; no new default dep.
- [x] **T6: Scan hardening.** Landed in Rust: `scan::Options::respect_gitignore` is effective and defaults to `true`, honoring `.gitignore`/`.ignore` (parent directories included); `--no-ignore` opts out. The Go baseline accepted the option and ignored it, so this is a recorded divergence. See [Scan](/guide/scan/).

## Later (v2 — language + display)

- [ ] **T7: Query v2.** `|` alternation within a token group, negation beyond tags, per-field boosting. Acceptance: [Query Syntax](/guide/query-syntax/) updated + table tests.
- [ ] **T8: Facet counts + highlighting.** Expose facet counts to TUI/`--format json` and match-fragment snippets. Acceptance: works indexless; aligns with T5 API if an index lands.

### Delivered

- [x] **T9: mdtask-compatible task CLI + TUI.** `mdfu task` (`list`, `view`/`show`, `open`, `move`, `set`, `ids`, `archive`, `validate`, `install-skills`) plus `mdfu tasks` for an interactive TUI, matching the external `mdtask` format (checkbox headers, `#tag`/`!priority`/`@key:value` metadata, `@blocked_by`, `.mdtaskrc`/`.mdfurc` config) and command surface; see [Tasks](/guide/tasks/) and [Tasks TUI](/guide/tasks-tui/). Acceptance: engine, CLI, and TUI tests green (`cargo test --workspace`); `mdfu tasks` falls back to `mdfu task list` when stdout is not a TTY.

## Working agreements

- `mdfu-core::model::Document` is the shared contract — changes need cross-crate sign-off.
- Each task: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --workspace` green before merge.
