# Gap analysis — mdfu vs `qmd` (tobi/qmd)

**Scope:** comparison of `mdfu` (this repo, Rust) against
[`tobi/qmd`](https://github.com/tobi/qmd) v2.8.3 (`main`, last push
2026-10-06), limited to **similar features** — the areas where both tools
target the same problem (find a note in a local markdown vault). Recorded
2026-10-12.

**Outcome:** the two projects are **not competitors** and most of the raw
feature delta is category mismatch, not mdfu falling behind. `qmd` is an
agent-first retrieval server with no TUI; mdfu is a human-first interactive
picker that cannot serve as a retrieval server yet. Two genuine gaps are
worth closing cheaply (scores thrown away at the output boundary, bare-word
ranking with no IDF), one prerequisite must land first (**no relevance
benchmark exists**), and one item (MCP) is the highest value-per-day work
available. The `qmd` semantic stack (~2 GB of GGUF models) should **not** be
ported — it destroys the single-static-binary identity.

---

## 0. Framing: why a raw feature diff misleads

| | **`qmd`** | **`mdfu`** |
|---|---|---|
| Version / language | v2.8.3 · TypeScript · MIT | 0.1.0 · Rust 2021 · MIT |
| Popularity | **30,345★**, 1,908 forks, 781 commits | n/a |
| Created / last push | Dec 2025 / **Oct 2026** | in-flight |
| Node size | 5,304-line `src/cli/qmd.ts` | 35,468 LOC across 3 crates |
| **What it is** | **Agent-first retrieval server.** BM25 + dense vectors + LLM rerank over GGUF models, SQLite FTS5 + `sqlite-vec`, MCP server, SDK. Built for an LLM to call. | **Human-first interactive picker.** Static binary for a human to find one note, plus an mdtask-compatible task CLI/TUI. |
| **Interactive TUI** | **None** — no TUI dependency exists (verified: no blessed/ink/inquirer in `package.json`) | **ratatui picker** — live re-rank, multi-select, preview, links |
| Core dependencies | better-sqlite3, node-llama-cpp, sqlite-vec, tree-sitter×4, zod | `ignore`, `regex`, `serde`, `serde_json`, `yaml_serde`, `etcetera`, ratatui |
| Network / models | **~2 GB GGUF download on first run** | **none, ever** |
| Cold start | model load, seconds | **231 ms p95 for 5k files** |

Two facts decide the whole roadmap:

1. **`qmd` reports on its own fixture that BM25 alone scores ~0.50 and full
   hybrid ~1.00.** Semantic search is a *supplement*, not the foundation.
2. **mdfu's job — a human typing 3 characters and getting the right note in
   under 200 ms — is a job `qmd` cannot do at all**, because it has no TUI.

So the useful gap table is not "what does qmd have that mdfu lacks" but
"which of qmd's capabilities are reachable for a static binary with no
network, no daemon, and no models."

---

## 1. Where mdfu is already ahead (do not regress)

1. **Interactive picker.** qmd is strictly non-interactive.
2. **Typed date range filters.** `created:2026-01..2026-03`, `before:`,
   `after:`, RFC3339 + granularity (`rust/crates/mdfu-core/src/query.rs:59-70`).
   qmd's filter AST has **no date operator** — you'd be doing string `prefix`
   matches.
3. **Format-agnostic normalization.** OKF v0.1/v0.2 + Portent/Tolaria → one
   `Document`. qmd has no cross-format concept; `qmd.metadata` is its own
   opt-in namespace.
4. **Never-fails-hard parse + frozen exit contract.** `ParseError` is a
   `Document` field, not a failure; the 0/1/2 matrix is enforced by
   `rust/crates/mdfu-cli/tests/parity.rs` with a `--mutate` tamper check.
5. **mdtask subsystem.** qmd has nothing resembling `mdfu task` × 10
   subcommands + the `mdfu tasks` TUI + any-checkbox mode.
6. **Snippets + highlighting.** `output::snippet()` and `highlight_re()` —
   parity with qmd's `extractSnippet`.

---

## 2. Verified baseline

Read directly from source, not assumed from docs.

### mdfu's actual retrieval path

`rust/crates/mdfu-core/src/search.rs:379-395` (`score_doc`):

```rust
if searchable.contains(&needle) {
    total += i32::try_from(needle.chars().count())...;   // score == needle length
} else {
    let matches = fuzzy::find(word, &[doc.title.as_str()]);  // fuzzy fallback is TITLE-ONLY
    ...
}
total += title_match_boost(&title_folded, &needle);  // 200 / 100 / 50
```

A 3-char word appearing in every document scores 3. A rare 12-char word also
scores 12. **No inverse document frequency, no term frequency**, and the fuzzy
fallback only ever looks at the title. `fuzzy` is a local fzf-style
implementation inlined at `search.rs:32` — there is no fuzzy crate in
`mdfu-core`'s dependency list.

Ranking constants (`search.rs:19-23`): `EXACT_TITLE_BOOST = 200`,
`TITLE_BOOST = 100`, `PARTIAL_TITLE_BOOST = 50`.

### mdfu's query grammar (`query.rs:72-98`)

```rust
pub struct Query {
    pub bare: Vec<String>,                          // AND-combined
    pub tags: Vec<String>, pub not_tags: Vec<String>,
    pub doc_type: String,   // type:
    pub title: String,      // title:
    pub status: String,     // status:
    pub path: String,       // path:
    pub generic: BTreeMap<String, String>,          // any other key:value  <-- STRINGLY TYPED
    pub created: Option<DateFilter>,
    pub updated: Option<DateFilter>,
    pub raw: String,
}
```

### mdfu's output contract (`output.rs:18-33`, `pipeline.rs:113`)

```rust
pub struct Result {
    pub path: String,
    pub score: f64,   // "Ranking score. The Go CLI currently always emits `0`."
    pub title: String,
    #[serde(rename = "type")] pub doc_type: String,
    pub snippet: String,
}
```

`pipeline.rs:113` hardcodes `score: 0.0`. `search::rank` computes real `i32`
scores that die at the output boundary.

### CLI surface (`mdfu-cli/src/main.rs:54-94`)

Flags: `--root` (default `.`), `--hidden`, `--no-ignore`, `--limit N`,
`--format paths|json|vimgrep`, `--archived`, `--no-hyperlinks`,
`--config PATH|default`, `--version`, positional `QUERY`. Subcommands
(`mdfu task` × 10, `mdfu tasks`) route *before* clap in `router.rs`. Exit
codes 0/1/2.

### Confirmed absences

Grep-verified across all of `rust/crates/` — **zero** hits for: `BM25`,
`InvertedIndex`, `postings`, `rusqlite`, `sqlite`, `tantivy`, `sled`,
`Embedding`, `Stemmer`, `stopword`, `Levenshtein`, `trigram`, `Rrf`,
`reciprocal`, `fusion`, `notify`/`inotify`/`watcher`, `daemon`, `tokio`,
`TcpListener`, `jsonrpc`, `modelcontextprotocol`.

### Measured headroom (`reports/rust-port-perf.md`, 5k-file corpus)

| Metric | Budget | Recorded p95 | Headroom |
|---|---|---|---|
| Cold `scan+parse` | <500 ms | 231.37 ms | **2.2×** |
| Per-keystroke `query+rank` | <50 ms | 13.42 ms | **3.7×** |

---

## 3. The gaps worth closing

Budgets: **XS** = <½d · **S** = ½–1d · **M** = 2–5d · **L** = 6–10d, one
developer with existing repo context. See §6 for why these run long.

### D1 — No relevance-quality benchmark ⛔ *gates A2 and A5*

qmd `qmd bench <fixture.json>` reports precision@k / recall / MRR / F1 across
all four backends, with a fixture format (`expected_files`,
`expected_in_top_k`, `type: exact|semantic|topical|cross-domain|alias`).

mdfu has `scan_rank` — **timing only, zero relevance measurement** — and
`parity.rs`, which locks *current* behavior byte-for-byte but cannot tell you
whether that behavior is *good*.

You cannot justify a ranking change without measuring relevance first.

**Budget S–M (1–2d).** Port the fixture format + precision@k/recall/MRR into a
`mdfu bench` subcommand over `testdata/`. **Do this before A2** — it is the
cheapest item in the list and it gates the most valuable one.

### A1 — Scores are computed then thrown away (`score: 0.0`)

**Evidence:** `pipeline.rs:113`, `output.rs:24`.

qmd: normalized 0–1 scores, `--min-score`, `--explain` (dumps `ftsScores`,
`vectorScores`, per-subquery `rrfContribution`).

This is a **defect, not a feature gap**. The number exists internally and is
discarded at the boundary.

**Budget S (0.5–1d).** Thread the `i32` score through `output::Result`,
normalize to 0..1, add `--min-score` and an `--explain` JSON block.

⚠️ **Parity tax:** emitting non-zero scores diverges from the frozen Go
baseline → recorded divergence + `parity.rs` guard + `rust/PARITY.md` entry.

### A2 — Retrieval quality: bare-word scoring has no IDF

**Evidence:** `search.rs:379-395` (quoted in §2).

qmd: BM25 via FTS5.

This is mdfu's **biggest real quality gap** — and it is fixable *without* an
index, which is why it matters more than A6.

**Budget M (2–4d).** Compute per-run term frequencies + IDF during the parse
pass that already happens on every invocation, and score BM25-style. Because
it rebuilds every run, it **keeps the no-persistent-index constraint intact**.
Follow-on: fuzzy over `search_blob` instead of title-only (0.5d).

⚠️ **Parity tax:** changes result ordering → breaks `parity.rs` byte
comparisons. Gate behind `--rank=legacy|bm25` (default `legacy` for the first
release) or record it as an explicit divergence.

### A3 — Facet counts / metadata discovery

qmd: `qmd collection metadata` — keys, types, coverage, distinct counts, value
distributions, paged windows (`--key-limit`/`--value-limit`/`--offset`), and
`--filter` to preview what survives a filter.

mdfu: PLAN.md **T8, still unchecked**. Nothing.

High picker value — "what tags do I even have" is how you build a query.

**Budget M (3–5d)** — larger than it looks because of the paged/distinct-count
UI. Every document's normalized fields are already in memory from the single
scan pass. Surface as `--facets` JSON plus a TUI pane.

### A4 — `get` / line-range retrieval: the missing primitive

qmd: `qmd get <file>[:from[:count]]`, `multi-get <glob>`, `--max-bytes`, line
numbers on by default, **docid addressing** (`#abc123`), fuzzy-suggestions on
miss.

mdfu: **no `get` subcommand at all.** Only `open.rs` shelling out to `$EDITOR`.

This is the single thing that makes mdfu scriptable/LLM-usable beyond one-shot
search.

**Budget S–M (1–2d)** — bodies are already in memory and path resolution
exists. Docid (6-char hash, qmd-compatible) is +0.5d.

### A5 — Filtering is stringly-typed

**Evidence:** `Query.generic: BTreeMap<String, String>` (`query.rs:90`) —
*every* generic `key:value` is a stringified compare. No numeric `gt`/`lt`, no
`exists`, no `contains`/`prefix`/`suffix`, no boolean logic, no
`and`/`or`/`not`.

qmd's filter AST: `eq ne gt gte lt lte in nin all contains prefix suffix type
exists` + `caseInsensitive` + recursive `and`/`or`/`not`.

The field type already exists (`raw: Map<String, Value>`, `model.rs:135`) — it
is just flattened to strings at query-parse time.

**Budget L (5–8d).** Biggest syntax change; breaks the single-box query model;
touches `query.rs` + `search.rs` + docs + parity tests. Fold in PLAN.md **T7**
(`|` alternation, negation beyond tags, per-field boosting) while you are in
there — it is the same surgery.

### A8 — Configurable editor URI template

qmd: OSC 8 hyperlinks on TTY, configurable `editor_uri` template
(`vscode://file{path}:{line}:{col}`), plain text when piped, `NO_COLOR`
support.

mdfu has OSC 8 (`mdfu-tui/src/links.rs`, `osc8_link`) and `--no-hyperlinks`.
**Missing:** the configurable `editor_uri` template; config is theme-only.

**Budget XS (0.5d)** — one config key plus one format function.

### B1 — MCP server ⭐ best value-per-day on this list

qmd: full MCP — 5 tools, stdio + HTTP + daemon, PID file, Origin/Host
validation against DNS rebinding.

mdfu: **nothing.** Verified no `jsonrpc` / `TcpListener` / MCP anywhere.

mdfu is already an excellent agent backend — `--format json`, 13 ms
keystrokes, zero runtime deps. It just is not *discoverable*. And **stdio MCP
spawns `mdfu` as a subprocess, which is not a daemon** — it stays inside the
locked constraint.

**Budget M (3–5d)**, stdio-only, 4 tools (`search`, `get`, `multi_get`,
`facets`). Needs `rmcp` (~15 crates, real binary-size cost). **Skip the
HTTP/daemon transport** — that is the part that violates "no daemon."

### A6 — Persistent index: *defer, and probably never*

mdfu: none. Full re-walk + re-parse on **every invocation**.

**Evidence:** §2 measured headroom — 2.2× on cold start, 3.7× on keystrokes.
**At the stated 5–10k budget mdfu does not need an index.**

The real risk is the long tail: the Go-era investigation
(`reports/tui-search-perf-regression.md`) recorded ~75 ms/keystroke from
fuzzy-scoring a 2.5 MB note in a 2,558-file/863 MB real vault. The synthetic
5k corpus (~2.1 KB files) does not stress this.

**Budget L (6–10d)** for a real spike. `tantivy` is the only pure-Rust dep
covering BM25 + fuzzy + facets together; bundled `rusqlite` + FTS5 if you want
SQL facets. Both roughly double binary size and both violate the spirit of the
constraint.

**Recommendation: do A2 instead.** It buys most of the ranking quality at a
fraction of the cost and zero architectural cost. Revisit only past ~10k files.

Note that `website/src/content/docs/reference/backend-evaluation.md` is still
an entirely **Go-era** decision record (bleve, bluge, `mattn/go-sqlite3` vs
`modernc.org/sqlite`). The Rust port never got its own decision record, so
PLAN.md **T5 is effectively unstarted**.

### B2 — `context`: curated per-path descriptions

qmd: `qmd://collection/path` → description; a tree, longest-prefix-wins,
returned alongside every hit so an LLM can judge relevance.

mdfu: no equivalent. (It does have `mdfu-tui/src/links.rs` at 1,518 LOC doing
markdown-link extraction in the preview — an anchor graph exists.)

**Budget M (3–4d)** — requires an `output::Result` schema change. Real value
for agents, marginal for humans.

### B3 — Multi-collection registry

qmd: named collections with path + mask + ignore + include-by-default, a
global registry searchable from any cwd, and `--index <name>` named indexes.

mdfu: a single `--root`; the config file is theme-only.

**Budget M (4–6d)** — mostly config schema, scan fan-out, result namespacing,
and a `collection` subcommand family. Lower value than it looks: `mdfu --root`
already covers the per-vault case, and cross-vault querying is rare for a
human picker.

---

## 4. Out of scope: do NOT port

| | Why not |
|---|---|
| **C1. Vector search / LLM rerank / query expansion / HyDE** | **~2 GB of GGUF models** (`embeddinggemma-300M` 300 MB + `qwen3-reranker-0.6b` 640 MB + `qmd-query-expansion-1.7B` 1.1 GB) auto-downloaded from HuggingFace on first run, via node-llama-cpp with Metal/Vulkan/CUDA backends. Turns a 231 ms zero-dep binary into a multi-second, 2 GB-install product. **Categorical regression.** Also: qmd's own numbers say BM25 gets you half the quality — so if you want semantic search, buy the *cheap* half: a synonym/alias table in config, **XS (0.5d)**, no model. |
| **C2. HTTP/daemon MCP transport** | Requires a persistent process, PID files, port binding, and an auth story. Directly violates "no daemon." |
| **C3. `qmd trust` config sandboxing** | Exists *because* qmd executes shell commands (`update: "git pull"`) from a config file that travels in a `git clone`. mdfu runs no hooks. **Not a gap — a property worth keeping.** |
| **C4. Tree-sitter AST chunking** | qmd chunks *code*. mdfu indexes *markdown notes*. Not applicable. |
| **C5. Per-collection `update` hooks** | A consequence of A6 landing. Irrelevant without it. |

---

## 5. Recommended sequence

```
Phase 0  measure first, unblocks the rest
         D1 relevance bench .............. 1-2d
         A1 real scores + --min-score .... 0.5-1d
                                          ── 2-3d

Phase 1  quality win, zero architectural cost
         A2 per-run IDF/BM25 ranking ...... 2-4d   (gated on D1)
         A8 editor_uri config ............. 0.5d
                                          ── 2.5-4.5d

Phase 2  agent surface — the real differentiator
         A4 get / multi-get / docid ....... 1-2d
         B1 MCP stdio server ............. 3-5d
                                          ── 4-7d

Phase 3  query expressiveness
         A5 typed filter AST (folds in T7)  5-8d

Phase 4  picker UX
         A3 facet counts ................ 3-5d

DEFER    A6 persistent index (L, 6-10d) · B3 collections (M, 4-6d)
NEVER    C1 semantic stack · C2 daemon · C3 trust · C4 AST · C5 hooks
```

**Phases 0–2: ~9–15 days.** That gets a measurably better ranker, a real
score surface, a retrievable-document primitive, and MCP discoverability —
while keeping the single-static-binary, no-network, no-model, 231 ms
cold-start identity.

Full A-tier + B1 + B2 + B3 ≈ **28–45 days**, which is roughly "reimplement qmd
in Rust, worse." Phases 0–2 is the version that makes mdfu *better at what it
already is*.

---

## 6. Caveats — where this could be wrong

1. **The qmd side is doc-verified, not source-verified.** I read its `README.md`,
   `package.json`, and the `case` dispatch table in the 5,304-line
   `src/cli/qmd.ts`, and confirmed no TUI dependency exists. I did not read its
   search internals. The RRF/fusion design (k=60, ×2 original-query weight,
   +0.05 top-rank bonus, position-aware blend 75/60/40) is **qmd's own
   documentation** — treat the design as accurate and any constant as
   illustrative.
2. **The budgets are the soft part.** They are calibrated against *this* repo's
   gates: `clippy::pedantic` at `-D warnings`, `unsafe_code = "forbid"`,
   `unwrap_used`/`panic` denied, a 455-line `mdfu-tui/tests/guards.rs` build
   gate, and byte-exact `parity.rs` against a frozen Go SHA with a tamper
   check. That is roughly a **1.5–2× multiplier** versus a codebase without a
   frozen-parity contract — and it bites hardest on A2 and A5, the two items
   that change ranking or query semantics and therefore collide with the
   baseline. A fresh developer without the parity contract in their head adds
   2–3 days up front.
3. **"Never" on C1 is a product judgment, not a technical one.** If the goal is
   literally *"be qmd but in Rust,"* then C1+B1+B3+A6 **is** the roadmap
   (~30–40 days) — and the result is a worse qmd with a slower cold start.
   This analysis assumes the goal is deepening mdfu's actual wedge.
4. **A2 could surprise you.** The synthetic 5k corpus (~2.1 KB files) does not
   stress the long tail. Before committing 4 days, point `scan_rank` at the
   real vault — the 2.5 MB-note path may already be the worst keystroke, in
   which case A2's payoff is larger than budgeted and A6's window is closer
   than it looks.

---

## 7. Doc debt found while comparing

`website/src/content/docs/reference/backend-evaluation.md` is still a pure Go
decision record — `bleve`, `bluge`, `mattn/go-sqlite3` vs `modernc.org/sqlite`.
None of it applies post-port.

`website/src/content/docs/guide/configuration.md` documents two keys that are
**accepted but inert** in the Rust port: `markdown_style` and
`preview_header_color`.

**Budget XS (0.5d).** Either rewrite the backend-evaluation page for the Rust
candidate set (`tantivy` vs bundled `rusqlite`/FTS5 vs stay-indexless) or
delete it, and stop advertising inert config keys. The docs currently mislead
the next contributor — including on the decision that gates A6.