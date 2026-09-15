---
title: Query Syntax
description: Single-box query language — bare words fuzzy-match, key:value hard-filters, AND-combined.
---

Single-box input (`internal/query/query.go:Parse`). Bare words fuzzy-match; `key:value` tokens hard-filter. Tokens combine with **AND**. Keys case-insensitive; values preserve case. Quote multi-word values: `title:"Monthly Active"`. `|` alternation is **not** supported in v1 (reserved, see the [Roadmap](/project/roadmap/)).

| Token | Meaning | Example |
|---|---|---|
| `word …` | Bare words, AND-combined, fuzzy-matched against `SearchBlob`. | `kumquat zebra` |
| `tag:v` / `tags:a,b` | Include tag(s), case-insensitive. `tags:` splits on commas. | `tag:launch` |
| `-v` / `!v` inside tag value | Negate a tag. Only tag values support negation. | `tag:-launch` |
| `type:V` | Normalized `type`, case-insensitive exact. | `type:Task` |
| `title:text` | Fuzzy-contains (case-insensitive) against title. Repeatable — joined with space. | `title:Monthly` |
| `path:sub` | Case-insensitive substring of file path. | `path:portent` |
| `status:V` | Case-insensitive exact against lowercased `status`. `status:archived` also matches `archived:true`. | `status:Draft` |
| `organized:V` | Boolean filter over normalized `organized` (`true`/`false`; unset = `false`). | `organized:true` |
| `archived:V` | Boolean filter over `IsArchived()`. | `archived:true` |
| `created:D`, `updated:D` | Date filter on `CreatedAt`/`UpdatedAt`. | `created:2024-05-01` |
| `date:D` | Matches if **either** date satisfies `D`. | `date:2023-11-15` |
| `before:D` / `after:D` | Exclusive upper/lower bound on **either** date. | `before:2024-01-01` |
| `key:value` (other) | Generic filter: `Raw` lookup case-insensitive (underscores ignored), then normalized fields (`description`, `resource`, `role`, `tags`, `body`, `format`, `belongs_to`/`related_to`, dates…). Value fuzzy-matches. Unknown key → no match. | `author:bob` |

## Date forms (`D`)

| Form | Meaning |
|---|---|
| `2024-03-15` | Exact day (inclusive). `2024-01` = whole month; RFC3339 = instant. |
| `A..B` | Inclusive range. Either end may be empty (`2024-01-01..`, `..2024-12-31`). |
| `>=D` / `<=D` | Inclusive lower/upper bound (day/month expands to start/end of period). |
| `>D` / `<D` | Exclusive lower/upper bound. |

Supported layouts: `YYYY-MM-DD`, `YYYY-MM`, RFC3339, plus `2006-01-02T15:04:05` and `2006-01-02 15:04:05`. Anything else is a parse error.

See also: [Frontmatter Model](/guide/frontmatter-model/) for which fields queries match against, and [Search & Ranking](/guide/search-ranking/) for how filters combine with fuzzy scoring.
