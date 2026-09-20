# mdfu

Fuzzy finder for Markdown notes — body text plus normalized frontmatter (OKF v0.1/v0.2 and Portent/Tolaria) from one fast static binary, interactively (TUI) or pipeably (`--filter`, fzf-compatible).

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
mdfu --filter "retention"               # print matching paths (pipeable)
mdfu --filter "type:Task tag:launch"    # key:value hard filters, AND-combined
mdfu --filter "status:Draft" --format json
```

Point it somewhere else with `--root`:

```sh
mdfu --root ~/vault/Notes --filter "type:Metric"
```

Handy one-liners:

```sh
# Open the top match in your editor
$EDITOR "$(mdfu --filter "monthly active" --limit 1)"

# Feed a selection to another tool (paths, one per line)
mdfu --filter "tag:launch" | xargs wc -l
```

Try it on the bundled fixtures:

```sh
cd testdata
mdfu                                # interactive picker
mdfu --filter "kumquat zebra"       # → bad-yaml.md (broken YAML stays searchable)
```

## Develop

```sh
go build ./... && go test ./...
```

### Local CI (run the GitHub Actions locally)

The workflows under `.github/workflows/` can be run locally with
[`act`](https://github.com/nektos/act). Docker is required; `act` is
auto-installed on first use (Homebrew, then `go install`).

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
`brew.yml`, and `release.yml` (requires a real tag).

## Docs

Documentation is an Astro Starlight site under `website/`.

- Live: https://elicore.github.io/mdfu/
- Source: `website/src/content/docs/` (preview: `cd website && npm ci && npm run dev`)
- Roadmap: `PLAN.md`
