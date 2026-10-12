# Rust port — 5k-file performance budget

**Scope:** todo 33 of the Rust port — measure cold `scan+parse` and
per-keystroke `query+rank` on a synthetic 5k-file vault and compare against the
PLAN budget (`PLAN.md:20`: 5–10k files, cold `scan+parse` **<500 ms**,
per-keystroke `query+rank` **<50 ms**).

**Outcome:** both budgets **PASS** with room to spare. No index, daemon, or
caching layer was added (locked constraint); the measurements use the
production `scan`/`parse`/`query`/`search` code paths unchanged.

## Machine and toolchain

| | |
|---|---|
| CPU | Intel Core i7-8700 @ 3.20 GHz (6 cores / 12 threads) |
| RAM | 31,260 MiB |
| OS / kernel | Linux 6.14.0-37-generic, x86_64 |
| rustc / cargo | 1.98.1 (Homebrew), pinned by `rust-toolchain.toml` |
| Build profile | `bench` (release-like, `opt-level = 3`) |
| Vault location | `/tmp` = **tmpfs** (RAM-backed reads); override with `MDFU_BENCH_ROOT` for disk-backed re-runs |

## Harness

- `crates/mdfu-core/benches/scan_rank.rs` — timed, dependency-free
  harness (`[[bench]] name = "scan_rank", harness = false` in
  `crates/mdfu-core/Cargo.toml`). No Criterion dependency, so the workspace
  manifest stays untouched apart from the one bench entry.
- **Corpus** (deterministic, fixed seed `0x9E3779B97F4A7C15`, xorshift64*):
  5,000 Markdown files across 50 directories, **11,182,768 bytes** (10.66 MiB,
  avg 2,236 B ≈ 2.1 KiB). Each file: YAML frontmatter (`type`, `title`, `tags`,
  `status`, `created`, `updated`, `description`) plus ~2 KiB of prose. Rare
  markers make the query set interesting: `retention` in ~5% of titles
  (title-boost path), `kumquat`/`zebra` in exactly one common file, `perf` tag
  on 12.5%, `draft` status on 20%.
- **Cold `scan+parse`** per run: fresh `scan::walk_markdown` (no cache) plus
  `parse::parse_file` for every discovered path; 20 runs, wall clock. The first
  run is recorded separately ("first") from the percentile pool.
- **Per-keystroke `query+rank`** per query: `query::parse` + `search::rank` +
  the pointer→index map that is the body of the CLI's injected filter
  (`build_filter_fn`), 50 reps per query. This is exactly what a keystroke costs
  in the picker.
- Percentiles are nearest-rank over the sorted samples
  (`p50 = ceil(0.50·n)`, `p95 = ceil(0.95·n)`), so with 20 runs p95 is
  the second-worst sample — a conservative, outlier-sensitive read.
- **Failure guard:** an empty corpus (no files generated, no files walked, or
  no docs parsed) exits 1 with `empty corpus: ...`; the harness can never
  report a silent `0 ms`. Any run that loses files (parsed ≠ 5,000) also fails.
- A budget miss prints `MISS` but still exits 0: misses are **recorded**, not
  treated as port blockers.

## Commands (from the workspace root)

```sh
# Recorded run (defaults: 5,000 files, 20 cold runs, 50 reps/query)
cargo bench -p mdfu-core --bench scan_rank

# Full workspace acceptance (compiles + runs every bench target; exit 0)
cargo bench

# QA: empty corpus must fail, not report 0 ms
MDFU_BENCH_FILES=0 cargo bench -p mdfu-core --bench scan_rank   # exit 1

# Machine-readable capture
MDFU_BENCH_JSON=/tmp/opencode/bench-report.json cargo bench -p mdfu-core --bench scan_rank
```

Overrides: `MDFU_BENCH_FILES`, `MDFU_BENCH_DIRS`, `MDFU_BENCH_COLD_RUNS`,
`MDFU_BENCH_KEYSTROKE_REPS`, `MDFU_BENCH_ROOT`, `MDFU_BENCH_KEEP`,
`MDFU_BENCH_JSON`.

## Results

### Cold `scan+parse` (5,000 files, wall clock per full run)

| Run | first | p50 | p95 | min | max |
|---|---|---|---|---|---|
| isolated #1 | 152.98 ms | 126.26 ms | 145.18 ms | 120.34 ms | 152.98 ms |
| workspace `cargo bench` | 292.78 ms | 142.92 ms | 285.40 ms | 125.59 ms | 292.78 ms |
| **isolated #2 (recorded)** | **138.48 ms** | **135.04 ms** | **231.37 ms** | **120.18 ms** | **231.60 ms** |

The minimum is stable at ~120–126 ms across all runs; the p95 spread
(145–285 ms) is scheduler/outlier noise (the workspace run overlapped other
cargo test targets on the same box). **Verdict: PASS** — worst observed p95
285.40 ms is still 1.75× under the 500 ms budget.

### Per-keystroke `query+rank` (recorded run, 50 reps each)

| Query | Kind | Hits | p50 | p95 | parse p50 | rank p50 |
|---|---|---|---|---|---|---|
| `retention` | bare word, title boost (~5% titles) | 563 | 10.57 ms | 13.42 ms | 1.18 µs | 10.47 ms |
| `kumquat zebra` | two rare bare words, AND | 1 | 7.11 ms | 9.23 ms | 1.00 µs | 6.96 ms |
| `type:metric` | hard `key:value` filter, no bare words | 500 | 0.25 ms | 0.40 ms | 0.58 µs | 0.15 ms |
| `rtntn` | fuzzy-only subsequence over titles | 563 | 10.23 ms | 10.74 ms | 1.15 µs | 10.12 ms |
| `status:draft perf` | hard filter + bare word | 998 | 2.83 ms | 3.07 ms | 1.33 µs | 2.71 ms |

Query parsing is negligible (~1 µs); rank dominates. Across all three runs the
worst per-query p95 was 13.42 ms (`retention`) and the slowest p50 was 10.57 ms.
**Verdict: PASS** — worst observed p95 13.42 ms is 3.7× under the 50 ms budget.

### Budget summary

| Metric | Budget | Recorded p95 | Worst observed p95 | Verdict |
|---|---|---|---|---|
| Cold `scan+parse` | <500 ms | 231.37 ms | 285.40 ms | **PASS** |
| Per-keystroke `query+rank` | <50 ms | 13.42 ms | 13.42 ms | **PASS** |

## Context and caveats

- **No index/daemon.** The harness calls the same walker/parser/ranker the CLI
  uses; nothing was cached between keystrokes or runs to hit the budgets.
- **Prior Go-era context (read-only):** `reports/tui-search-perf-regression.md`
  notes that Go `search.Rank` cost ~75 ms per keystroke over a 2,558-file real
  vault (pre-T2 "indexless rank optimization"), i.e. a Go-era keystroke miss on
  real data. The Rust port's worst synthetic keystroke here is 13.42 ms.
- **Corpus shape:** ~2 KiB bodies are representative of note vaults by count;
  vaults with multi-megabyte notes rank slower on body-substring queries (the
  Go report measured a 2.5 MB note). The benchmark is a budget floor check, not
  a worst-case body-size stress test.
- **tmpfs:** `/tmp` is RAM-backed, so the cold number excludes disk read
  latency; set `MDFU_BENCH_ROOT` to a disk-backed path to re-measure. The scan
  still performs 5,000 real `open`/`read` syscalls and full YAML parsing.
- **p95 definition:** second-worst of 20 samples; medians (cold ~120–143 ms;
  keystroke 0.25–10.6 ms by query) are the stable signal. A miss would have
  been recorded here as-is, not "fixed" with an index.
