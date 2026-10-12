---
title: Search & Ranking
description: Hard-filter matching, graded fuzzy scoring with title boosts, and the known bottleneck.
---

Implementation: `crates/mdfu-core/src/search.rs`. Two phases: hard-filter, then fuzzy score.

## `matches_doc` — hard filters (bare words ignored)

- `tags`: subset, case-insensitive. `not_tags`: exclusion.
- `doc_type`: exact, case-folded.
- `title` / generic `key:value`: `fuzzy_contains_fold` = case-insensitive substring **or** a fuzzy hit.
- `status`: exact-fold, plus `status:archived` matches `Document::is_archived()`.
- `path`: substring-fold.
- Dates: `created`/`updated` via the `DateFilter` (inclusive/exclusive `from`/`to`); `date`/`before`/`after` use either-field semantics. An absent timestamp never matches a present filter.

## `rank` — bare-word scoring

1. Filter via `matches_doc`.
2. No bare words → sort by `path`.
3. Else `score_doc` per doc: every bare word must match — AND semantics, a miss drops the doc. A word matches by case-insensitive substring of `search_blob` (fallback `title + " " + body`); only if that misses does it fall back to the fuzzy matcher against the `title`.
4. Sort: score desc → most-recent `updated_at` (absent = oldest) → `path`.

### Score formula

Per bare word, `score_doc` adds a base plus a **graded** title boost:

| Contribution | Constant | Value | When |
| --- | --- | --- | --- |
| Match length | — | `len(word)` | word is a substring of the blob (body/frontmatter) |
| Fuzzy score | — | `fuzzy::find` score | not in blob, but fuzzy-matches the title |
| Exact title | `EXACT_TITLE_BOOST` | `+200` | title equals the word exactly |
| Whole-word title | `TITLE_BOOST` | `+100` | word appears as a whole word in the title |
| Partial title | `PARTIAL_TITLE_BOOST` | `+50` | word appears **inside** a longer title word |

The three title boosts are mutually exclusive, so whole-word/word-boundary hits are preferred over longer words that merely contain the query. A search for `butter` therefore ranks `Butter` (200 + 6) above `Buttermilk` (50 + 6), which in turn edges out a body-only mention (6). Boundaries are UTF-8-aware and treat any non-letter/digit/`_` character (spaces, hyphens, punctuation) as a separator.

:::note[Why graded title boosts?]
An earlier version added a flat `+100` for any title substring, so `buttermilk` and `butternut` tied with an exact `butter` title and order fell through to recency/path. Grading by match quality makes the search feel exact-first.
:::

:::note[Why title-only fuzzy?]
Running the subsequence fuzzy matcher over the whole search blob includes the entire body, so almost any short query matches hundreds of unrelated notes (e.g. `kumquat` matched 948 files in a real vault). Body/frontmatter matches therefore require a literal substring; fuzzy is reserved for the short, high-signal title.
:::

## `filter_archived`

`rank` never hides; `filter_archived(docs, include)` / TUI `ctrl+a` layer applies hiding. `archived: true` / `status: archived` still match hidden docs.

:::caution[Known bottleneck]
`score_doc` lowercases the whole search blob per doc per keystroke and runs the fuzzy matcher on misses. No pre-lowercasing, no token pre-split, filter maps rebuilt per doc. Fine at fixture scale; the \<50ms/keystroke budget at 5k files needs the [Roadmap](/project/roadmap/) optimization tasks (lowered-blob cache, facet bitmaps, fuzzy-core benchmark).
:::

See also: [Query Syntax](/guide/query-syntax/) for token semantics, and [Backend Evaluation](/reference/backend-evaluation/) for the indexed-alternative decision.