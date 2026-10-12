---
title: Frontmatter Model
description: Unified Document contract, format detection, and frontmatter normalization.
---

Canonical contract lives in `crates/mdfu-core/src/model.rs`. Do not change without agreement — `scan`/`parse`/`query`/`search`/`output` and the TUI all depend on it.

```rust
pub enum FormatKind { Okf, Portent, Generic, None }

pub struct Document {
    pub path: String,
    pub title: String,
    pub doc_type: String,
    pub description: String,
    pub resource: String,
    pub role: String,            // "" | "index" | "log"
    pub tags: Vec<String>,
    pub body: String,
    pub created_at: Option<String>, // RFC 3339
    pub updated_at: Option<String>, // RFC 3339
    pub status: String,
    pub organized: Option<bool>,
    pub archived: Option<bool>,
    pub belongs_to: Vec<String>,
    pub related_to: Vec<String>,
    pub sources: Vec<Source>,
    pub generated: Option<Attestation>,
    pub verified: Option<Attestation>,
    pub format: FormatKind,
    pub raw: Map<String, Value>,
    pub search_blob: String,
    pub parse_error: Option<String>,
}
```

Field names are snake_case in Rust, but every one carries a `#[serde(rename = "...")]` so the JSON wire format stays byte-compatible (`DocType`, `SearchBlob`, `ParseError`, …).

## Family detection (`crates/mdfu-core/src/parse.rs`)

- Portent/Tolaria `type`: `Task|Project|Operation|Responsibility|Event|Note|Topic|Person`, or bare `organized`/`archived` keys → `Portent`.
- Any other `type` → `Okf` (covers OKF v0.1 `type`+`timestamp` and v0.2 `generated.at`, `verified{by,at}`, `sources[{id,resource,title,author}]`, `description`).
- Frontmatter without `type` → `Generic`. No frontmatter → `None`.

## Normalization

- **Title:** `title` → else first `# H1` → else filename without extension.
- **Type:** verbatim `type`, matching folds case.
- **Tags:** YAML list | single string | comma string; `[[wikilink]]` markers stripped.
- **Dates:** `created_at` ← first of `created,date,timestamp,generated.at`; `updated_at` ← first of `generated.at,timestamp,last_modified,updated,modified`. Layouts: RFC 3339, `2006-01-02`, `2006-01`, `2006-01-02T15:04:05` (+ `15:04:05` space variant), unix timestamps.
- **Lifecycle:** `status` lowercased; `organized`/`archived` coerced from bool/string/number. `Document::is_archived()` = `archived == Some(true)` **or** `status == "archived"`.
- **Relationships:** `belongs_to`/`belongs-to` → `belongs_to`, `related_to`/`related-to` → `related_to` (string or list, wikilinks stripped); `resource`; `sources[]`; `generated`/`verified {by,at}`. `index.md`/`log.md` basenames set `role: "index" | "log"` (deprioritized in ranking, never dropped).
- **Search blob:** `title + description + tags + flattened scalars of raw + body` (`build_search_blob`, `flatten_scalars` sorted-key, recursion into nested maps/lists, keys/nils skipped). Stored as-is; lowercased at match time.
- **Resilience:** only a leading `---` block counts (`split_frontmatter`); malformed YAML never fails hard — recorded on `Document::parse_error`, file stays searchable via filename/H1 + body (`testdata/bad-yaml.md`).

See also: [Query Syntax](/guide/query-syntax/) for how these fields are filtered, and [Search & Ranking](/guide/search-ranking/) for how the search blob is scored.