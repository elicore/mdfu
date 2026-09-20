# mdfu local dev tasks.
#
# `act-*` targets run the real GitHub Actions YAML locally via nektos/act
# (Docker required); see scripts/act-local.sh. Unsupported locally:
# codeql, pages/deploy, release-please, brew, release.

ACT_LOCAL := ./scripts/act-local.sh

.PHONY: help act-setup act-list act-ci act-module-guard act-docs act-release-check act-all

help:
	@$(ACT_LOCAL) --help

## Install nektos/act if it is missing (auto-detected on first run).
act-setup:
	@$(ACT_LOCAL) list >/dev/null && echo "act ready"

## List workflows/jobs act can see and run locally.
act-list:
	@$(ACT_LOCAL) list

## Run CI: gofmt, vet, race tests, coverage, cross-build matrix.
act-ci:
	@$(ACT_LOCAL) ci

## Run the single-module layout guard.
act-module-guard:
	@$(ACT_LOCAL) module-guard

## Build the Starlight docs site and check internal links.
act-docs:
	@$(ACT_LOCAL) docs

## Run the GoReleaser snapshot build.
act-release-check:
	@$(ACT_LOCAL) release-check

## Run every locally-runnable workflow.
act-all:
	@$(ACT_LOCAL) all
