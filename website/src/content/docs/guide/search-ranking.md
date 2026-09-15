---
title: Search & Ranking
description: Hard-filter matching, fuzzy scoring with title boost, and the known bottleneck.
---

Implementation: `internal/search/search.go`. Two phases: hard-filter, then fuzzy score.

## `MatchesDoc` — hard filters (bare words ignored)

- `Tags`: subset, case-insensitive. `NotTags`: exclusion.
- `DocType`: exact, case-folded.
- `Title` / generic `key:value`: `fuzzyContainsFold` = case-insensitive substring **or** `sahilm/fuzzy` hit.
- `Status`: exact-fold, plus `status:archived` matches `IsArchived()`.
- `Path`: substring-fold.
- Dates: `Created`/`Updated` via `matchDate` (inclusive/exclusive `From`/`To`); `date`/`before`/`after` use either-semantics (`matchEither`). Nil timestamp never matches a present filter.

## `Rank` — fuzzy scoring

1. Filter via `MatchesDoc`.
2. No bare words → sort by `Path`.
3. Else `scoreDoc` per doc: every bare word must fuzzy-match `SearchBlob` (fallback `Title+" "+Body`) via `fuzzy.Find` — AND semantics, miss drops the doc. Score = sum of `fuzzy` scores + `+100` per word that is a case-insensitive substring of `Title` (`titleBoost`).
4. Sort: score desc → most-recent `UpdatedAt` (nil = oldest) → `Path`.

## `FilterArchived`

`Rank` never hides; `FilterArchived(docs, include)` / TUI `ctrl+a` layer applies hiding. `archived:true` / `status:archived` still match hidden docs.

:::caution[Known bottleneck]
`scoreDoc` calls `fuzzy.Find(word, []string{wholeBlob})` per doc per word — `O(docs × words × blobLen)`. No pre-lowercasing, no token pre-split, filter maps rebuilt per doc. Fine at fixture scale; the \<50ms/keystroke budget at 5k files needs the [Roadmap](/project/roadmap/) optimization tasks (lowered-blob cache, facet bitmaps, fuzzy-core benchmark).
:::

See also: [Query Syntax](/guide/query-syntax/) for token semantics, and [Backend Evaluation](/reference/backend-evaluation/) for the indexed-alternative decision.
