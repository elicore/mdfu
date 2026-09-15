---
title: History
description: Completed tracks and milestones — scaffold through docs.
---

Completed work, kept for reference. Do not add new tasks here — see the [Roadmap](/project/roadmap/).

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

## Milestones (all done)

- M1: build green; scan finds `*.md`; parse fixtures produce expected docs.
- M2: query table tests pass; rank title-boost verified.
- M3: `--filter "tag:x"` prints ranked paths; `--format json` valid; exits `0/1/2` verified live.
- M4 (wired; perf benchmark moved to the [Roadmap](/project/roadmap/)): TUI opens with live filtering, Enter prints selection.
