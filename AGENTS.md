# PROJECT KNOWLEDGE BASE

**Generated:** 2026-10-03
**Commit:** 07f4527
**Branch:** main

## OVERVIEW

`mdfu` — fuzzy finder for Markdown notes (body + normalized frontmatter, OKF v0.1/v0.2 + Portent/Tolaria) plus an mdtask-compatible task CLI/TUI. Single static Go 1.25 binary, Bubble Tea TUI, no index/daemon/cgo. Module: `github.com/elicore/mdfu`. A behavioral Rust port lives in `rust/` (Cargo workspace, binary `mdfu`); since todo 36 the release pipeline builds Rust while the Go tree stays the parity reference until the todo-37 removal.

## STRUCTURE

```
mdfu/
├── cmd/mdfu/        # main: flag parsing, mode dispatch, mdfu task/tasks router
├── internal/        # all library code (11 packages, flat — see CODE MAP)
│   ├── task/        # mdtask-compat engine: normative SPEC.md lives here
│   └── tui/         # note picker + task TUI (Bubble Tea)
├── tests/           # cross-package functional + regression tests (package tests)
├── testdata/        # fixture vault (broken YAML, OKF, Portent samples) — shared by both trees
├── rust/            # Cargo workspace: crates/{mdfu-core,mdfu-tui,mdfu-cli}; PARITY.md contract
├── website/         # Astro Starlight docs — SEPARATE npm package, own .gitignore
├── scripts/         # act-local.sh (local CI runner)
├── Casks/           # Homebrew cask template (goreleaser-managed)
├── reports/         # perf/regression investigation notes
├── .github/workflows/ # CI; run locally via `make act-*`
├── PLAN.md          # active roadmap (P0/P1 tasks)
└── internal/task/SPEC.md  # NORMATIVE frozen task-format spec
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| CLI flags/modes/exit codes | `cmd/mdfu/main.go` | `run()` returns exit code; 0/1/2 semantics |
| `mdfu task` subcommand routing | `cmd/mdfu/task.go` | `maybeRouteTask` runs BEFORE flag parsing |
| Task parsing/commands/IDs | `internal/task/` | behavior changes → update `SPEC.md` + table tests |
| Note picker / TUI | `internal/tui/tui.go` (1.4k LOC) | Two modes: list (no search bar) ↔ search (`/` to focus; `esc` back keeps query, second `esc` clears then aborts). List keys allowlisted; `e` edits focused doc via `$EDITOR` then re-parses. Entry: `Run`, `RunWithFilter`, `SetFilter` |
| Rust port | `rust/crates/` | `mdfu-core` (no ratatui) / `mdfu-tui` / `mdfu-cli`; parity contract `rust/PARITY.md`; release source since todo 36 |
| Task TUI | `internal/tui/tasks_*.go` | separate model; filters injected, not global |
| Frontmatter→Document | `internal/parse/`, `internal/model/` | `model.Document` = shared contract |
| Query syntax | `internal/query/` | bare words fuzzy, `key:value` AND-hard-filter |
| Ranking/scoring | `internal/search/` | title-boost constants at top of file |
| File walking | `internal/scan/` | `RespectGitignore` option exists but is NOT implemented (PLAN.md T6) |
| Output formats | `internal/output/` | paths/json/vimgrep; `Result` decoupled from scan/search |
| Theme/styles | `internal/theme/` | XDG YAML merged over builtin defaults |
| Docs site | `website/src/content/docs/` | guide/ reference/ project/; sidebar in `astro.config.mjs` |

## CODE MAP

LSP (gopls) and codegraph unavailable — centrality measured by grep import counts.

| Package | Exported funcs/types | Importers | Role |
|---------|---------------------|-----------|------|
| `internal/model` | 2/4 | 12 | Shared `Document` contract — cross-package sign-off for changes |
| `internal/theme` | 12/1 | 11 | Theme resolution; consumed by tui + cmd |
| `internal/task` | 88/14 | 9 | Task engine; registry via `init()` per subcommand |
| `internal/tui` | 86/14 | 3 | Both TUIs; entry: `Run`, `RunTasks` |
| `internal/query` | 14/2 | 3 | Query parser |
| `internal/search` | 20/0 | 2 | Ranking |
| `internal/parse` | 10/0 | 2 | Frontmatter split + normalize |
| `internal/output` | 11/1 | 2 | Formatting |
| `internal/scan` | 2/1 | 1 | Directory walk |
| `internal/open` | 2/0 | 1 | URL launch (web schemes only) |

Pipeline: `scan` → `parse` → `model` → `query`+`search` → `output`; TUI re-ranks live via `FilterFunc`.

## RUST MAP

| Crate | Role |
|-------|------|
| `mdfu-core` | scan/parse/model/query/rank/output — must not depend on ratatui or TUI crates |
| `mdfu-tui` | ratatui note picker + task TUI |
| `mdfu-cli` | `mdfu` binary: clap flags, mode dispatch, task/tasks router |

`rust/PARITY.md` is the parity contract (frozen Go baseline SHA); non-TTY output
is byte-compatible, TTY glyphs/SGR are relaxed (Go lipgloss vs Rust ratatui).
Release source is the Rust workspace: `.goreleaser.yaml` uses `builder: rust`,
and `release.yml`/`release-check.yml`/`brew.yml`/`release-please.yml` install
rustup 1.98.1 + zig 0.15.2. Go remains in `ci.yml`/`codeql.yml` until todo 37.

## CONVENTIONS

- **Go style**: `gofmt` clean, `go vet`, `go mod tidy -diff` all gate CI. No lint config file — stdlib style.
- **Tests**: table tests with literal expected strings (byte-identical to SPEC.md). In-package (`package task`, not `task_test`) except cross-package `tests/`. `Env` struct injects Stdout/Stderr/Cwd/Now — command layer never touches the real host.
- **Exit codes**: SPEC.md §I — 0 success, 1 runtime/IO/config error, 2 usage error.
- **Flag handling**: Go `flag` stops at first positional → `task.ReorderInterspersed` reorders (GNU-style interspersed flags). Don't "fix" by parsing manually.
- **Commits**: conventional (`feat(tui):`, `docs:`, `ci:`) — release-please drives CHANGELOG from these.
- **Subcommand registration**: each `internal/task/*.go` calls `register()` from `init()`; never edit `commands.go` dispatch.
- **Docs**: user-facing docs live ONLY in `website/src/content/docs/` (docs/ is a redirect stub). Update sidebar in `astro.config.mjs` when adding a page.

## ANTI-PATTERNS (THIS PROJECT)

- **No second Go module / go.work / submodules** — enforced by `module-guard.yml` (exactly one `go.mod` outside `website/`).
- **No second Cargo workspace / stray crates** — exactly `rust/Cargo.toml` + the 3 member manifests + one `rust/Cargo.lock`; enforced by the `single-workspace` job in `module-guard.yml`.
- **No persistent index, daemon, or cgo** — locked constraint (PLAN.md).
- **No global filter in task TUI** — `tasks_run_test.go` greps tasks sources for `SetFilter`/`globalFilter` and fails the build. Inject via `TaskFilterFunc`.
- **Never touch the note picker's `SetFilter` global from tasks code** (same guard).
- **No `mdtask`/`mdtask-add`/`mdtask-do` strings in embedded skills** — `install_skills_test.go` forbids foreign skill names in `internal/task/skills/`.
- **Don't parse task-looking lines inside body text** — body is opaque per SPEC.md §A.
- **Don't change `model.Document` casually** — working agreement: cross-package sign-off (PLAN.md).
- **Checkbox `X` (uppercase) is not a task** — must be ignored, not normalized.

## UNIQUE STYLES

- `internal/task/SPEC.md` is normative+frozen: output is byte-identical to the spec; tests assert with string literals. Divergences only via §K.
- Parsing never fails hard (`parse`): malformed frontmatter → `Document.ParseError`, body stays searchable.
- `output.Result` intentionally decoupled from scan/query/search — callers map before formatting.
- Big files exist and are accepted: `tui/tui.go` (1377), `tui/tasks_model.go` (763), `search/search.go` (510).
- Local CI is first-class: `make act-ci` runs the real GitHub Actions YAML via nektos/act (Docker). Workflows that can't run locally: codeql, pages deploy, release-please, brew, release, release-check (Rust snapshot needs rustup+zig+cargo-zigbuild in the container).

## COMMANDS

```bash
go build ./... && go test ./...          # dev loop (must stay green)
test -z "$(gofmt -l .)" && go vet ./...  # CI gates
cd rust && cargo test --workspace        # Rust dev loop (parity target)
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings  # Rust CI gates
cd rust && cargo build --release         # → rust/target/release/mdfu
make parity                              # Go-vs-Rust differential harness (frozen baseline)
make act-ci                              # full local CI (gofmt/vet/race/coverage/cross-build)
make act-rust                            # Rust workflow locally (act)
make act-docs                            # Starlight build + internal link check
make act-all                             # every locally-runnable workflow
cd website && npm ci && npm run dev      # docs preview (Node 22)
```

## NOTES

- Branch `main`; `.omo/` holds plan/task evidence (gitignored).
- Release vs parity: releases are built from `rust/` (`.goreleaser.yaml`), but the Go tree is still the behavioral reference — `make parity` diffs the frozen-SHA Go build against the Rust build, and `ci.yml`/`codeql.yml` keep Go green until todo 37 removes the tree.
- Note picker seams are injected, never imported: `FilterFunc` (filtering) and `ReloadFunc func(path string) (*model.Document, error)` (re-parse after `$EDITOR` exits). `internal/tui` imports only `model` + stdlib + bubbletea; `cmd/mdfu` wires `Config.Reload` to `reloadDocument` in `pipeline.go`.
- `scan.Options.RespectGitignore` is a no-op (PLAN.md T6) — flag exists, behavior doesn't.
- `--limit`: filter mode defaults 20, TUI 50; `0`/negative = unlimited.
- `mdfu task` intentionally falls through to fuzzy search when router doesn't match ("task" is also a Portent DocType).
- `website/dist` and `node_modules` gitignored at website level; root `/dist/` too.
