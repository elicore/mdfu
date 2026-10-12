# mdfu

Fuzzy finder for Markdown notes — body text plus normalized frontmatter (OKF v0.1/v0.2 and Portent/Tolaria) from one fast static binary, interactively (TUI) or pipeably (`mdfu <query>`, fzf-compatible).

Requires Rust 1.98.1+ (pinned in `rust-toolchain.toml`) to build from
source. Prebuilt binaries are available via Homebrew.

## Install

```sh
# Homebrew (prebuilt bottle, binary lands on PATH via `binaries: [mdfu]`)
brew tap elicore/mdfu
brew install --cask elicore/mdfu/mdfu

# or from source (binary lands in target/release/mdfu)
git clone https://github.com/elicore/mdfu && cd mdfu && cargo build --release
```

## Quick start

`mdfu` searches the current directory by default, so just `cd` into your notes and go:

```sh
cd ~/vault/Notes

mdfu                                    # interactive picker
mdfu "retention"                        # print matching paths (pipeable)
mdfu "type:Task tag:launch"             # key:value hard filters, AND-combined
mdfu "status:Draft" --format json
```

Point it somewhere else with `--root`:

```sh
mdfu --root ~/vault/Notes "type:Metric"
```

Handy one-liners:

```sh
# Open the top match in your editor
$EDITOR "$(mdfu "monthly active" --limit 1)"

# Feed a selection to another tool (paths, one per line)
mdfu "tag:launch" | xargs wc -l
```

Try it on the bundled fixtures:

```sh
cd testdata
mdfu                                # interactive picker
mdfu "kumquat zebra"                # → bad-yaml.md (broken YAML stays searchable)
```

## Tasks

`mdfu` also ships an mdtask-compatible checklist workflow for task files:

```sh
mdfu task list                      # list tasks in the current scope
mdfu task ids --prefix PRJ          # assign IDs to tasks without one
mdfu tasks                          # interactive task TUI
```

`mdfu task` is compatible with the external `mdtask` tool's format and commands.

### Any checkbox in the browser

`mdfu tasks` lists **every** markdown checkbox, not just mdtask tasks: any
`-`/`*`/`+` bullet, any indentation, `[ ]`/`[x]`/`[X]`, with or without an ID.
Toggle it, edit its title or body, open it, move it, and archive it. The
`mdfu task …` commands stay strict, so their output is byte-compatible.

See [Any Checkbox](https://elicore.github.io/mdfu/guide/any-checkbox/) for the
recognition rules and per-action behaviour.

## Develop

```sh
cargo build --release          # → target/release/mdfu
cargo test --workspace
cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

### Implementation

`mdfu` is implemented in the Cargo workspace at the repository root: three
crates under `crates/` — `mdfu-core` (scan/parse/query/rank/output), `mdfu-tui`
(ratatui interface), and `mdfu-cli` (the `mdfu` binary). The Go implementation
was removed after the parity gate; its frozen baseline stays in git history and
`PARITY.md` records the behavioral contract.

The port targets **behavioral parity**, not byte-identical task tables: non-TTY
output (including `task list --json`) is byte-compatible, while TTY box-drawing
glyphs and exact SGR styling are relaxed because they come from different UI
libraries (Go lipgloss vs Rust ratatui). `make parity` runs the Go-vs-Rust
differential harness against the frozen baseline (it builds the Go reference
from the recorded git SHA, so a Go toolchain is still required for that target
only).

### Local CI (run the GitHub Actions locally)

Run the workflows under `.github/workflows/` on your machine with
[`act`](https://github.com/nektos/act) — the same YAML CI runs, before you push.

**What it gives us**

- Reproduce CI failures locally (Docker required) instead of waiting on a
  GitHub run, and debug them with full local control.
- Exercise the *actual* workflow YAML against uncommitted changes, so a change
  is validated before it ever reaches a PR.
- Catch workflow/config breakage in the inner loop rather than on `main`.

**Before**

CI only ran on GitHub after a push or PR. There was no local way to run or debug
a workflow, and because most workflows only fail against a *fresh* checkout,
breakage could sit green in a PR and land on `main` — the docs build was in fact
broken on `main` by the Astro 7 upgrade until this runner surfaced it.

**Target state**

Every workflow that can run on Linux is executable before push and stays green
on `main`; changes touching code, docs, or CI are validated locally first.

```sh
make act-list          # list workflows/jobs act can run
make act-rust          # Rust workspace: fmt, clippy, tests, release build
make act-module-guard  # Rust workspace layout guard
make act-docs          # Starlight build + internal link check
make parity            # Go-vs-Rust differential parity harness
make act-all           # all of the above
```

Each run executes from a clean temporary context (untracked worktrees and build
output excluded) while still overlaying your uncommitted changes, so local edits
are tested as CI would see them.

These workflows cannot run locally — they need GitHub-hosted backends or a
macOS runner: `codeql.yml`, `pages.yml` (deploy job), `release-please.yml`,
`brew.yml`, `release.yml` (requires a real tag), and `release-check.yml` (Rust
snapshot: needs `rustup` + `zig` + `cargo-zigbuild` in the act container). See
the docs site's [Local CI](/mdfu/project/local-ci/) page for the full rationale.

## Docs

Documentation is an Astro Starlight site under `website/`.

- Live: https://elicore.github.io/mdfu/
- Source: `website/src/content/docs/` (preview: `cd website && npm ci && npm run dev`)
- Roadmap: `PLAN.md`
