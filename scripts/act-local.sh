#!/usr/bin/env bash
#
# Local GitHub Actions runner for mdfu, powered by nektos/act.
#
# Runs the workflow YAML from a clean temporary context so that untracked
# worktrees (.worktrees/) and build output can't pollute jobs that inspect the
# tree (module-guard) or change results. Uncommitted working-tree changes are
# still overlaid, so local edits are exercised.
#
#   ./scripts/act-local.sh list
#   ./scripts/act-local.sh ci
#   ./scripts/act-local.sh module-guard
#   ./scripts/act-local.sh docs
#   ./scripts/act-local.sh release-check
#   ./scripts/act-local.sh all
#
# Workflows that cannot run locally (GitHub-hosted backends / macOS runners):
# codeql.yml, pages.yml (deploy), release-please.yml, brew.yml, release.yml.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

RUNNABLE=(ci module-guard docs release-check)
declare -A WORKFLOW=(
  [ci]=".github/workflows/ci.yml"
  [module-guard]=".github/workflows/module-guard.yml"
  [docs]=".github/workflows/docs.yml"
  [release-check]=".github/workflows/release-check.yml"
)
# release-check only triggers on pull_request/workflow_dispatch; the rest push.
declare -A EVENT=(
  [ci]="push"
  [module-guard]="push"
  [docs]="push"
  [release-check]="pull_request"
)

log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!!\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<EOF
Usage: $(basename "$0") [command]

Commands:
  list           List all workflows/jobs act can see
  ci             Run CI (test + cross-build matrix)
  module-guard   Run single-module layout guard
  docs           Build the Starlight site + offline internal link check
  release-check  Run GoReleaser snapshot build
  all            Run every locally-runnable workflow (default)

Locally unsupported: codeql, pages/deploy, release-please, brew, release.
EOF
}

ensure_docker() {
  command -v docker >/dev/null 2>&1 || die "docker not found — install Docker Desktop/Engine first"
  docker info >/dev/null 2>&1 || die "docker daemon is not running"
}

install_act() {
  warn "act not found on PATH; attempting automatic install"
  if command -v brew >/dev/null 2>&1; then
    log "brew install act"
    if brew install act; then return 0; fi
  fi
  if command -v go >/dev/null 2>&1; then
    log "go install github.com/nektos/act@latest"
    GOBIN="${GOBIN:-$(go env GOPATH)/bin}" go install github.com/nektos/act@latest && return 0
  fi
  die "could not install act automatically — see https://github.com/nektos/act"
}

resolve_act() {
  if command -v act >/dev/null 2>&1; then
    ACT_BIN="$(command -v act)"
    return
  fi
  install_act
  if command -v act >/dev/null 2>&1; then
    ACT_BIN="$(command -v act)"
    return
  fi
  local gopath_bin
  gopath_bin="$(go env GOPATH 2>/dev/null)/bin"
  if [ -x "$gopath_bin/act" ]; then
    ACT_BIN="$gopath_bin/act"
  else
    die "act installed but not on PATH — add $gopath_bin to PATH"
  fi
}

ensure_rsync() {
  command -v rsync >/dev/null 2>&1 || die "rsync not found — required to build the clean context"
}

CTX=""
cleanup() {
  if [ -n "$CTX" ]; then rm -rf "$CTX"; fi
  return 0
}
trap cleanup EXIT

make_context() {
  ensure_rsync
  CTX="$(mktemp -d "${TMPDIR:-/tmp}/mdfu-act.XXXXXX")"
  log "clean context: $CTX"
  git -C "$REPO_ROOT" clone --quiet --no-hardlinks "$REPO_ROOT" "$CTX"
  rsync -a \
    --exclude='.git/' \
    --exclude='.worktrees/' \
    --exclude='website/node_modules/' \
    --exclude='website/dist/' \
    --exclude='dist/' \
    --exclude='coverage.out' \
    "$REPO_ROOT/" "$CTX/"
}

act_args() {
  # act shares the /opt/hostedtoolcache volume across concurrent jobs; multiple
  # setup-go downloads racing corrupt it, so serialize.
  ACT_ARGS=(--rm --concurrent-jobs 1)
  case "$(uname -m)" in
    x86_64 | amd64) ACT_ARGS+=(--container-architecture linux/amd64) ;;
    aarch64 | arm64) ACT_ARGS+=(--container-architecture linux/arm64) ;;
  esac
  local token="${GITHUB_TOKEN:-}"
  if [ -z "$token" ] && command -v gh >/dev/null 2>&1; then
    token="$(gh auth token 2>/dev/null || true)"
  fi
  token="${token:-0000000000000000000000000000000000000000}"
  ACT_ARGS+=(--secret "GITHUB_TOKEN=$token")
}

run_list() {
  resolve_act
  # act exits nonzero when job names repeat across workflows (true here:
  # "build"/"goreleaser"); that is informational, not a failure.
  ( cd "$REPO_ROOT" && "$ACT_BIN" --list ) ||
    warn "act --list returned nonzero (duplicate job names across workflows are expected)"
}

run_workflow() {
  local name="$1"
  local wf="${WORKFLOW[$name]}"
  local event="${EVENT[$name]}"
  log "running '$name' ($wf, event=$event)"
  if ( cd "$CTX" && "$ACT_BIN" "$event" --workflows "$wf" "${ACT_ARGS[@]}" ); then
    RESULTS+=("PASS  $name")
  else
    RESULTS+=("FAIL  $name")
    FAILED=1
  fi
}

main() {
  local cmd="${1:-all}"
  case "$cmd" in
    -h | --help | help)
      usage
      exit 0
      ;;
    list)
      run_list
      exit 0
      ;;
  esac

  ensure_docker
  resolve_act
  make_context
  act_args
  RESULTS=()
  FAILED=0

  case "$cmd" in
    ci | module-guard | docs | release-check)
      run_workflow "$cmd"
      ;;
    all)
      for name in "${RUNNABLE[@]}"; do run_workflow "$name"; done
      ;;
    *)
      usage
      exit 2
      ;;
  esac

  printf '\n'
  log "summary"
  printf '%s\n' "${RESULTS[@]}"
  [ "$FAILED" -eq 0 ] || exit 1
}

main "$@"
