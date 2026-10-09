#!/usr/bin/env bash
#
# Go-vs-Rust differential parity harness (plan task 32).
#
# Builds the Go binary from the frozen baseline SHA recorded in rust/PARITY.md
# (via `git archive`, so the working tree is never touched), builds the Rust
# release binary, and runs rust/crates/mdfu-cli/tests/parity.rs against both.
# The test compares stdout, stderr, exit code, and rewritten vault bytes for
# the non-TTY note-filter and task-subcommand surface. TTY box bytes,
# SGR-exact output, and the interactive TUI are excluded and documented in
# rust/PARITY.md.
#
#   scripts/parity.sh            harness must report zero diffs
#   scripts/parity.sh --mutate   append one stdout byte inside a Go shim; the
#                                harness must report PARITY-DIFF (this mode
#                                exits 0 only when the tamper is detected)
#
set -euo pipefail

usage() {
  sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'
}

mode=normal
for arg in "$@"; do
  case "$arg" in
    --mutate) mode=mutate ;;
    -h|--help) usage; exit 0 ;;
    *) echo "parity: unknown argument: $arg" >&2; usage >&2; exit 2 ;;
  esac
done

command -v git >/dev/null || { echo "parity: git is required" >&2; exit 1; }
command -v go >/dev/null || { echo "parity: the Go toolchain is required" >&2; exit 1; }
command -v cargo >/dev/null || { echo "parity: cargo is required" >&2; exit 1; }

root="$(git rev-parse --show-toplevel)"
cd "$root"

sha="$(sed -n 's/.*Frozen Go baseline SHA: `\([0-9a-f]\{40\}\)`.*/\1/p' rust/PARITY.md | head -n1)"
if [ -z "$sha" ]; then
  echo "parity: cannot read the frozen baseline SHA from rust/PARITY.md" >&2
  exit 1
fi
head_sha="$(git rev-parse HEAD)"
echo "parity: frozen baseline $sha"
echo "parity: working HEAD    $head_sha"
if git diff --quiet "$sha" "$head_sha" -- cmd internal tests go.mod go.sum; then
  echo "parity: Go build inputs unchanged from baseline (cmd/ internal/ tests/ go.mod go.sum)"
else
  echo "parity: WARNING Go build inputs differ from baseline; the baseline archive is still what gets built" >&2
fi
echo "parity: $(go version | awk '{print $1, $3}'), $(cargo --version)"

work="$(mktemp -d "${TMPDIR:-/tmp}/mdfu-parity.XXXXXX")"
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/go-src"
git archive "$sha" | tar -x -C "$work/go-src"
(
  cd "$work/go-src"
  go build -o "$work/mdfu-go" ./cmd/mdfu
)
echo "parity: Go binary sha256 $(sha256sum "$work/mdfu-go" | cut -d' ' -f1)"

(
  cd rust
  cargo build --release --quiet
)
rust_bin="$root/rust/target/release/mdfu"
echo "parity: Rust binary $rust_bin"

run_harness() {
  (
    cd rust
    MDFU_PARITY_GO="$1" MDFU_PARITY_RUST="$rust_bin" \
      cargo test -p mdfu-cli --release --test parity -- --nocapture
  )
}

if [ "$mode" = mutate ]; then
  shim="$work/mdfu-go-tampered"
  {
    echo '#!/usr/bin/env bash'
    echo 'set -u'
    echo 'out="$(mktemp)"'
    printf '%s\n' "\"$work/mdfu-go\" \"\$@\" >\"\$out\""
    echo 'rc=$?'
    echo 'cat "$out"'
    echo "printf 'X'"
    echo 'rm -f "$out"'
    echo 'exit $rc'
  } >"$shim"
  chmod +x "$shim"
  echo "parity: mutation mode: every Go stdout gets one appended byte"
  set +e
  run_harness "$shim" >"$work/mutate.log" 2>&1
  rc=$?
  set -e
  if [ "$rc" -eq 0 ]; then
    echo "parity mutation: NOT DETECTED: the harness stayed green on tampered output" >&2
    cat "$work/mutate.log" >&2
    exit 1
  fi
  if ! grep -q 'PARITY-DIFF' "$work/mutate.log"; then
    echo "parity mutation: harness failed without a PARITY-DIFF report" >&2
    tail -n 60 "$work/mutate.log" >&2
    exit 1
  fi
  echo "parity mutation: DETECTED (cargo test exit $rc, $(grep -c 'PARITY-DIFF' "$work/mutate.log") report lines)"
  grep -m3 'PARITY-DIFF' "$work/mutate.log"
  exit 0
fi

run_harness "$work/mdfu-go"
echo "parity: zero diffs on the covered surface"
