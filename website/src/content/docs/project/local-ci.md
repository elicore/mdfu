---
title: Local CI
description: Run the GitHub Actions locally with act — what it gives us, the state before, and the target state.
---

CI for this repo is the workflows under `.github/workflows/`. This page explains
the local runner that executes them on a developer machine, and why it exists.

## What it gives us

The local runner (`scripts/act-local.sh`, wrapped by `make act-*`) runs the same
workflow YAML that GitHub Actions runs, using [`act`](https://github.com/nektos/act):

- **Reproduce failures locally.** A failing workflow can be rerun and debugged on
  the spot instead of waiting on a GitHub run.
- **Validate before pushing.** Workflows are exercised against *uncommitted*
  changes, so code, docs, and CI edits are checked before they reach a PR.
- **Test workflows that only fail on a fresh checkout.** `act` runs from a clean
  temporary context (git clone + overlay of the working tree), excluding
  untracked scratch dirs such as `.worktrees/` and build output — the same
  conditions a clean CI checkout sees.
- **Catch breakage in the inner loop** rather than after it lands on `main`.

## State before

CI only ran on GitHub, triggered by a push or a pull request. There was no local
way to run or debug a workflow.

Concretely, most workflows only fail against a *fresh* dependency install, so a
break can pass in a PR (which happens on a clean checkout) and still break
`main` for everyone else. That is exactly what happened to the docs build: after
the site moved to a newer Astro, `npm ci` + `npm run build` failed on the
Sätteri markdown processor, but the issue went unnoticed until the local runner
reproduced it. There was no earlier point at which a developer could have seen it.

## Target state

- Every workflow that can run on Linux is executable **before** push and stays
  green on `main`.
- Changes touching `cmd/`, `internal/`, `website/`, `docs/`, or
  `.github/workflows/` are validated locally first.
- Workflow problems are found in the developer's inner loop, not after merge.

## Usage

Docker is required. `act` is auto-installed on first use (Homebrew, then
`go install`).

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
are tested as CI would see them. The runner supplies `GITHUB_TOKEN` from
`gh auth token` when available.

## Locally unsupported

These workflows need GitHub-hosted backends or a macOS runner and cannot run
locally:

| Workflow | Why |
| --- | --- |
| `codeql.yml` | CodeQL analysis needs the GitHub security backend. |
| `pages.yml` | The deploy job needs the Pages environment and OIDC token. |
| `release-please.yml` | Drives GitHub releases/PRs via the GitHub API. |
| `brew.yml` | Runs on `macos-latest` and needs real release assets. |
| `release.yml` | Triggered by a real version tag and publishes a release. |

`release-check.yml` covers the GoReleaser build path locally; the publish steps
are the only part that requires GitHub.
