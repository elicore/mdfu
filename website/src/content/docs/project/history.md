---
title: History
description: Completed tracks and milestones — scaffold through docs.
---

Completed work, kept for reference. Do not add new tasks here — see the [Roadmap](/project/roadmap/).

## Releases

- **0.4.0** (2026-10-12) — the Rust rewrite ships. The Go implementation was
  deleted after the parity gate; `mdfu` is now one static binary built from the
  Cargo workspace at the repository root. Non-TTY output stays
  byte-compatible with the frozen Go baseline, `--no-ignore` now takes effect,
  and TTY glyphs/SGR follow ratatui instead of lipgloss. See
  [Architecture](/reference/architecture/).
- **0.1.0 – 0.3.0** — the Go era. The Rust port landed after `v0.3.0`, so the
  whole rewrite is what 0.4.0 ships.

## Tracks (all merged)

- Track A (scaffold+scan): `d2c7a30` — `cmd` skeleton + `internal/scan` walker.
- Track B (parse+model): `df06910` — canonical `internal/model` + `internal/parse` + fixtures.
- Track C (query+search): `6bace15` — `internal/query` + `internal/search`.
- Track D (cli-output): `282802d` — `internal/output` formatters real; stub replaced by Track F.
- Track E (tui): `63afefa` — BubbleTea picker standalone; wiring in Track F.
- Track F (wire-up): real `scan→parse→query→rank→output` + live TUI `FilterFunc`; `--archived` flag.
- Track G (tests): `tests/functional_test.go` (11) + `tests/regression_test.go` (10).
- Track H (docs): README + examples + headless screenshot.

Integration order was A+B → C → D+E → main, each green on `go build ./... && go test ./...`.

These tracks are the Go era. The Go implementation was ported to Rust, then
deleted once the parity gate passed; its source survives only in git history at
the frozen baseline SHA recorded in `PARITY.md`. The current implementation is
the Rust workspace at the repository root — see [Architecture](/reference/architecture/).

## Milestones (all done)

- M1: build green; scan finds `*.md`; parse fixtures produce expected docs.
- M2: query table tests pass; rank title-boost verified.
- M3: `--filter "tag:x"` prints ranked paths; `--format json` valid; exits `0/1/2` verified live.
- M4 (wired; perf benchmark moved to the [Roadmap](/project/roadmap/)): TUI opens with live filtering, Enter prints selection.
