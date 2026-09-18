---
title: Search & Ranking
description: Hard-filter matching, graded fuzzy scoring with title boosts, and the known bottleneck.
---

Implementation: `internal/search/search.go`. Two phases: hard-filter, then fuzzy score.

## `MatchesDoc` — hard filters (bare words ignored)

- `Tags`: subset, case-insensitive. `NotTags`: exclusion.
- `DocType`: exact, case-folded.
- `Title` / generic `key:value`: `fuzzyContainsFold` = case-insensitive substring **or** `sahilm/fuzzy` hit.
- `Status`: exact-fold, plus `status:archived` matches `IsArchived()`.
- `Path`: substring-fold.
- Dates: `Created`/`Updated` via `matchDate` (inclusive/exclusive `From`/`To`); `date`/`before`/`after` use either-semantics (`matchEither`). Nil timestamp never matches a present filter.

## `Rank` — bare-word scoring

1. Filter via `MatchesDoc`.
2. No bare words → sort by `Path`.
3. Else `scoreDoc` per doc: every bare word must match — AND semantics, miss drops the doc. A word matches by case-insensitive substring of `SearchBlob` (fallback `Title+" "+Body`); only if that misses does it fall back to `sahilm/fuzzy` against the `Title`.
4. Sort: score desc → most-recent `UpdatedAt` (nil = oldest) → `Path`.

### Score formula

Per bare word, `scoreDoc` adds a base plus a **graded** title boost:

| Contribution | Constant | Value | When |
| --- | --- | --- | --- |
| Match length | — | `len(word)` | word is a substring of the blob (body/frontmatter) |
| Fuzzy score | — | `fuzzy.Find` score | not in blob, but fuzzy-matches the title |
| Exact title | `exactTitleBoost` | `+200` | title equals the word exactly |
| Whole-word title | `titleBoost` | `+100` | word appears as a whole word in the title |
| Partial title | `partialTitleBoost` | `+50` | word appears **inside** a longer title word |

The three title boosts are mutually exclusive, so whole-word/word-boundary hits are preferred over longer words that merely contain the query. A search for `butter` therefore ranks `Butter` (200 + 6) above `Buttermilk` (50 + 6), which in turn edges out a body-only mention (6). Boundaries are UTF-8-aware and treat any non-letter/digit/`_` character (spaces, hyphens, punctuation) as a separator.

:::note[Why graded title boosts?]
An earlier version added a flat `+100` for any title substring, so `buttermilk` and `butternut` tied with an exact `butter` title and order fell through to recency/path. Grading by match quality makes the search feel exact-first.
:::

:::note[Why title-only fuzzy?]
Running the subsequence fuzzy matcher over `SearchBlob` includes the whole body, so almost any short query matches hundreds of unrelated notes (e.g. `kumquat` matched 948 files in a real vault). Body/frontmatter matches therefore require a literal substring; fuzzy is reserved for the short, high-signal title.
:::

## `FilterArchived`

`Rank` never hides; `FilterArchived(docs, include)` / TUI `ctrl+a` layer applies hiding. `archived:true` / `status:archived` still match hidden docs.

:::caution[Known bottleneck]
`scoreDoc` lowercases the whole `SearchBlob` per doc per keystroke and runs `fuzzy.Find` on misses. No pre-lowercasing, no token pre-split, filter maps rebuilt per doc. Fine at fixture scale; the \<50ms/keystroke budget at 5k files needs the [Roadmap](/project/roadmap/) optimization tasks (lowered-blob cache, facet bitmaps, fuzzy-core benchmark).
:::

See also: [Query Syntax](/guide/query-syntax/) for token semantics, and [Backend Evaluation](/reference/backend-evaluation/) for the indexed-alternative decision.
