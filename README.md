# mdfu

Fuzzy finder for Markdown notes — body text plus normalized frontmatter (OKF v0.1/v0.2 and Portent/Tolaria) from one fast static binary, interactively (TUI) or pipeably (`mdfu <query>`, fzf-compatible).

Requires Go 1.25+.

## Install

```sh
# Homebrew (prebuilt bottle, binary lands on PATH via `binaries: [mdfu]`)
brew tap elicore/mdfu
brew install --cask elicore/mdfu/mdfu

# Go (installs into $(go env GOPATH)/bin — ensure it is on PATH)
go install github.com/elicore/mdfu/cmd/mdfu@latest

# or from source
git clone https://github.com/elicore/mdfu && cd mdfu && go build -o mdfu ./cmd/mdfu
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

## Develop

```sh
go build ./... && go test ./...
```

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
make act-ci            # gofmt, vet, race tests, coverage, cross-build matrix
make act-module-guard  # single-module layout guard
make act-docs          # Starlight build + internal link check
make act-release-check # GoReleaser snapshot
make act-all           # all of the above
```

Each run executes from a clean temporary context (untracked worktrees and build
output excluded) while still overlaying your uncommitted changes, so local edits
are tested as CI would see them.

These workflows cannot run locally — they need GitHub-hosted backends or a
macOS runner: `codeql.yml`, `pages.yml` (deploy job), `release-please.yml`,
`brew.yml`, and `release.yml` (requires a real tag). See the docs site's
[Local CI](/mdfu/project/local-ci/) page for the full rationale.

## Docs

Documentation is an Astro Starlight site under `website/`.

- Live: https://elicore.github.io/mdfu/
- Source: `website/src/content/docs/` (preview: `cd website && npm ci && npm run dev`)
- Roadmap: `PLAN.md`
