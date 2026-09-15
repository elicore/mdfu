---
title: Frontmatter Model
description: Unified Document contract, format detection, and frontmatter normalization.
---

Canonical contract lives in `internal/model/document.go`. Do not change without agreement — `scan/parse/query/search/tui/output` all depend on it.

```go
type FormatKind string // FormatOKF, FormatPortent, FormatGeneric, FormatNone
type Document struct {
  Path, Title, DocType, Description, Resource, Role string // Role: ""|index|log
  Tags []string
  Body string
  CreatedAt, UpdatedAt *time.Time
  Status string
  Organized, Archived *bool
  BelongsTo, RelatedTo []string
  Sources []Source
  Generated, Verified *Attestation
  Format FormatKind
  Raw map[string]any
  SearchBlob string
  ParseError error
}
```

## Family detection (`internal/parse`)

- Portent/Tolaria `type`: `Task|Project|Operation|Responsibility|Event|Note|Topic|Person`, or bare `organized`/`archived` keys → `portent`.
- Any other `type` → `okf` (covers OKF v0.1 `type`+`timestamp` and v0.2 `generated.at`, `verified{by,at}`, `sources[{id,resource,title,author}]`, `description`).
- Frontmatter without `type` → `generic`. No frontmatter → `none`.

## Normalization

- **Title:** `title` → else first `# H1` → else filename without extension.
- **Type:** verbatim `type`, matching folds case.
- **Tags:** YAML list | single string | comma string; `[[wikilink]]` markers stripped.
- **Dates:** `CreatedAt` ← first of `created,date,timestamp,generated.at`; `UpdatedAt` ← first of `generated.at,timestamp,last_modified,updated,modified`. Layouts: RFC3339, `2006-01-02`, `2006-01`, `2006-01-02T15:04:05` (+ `15:04:05` space variant), unix timestamps.
- **Lifecycle:** `status` lowercased; `organized`/`archived` coerced from bool/string/number. `IsArchived()` = `archived:true` **or** `status:archived`.
- **Relationships:** `belongs_to`/`belongs-to` → `BelongsTo`, `related_to`/`related-to` → `RelatedTo` (string or list, wikilinks stripped); `resource`; `sources[]`; `generated`/`verified {by,at}`. `index.md`/`log.md` basenames set `Role: index|log` (deprioritized in ranking, never dropped).
- **SearchBlob:** `title + description + tags + flattened scalars of Raw + body` (`BuildSearchBlob`, `FlattenScalars` sorted-key, recursion into nested maps/lists, keys/nils skipped). Stored as-is; lowercased at match time.
- **Resilience:** only a leading `---` block counts (`SplitFrontmatter`); malformed YAML never fails hard — recorded on `Document.ParseError`, file stays searchable via filename/H1 + body (`testdata/bad-yaml.md`).

See also: [Query Syntax](/guide/query-syntax/) for how these fields are filtered, and [Search & Ranking](/guide/search-ranking/) for how `SearchBlob` is scored.
