# PROJECT KNOWLEDGE BASE

**Generated:** 2026-10-03
**Updated:** 2026-10-12 (Rust workspace moved from `rust/` to the repository root; Go prose pruned)
**Branch:** main

## OVERVIEW

`mdfu` — fuzzy finder for Markdown notes (body + normalized frontmatter, OKF v0.1/v0.2 + Portent/Tolaria) plus an mdtask-compatible task CLI/TUI. Single static Rust binary (Cargo workspace at the repository root, binary `mdfu`), ratatui TUI, no index/daemon. The Go implementation was removed after the parity gate (todo 37); its frozen baseline SHA is recorded in `PARITY.md`, and `make parity` still builds that baseline from git history for the differential harness.

## STRUCTURE

```
mdfu/
├── crates/          # the implementation: mdfu-{core,tui,cli}
├── Cargo.toml       # workspace root (+ Cargo.lock, rust-toolchain.toml, rustfmt.toml)
├── PARITY.md        # NORMATIVE parity contract: frozen Go baseline + exit-code/error matrices
├── testdata/        # fixture vault (broken YAML, OKF, Portent samples) — shared by Rust tests + parity
├── website/         # Astro Starlight docs — SEPARATE npm package, own .gitignore
├── scripts/         # parity.sh (Go-vs-Rust differential), act-local.sh (local CI runner)
├── Casks/           # Homebrew cask template (goreleaser-managed)
├── reports/         # perf/regression investigation notes
├── .github/workflows/ # CI; run locally via `make act-*`
└── PLAN.md          # roadmap; tasks T1–T8 predate the Rust implementation, T6 landed in Rust (todo 38)
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| CLI flags/modes/exit codes | `crates/mdfu-cli/src/main.rs` | clap derive; `run()` returns exit code; 0/1/2 semantics |
| `mdfu task` subcommand routing | `crates/mdfu-cli/src/router.rs` | router runs BEFORE flag parsing; unknown token after `task` falls through to fuzzy search (D2) |
| Task parsing/commands/IDs | `crates/mdfu-core/src/task/` | `grammar`, `body`, `id`, `blocker`, `config`, `glob`, `discover`, `render`, `edit`, `scope`, `cli` (nine subcommands); behavior contract in `PARITY.md` |
| Note picker / TUI | `crates/mdfu-tui/src/notes/` | `app.rs` state/keys, `view.rs` render, `editor.rs` `$EDITOR` reload; list ↔ search modes (`/` focus, `esc` back keeps query, second `esc` clears then aborts) |
| Task TUI | `crates/mdfu-tui/src/tasks/` | separate model (`model.rs`), actions (`actions.rs`), editors (`edit.rs`); filters injected, never global |
| Frontmatter→Document | `crates/mdfu-core/src/parse.rs`, `model.rs` | `model::Document` = shared contract |
| Query syntax | `crates/mdfu-core/src/query.rs` | bare words fuzzy, `key:value` AND-hard-filter |
| Ranking/scoring | `crates/mdfu-core/src/search.rs` | title-boost constants at top of file |
| File walking | `crates/mdfu-core/src/scan.rs` | gitignore-respecting by default; `--no-ignore` opts out (intended divergence, task 38) |
| Output formats | `crates/mdfu-core/src/output.rs` | paths/json/vimgrep; `Result` decoupled from scan/search |
| Theme/styles | `crates/mdfu-core/src/theme.rs` | XDG YAML merged over builtin defaults |
| Widget/content boundary | `crates/mdfu-tui/src/view_model.rs`, `widgets/` | plain-data view models; no ratatui in core |
| Parity harness | `scripts/parity.sh` + `crates/mdfu-cli/tests/parity.rs` | `make parity`; `--mutate` proves the comparator detects a tamper |
| Docs site | `website/src/content/docs/` | guide/ reference/ project/; sidebar in `astro.config.mjs` |

## CODE MAP

| Crate | Key modules | Role |
|-------|-------------|------|
| `mdfu-core` | `scan`, `parse`, `model`, `query`, `search`, `output`, `theme`, `open`, `task` | Content + search logic; must not depend on ratatui or TUI crates |
| `mdfu-tui` | `view_model`, `widgets`, `preview`, `links`, `notes`, `tasks` | ratatui note picker + task TUI over core |
| `mdfu-cli` | `main` (clap), `router`, `pipeline` | the `mdfu` binary: flags, mode dispatch, task/tasks router |

Pipeline: `scan` → `parse` → `model` → `query`+`search` → `output`; the TUI re-ranks live via an injected `FilterFn`.

`PARITY.md` is the parity contract (frozen Go baseline SHA); non-TTY output
is byte-compatible, TTY glyphs/SGR are relaxed (Go lipgloss vs Rust ratatui).
Release source is the Rust workspace: `.goreleaser.yaml` uses `builder: rust`,
and `release.yml`/`release-check.yml`/`brew.yml`/`release-please.yml` install
rustup 1.98.1 + zig 0.15.2.

## CONVENTIONS

- **Rust style**: `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` (workspace) gate CI. Toolchain pinned to 1.98.1 by `rust-toolchain.toml`.
- **Tests**: in-crate unit tests plus integration targets (`mdfu-cli/tests/{cli,parity}.rs`, `mdfu-tui/tests/guards.rs`); expected strings are literal and byte-compared against the frozen baseline where the parity contract says so. Rust tests read `testdata/` via `CARGO_MANIFEST_DIR` paths.
- **Exit codes**: 0 success, 1 runtime/IO/config error, 2 usage error — matrices in `PARITY.md` (carried from the Go SPEC).
- **Flag handling**: clap handles interspersed flags natively; do not reintroduce the Go `ReorderInterspersed` shim (dropped in the port).
- **Commits**: conventional (`feat(tui):`, `docs:`, `ci:`) — release-please drives CHANGELOG from these. `release-type: go` is kept only because release-please's Rust strategy rejects virtual workspaces; `extra-files` pins the workspace version in the root `Cargo.toml`.
- **Task subcommands**: handlers live in `mdfu-core/src/task/cli.rs`; the `mdfu-cli` router decides interception order. Don't move routing into the core engine.
- **Docs**: user-facing docs live ONLY in `website/src/content/docs/` (docs/ is a redirect stub). Update sidebar in `astro.config.mjs` when adding a page.

## ANTI-PATTERNS (THIS PROJECT)

- **No second Cargo workspace / stray crates** — exactly the root `Cargo.toml` + the 3 member manifests under `crates/` + one `Cargo.lock`; enforced by the `single-workspace` job in `module-guard.yml`. No submodules. `website/` must never gain a `Cargo.toml`.
- **No persistent index or daemon** — locked constraint (PLAN.md). One statically linked binary.
- **No global filter in the task TUI** — `mdfu-tui/tests/guards.rs` scans `src/tasks/` for `notes::`/`SetFilter`/`global_filter` vocabulary and fails the build. Inject via the task filter type.
- **Never touch the note picker's filter global from tasks code** (same guard).
- **No `mdtask`/`mdtask-add`/`mdtask-do` strings in embedded skills** — `guards.rs` forbids foreign skill names in `mdfu-core/assets/skills/`.
- **Don't parse task-looking lines inside body text** — body is opaque per the frozen task contract.
- **Don't change `model::Document` casually** — it is the shared contract across scan/parse/query/search/output/tui.
- **Checkbox `X` (uppercase) is not a task** — must be ignored, not normalized (broad TUI checkbox mode is separate).

## UNIQUE STYLES

- `PARITY.md` is normative+frozen for ported behavior: non-TTY output is byte-identical; divergences are recorded explicitly and guarded by `parity.rs` tests. Update the doc and its guard phrases together.
- Parsing never fails hard (`parse`): malformed frontmatter → `ParseError`, body stays searchable.
- `output::Result` is intentionally decoupled from scan/query/search — callers map before formatting.
- Big files exist and are accepted: `task/cli.rs` (~82k), `parse.rs` (~50k), `links.rs` (~51k), `view_model.rs` (~47k), `search.rs` (~47k).
- Local CI is first-class: `make act-rust` runs the real GitHub Actions YAML via nektos/act (Docker). Workflows that can't run locally: codeql, pages deploy, release-please, brew, release, release-check (Rust snapshot needs rustup+zig+cargo-zigbuild in the container).

## COMMANDS

```bash
cargo test --workspace                  # dev loop (must stay green)
cargo fmt --check && cargo clippy --all-targets -- -D warnings  # CI gates
cargo build --release                   # → target/release/mdfu
make parity                              # Go-vs-Rust differential harness (builds the frozen Go baseline from git history; needs a Go toolchain)
make act-rust                            # Rust workflow locally (act)
make act-module-guard                    # Rust workspace layout guard locally (act)
make act-docs                            # Starlight build + internal link check
make act-all                             # every locally-runnable workflow
cd website && npm ci && npm run dev      # docs preview (Node 22)
```

## NOTES

- Branch `main`; `.omo/` holds plan/task evidence (gitignored).
- Release vs parity: releases are built from the workspace root (`.goreleaser.yaml`, `dir: .`); `make parity` diffs the frozen-SHA Go build against the Rust build, and the Go tree itself no longer exists in the working tree (baseline lives in git history, recorded in `PARITY.md`).
- TUI seams are injected, never imported: the note picker takes a filter function and a reload function (`mdfu-cli` wires them to `parse`); `mdfu-tui` imports `mdfu-core` only.
- `--limit`: filter mode defaults 20, TUI 50; `0`/negative = unlimited.
- `mdfu task` intentionally falls through to fuzzy search when the router doesn't match ("task" is also a Portent DocType).
- `website/dist` and `node_modules` gitignored at website level; root `/dist/` and `/target/` too.
- `PARITY.md` keeps its Go `internal/...`/`cmd/mdfu` source references on purpose — they are pinned to the frozen baseline SHA. Everywhere else, the docs describe the Rust implementation only.
