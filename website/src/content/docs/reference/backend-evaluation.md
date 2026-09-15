---
title: Backend Evaluation
description: Decision record — stay indexless vs bleve vs bluge vs SQLite FTS5.
---

Question: stay indexless (`sahilm/fuzzy` + hand filters) or adopt an embedded full-text library for free-text + fuzzy + faceted custom metadata?

## Why `bleve` was the prior recommendation

`github.com/blevesearch/bleve/v2` (`11k★`, Apache-2.0, Couchbase-backed) is the only pure-Go option covering all three out of box:

- Free text: `text` fields + analyzers, BM25/TF-IDF, highlighting, `scorch` on-disk or `NewMemOnly` in-memory, no `cgo`.
- Fuzzy: `NewFuzzyQuery` (Levenshtein) + `prefix/regexp/wildcard/match_phrase/query-string`.
- Facets/custom metadata: dynamic mapping indexes `Raw.author/views/…` automatically; `terms/numeric-range/date-range` facets; `BooleanQuery(must: facets, should: fuzzy)` combines filter + search.

## Why not `bluge`?

`blugelabs/bluge` (+ `zincsearch`/`blugehq`/`fy0` forks) is lighter and faster at indexing with a cleaner API and richer aggregations (`Terms/Range/Min/Max/HLL/T-Digest`). Reasons to hold off:

- Fragmented ecosystem — unclear canonical fork, near-zero importers, thin docs.
- Mainline query types lack fuzzy/regexp/wildcard (`Term/Phrase/Match/Prefix/Range/Boolean` only); `FuzzyQuery` exists only in forks, unproven.
- Pick `bluge` only if write throughput + readable codebase outweigh fuzzy + facet maturity + community.

## Why not SQLite FTS5?

`FTS5(porter+trigram)` + normal columns gives the fastest indexing and best SQL facets (`WHERE type=? AND tag=? GROUP BY`). Costs:

- No native fuzzy — needs `spellfix1` two-step queries or trigram substring hacks.
- Facets/ranking are hand-written SQL, not a search API.
- Go drivers: `mattn/go-sqlite3` needs `cgo` (breaks static binary/cross-compile); `modernc.org/sqlite` is pure-Go but larger/slower; FTS↔content sync (triggers/external-content) is extra work.
- Pick FTS5 only if you already need SQLite or persistent SQL joins.

## Decision for `mdfu`

- Stay indexless while the \<50ms/keystroke budget holds (see [Search & Ranking](/guide/search-ranking/) bottleneck + [Roadmap](/project/roadmap/) optimization tasks).
- Spike a persistent index only if benchmarks miss: `bleve-mem` first (facets UI), `bluge` for speed comparison, FTS5 for SQL — decide on binary size, `cgo`, cold-start, query latency.
- External servers (`Typesense`/`Meilisearch`) are out of scope: violate the single-static-binary constraint.
