# mdfu

Fuzzy finder for Markdown notes — search body text plus normalized frontmatter (OKF v0.1/v0.2 and Portent/Tolaria) from one fast static binary, interactively (TUI) or pipeably (`--filter`, fzf-compatible).

All features are implemented and merged: `--filter` runs the real
`scan → parse → query → search → output` pipeline (exit `0` on match,
`1` on no match, `2` on usage error) and the TUI filters live.
Every example below was verified against the built binary.

## Install

Requires Go 1.25+.

```sh
# Latest release binary into $GOBIN
go install github.com/anomalyco/mdfu/cmd/mdfu@latest

# Or build from source
git clone https://github.com/anomalyco/mdfu
cd mdfu
go build -o mdfu ./cmd/mdfu
```

## Quickstart

The repo ships fixtures under `testdata/` covering every supported frontmatter family:

```sh
go build -o mdfu ./cmd/mdfu

# Interactive picker over the fixtures
./mdfu --root testdata

# Non-interactive, fzf-compatible (all live-verified; exit 0 match / 1 no-match)
./mdfu --root testdata --filter "type:Task"
./mdfu --root testdata --filter "tag:launch"
./mdfu --root testdata --filter "status:Draft"
./mdfu --root testdata --filter "kumquat zebra"
```

| Fixture | Family | What's inside |
|---|---|---|
| `testdata/portent-task.md` | Portent (`type: Task`) | `Ship mdfu MVP`, tags `tolaria, launch, [[Project Atlas]]`, `status: Draft`, `created: 2026-01` |
| `testdata/okf-v02-metric.md` | OKF v0.2 (`type: Metric`) | `Monthly Active Users`, `generated.at` / `verified` attestations, `sources[]` |
| `testdata/okf-v01-legacy.md` | OKF v0.1 (`type: Claim`) | H1-derived content, legacy `timestamp: 2023-11-15`, comma-string tags |
| `testdata/generic.md` | Generic (no `type`) | Custom keys (`author: bob`, `views: 42`) |
| `testdata/nofrontmatter.md` | None | Bare `# Lone Note` + body, no frontmatter |
| `testdata/bad-yaml.md` | Broken YAML | Malformed frontmatter — body (`kumquat zebra xylophone`) stays searchable |

## Query syntax

One box: bare words fuzzy-match, `key:value` tokens hard-filter. Tokens combine with **AND**.
Keys are case-insensitive; values preserve case. Quote multi-word values: `title:"Monthly Active"`.
`|` alternation is **not** supported in v1 (reserved for later).

| Token | Meaning | Example |
|---|---|---|
| `word …` | Bare words, AND-combined, fuzzy-matched against the SearchBlob (title + description + tags + flattened frontmatter + body). Title substring hits score +100; ties break by most-recent `UpdatedAt`, then path. No bare words → results sorted by path. | `kumquat zebra` |
| `tag:v` / `tags:a,b` | Include tag(s), case-insensitive. `tags:` splits on commas. | `tag:launch`, `tags:retention,cohort` |
| `-v` / `!v` inside a tag value | Negate a tag. Only tag values support negation. | `tag:-launch`, `tags:a,-b,!c` |
| `type:V` | Normalized `type`, case-insensitive exact. | `type:Task` |
| `title:text` | Fuzzy-contains (case-insensitive) against the title. Repeatable — multiple `title:` tokens join with a space. | `title:Monthly`, `title:"Monthly Active"` |
| `path:sub` | Case-insensitive substring of the file path. | `path:portent` |
| `status:V` | Case-insensitive exact against lowercased `status`. `status:archived` also matches `archived: true` via `IsArchived()`. | `status:Draft` |
| `organized:V` | Generic boolean filter over the normalized `organized` flag (`true`/`false`; unset counts as `false`). | `organized:true` |
| `archived:V` | Generic boolean filter over `IsArchived()` (`archived: true` **or** `status: archived`). | `archived:true`, `archived:false` |
| `created:D`, `updated:D` | Date filter on `CreatedAt` / `UpdatedAt`. See date forms below. | `created:2024-05-01` |
| `date:D` | Matches if **either** `CreatedAt` or `UpdatedAt` satisfies `D`. | `date:2023-11-15` |
| `before:D` / `after:D` | Exclusive upper / lower bound on **either** date (shorthand for `<D` / `>D` with either-semantics). | `before:2024-01-01`, `after:2024-01-01` |
| `key:value` (any other key) | Generic filter: looks up `key` in Raw frontmatter case-insensitively (underscores ignored), then in normalized fields (`description`, `resource`, `role`, `tags`, `body`, `format`, `belongs_to`/`related_to`, dates, …). Value fuzzy-matches. Unknown key → no match. | `author:bob`, `views:42` |

Date value forms (`D`):

| Form | Meaning |
|---|---|
| `2024-03-15` | Exact day (whole day, inclusive). `2024-01` matches the whole month; RFC3339 (`2024-03-15T10:00:00Z`) matches the instant. |
| `A..B` | Inclusive range. Either end may be empty (`2024-01-01..`, `..2024-12-31`). |
| `>=D` / `<=D` | Inclusive lower / upper bound (day/month granularity expands to start/end of period). |
| `>D` / `<D` | Exclusive lower / upper bound (expands past end/start of period). |

Supported layouts: `YYYY-MM-DD`, `YYYY-MM`, RFC3339 (`2006-01-02T15:04:05Z07:00`),
plus `2006-01-02T15:04:05` and `2006-01-02 15:04:05` (no timezone). Anything else is a parse error.

**Archived-hidden-by-default:** documents with `archived: true` or `status: archived`
are hidden in the TUI unless toggled visible (`ctrl+a`), but stay searchable —
`archived:true` / `status:archived` still match them. (`Rank` itself never hides;
the TUI/`FilterArchived` layer applies the hiding.)

## CLI reference

```sh
mdfu [--root DIR] [--hidden] [--no-ignore] [--limit N]
       [--filter QUERY] [--format paths|json|vimgrep]
```

| Flag | Default | Meaning |
|---|---|---|
| `--root DIR` | `.` | Root directory to scan for `*.md` (case-insensitive extension, sorted; `.git` always skipped; symlinks not followed; hidden files/dirs skipped unless `--hidden`). |
| `--hidden` | off | Include hidden files and directories. |
| `--no-ignore` | off | Disable gitignore respect. |
| `--limit N` | `50` | Max number of results (`0`/negative = unlimited where applied). |
| `--filter QUERY` | — | Non-interactive mode: print matches for QUERY and exit. Omit for the interactive TUI. |
| `--format F` | `paths` | `paths` (one path per line, fzf-compatible), `json` (indented `[{path,score,title,type,snippet}]`), `vimgrep` (`path:1:1:title` quickfix lines). Unknown format → exit `2`. |

Exit codes: `0` = at least one match, `1` = no match, `2` = usage error (e.g. bad `--format`).

### Examples (against `testdata/`)

Match sets verified live against the built binary
(`go build -o mdfu ./cmd/mdfu`; exit `0` on match, `1` on no match):

```sh
# 1. Exact type lookup → testdata/portent-task.md
mdfu --root testdata --filter "type:Task"

# 2. Tag lookup (case-insensitive; [[wikilinks]] stripped) → testdata/portent-task.md
mdfu --root testdata --filter "tag:launch"

# 3. Lifecycle status (case-insensitive) → okf-v01-legacy.md + portent-task.md
mdfu --root testdata --filter "status:Draft"

# 4. Bare-word body search (AND-fuzzy); broken-YAML file stays searchable → bad-yaml.md
mdfu --root testdata --filter "kumquat zebra"

# 5. Generic key:value on custom frontmatter → generic.md
mdfu --root testdata --filter "author:bob"

# 6. Exact date on normalized CreatedAt → okf-v02-metric.md
mdfu --root testdata --filter "created:2024-05-01"

# 7. Combined hard filters → okf-v01-legacy.md
mdfu --root testdata --filter "type:Claim tag:retention"

# 8. Tag negation (everything except the launch-tagged task)
mdfu --root testdata --filter "tag:-launch"

# Same queries, other formats (formatters live: internal/output)
mdfu --root testdata --filter "status:Draft" --format json
mdfu --root testdata --filter "status:Draft" --format vimgrep

# Cap the list
mdfu --root testdata --filter "status:Draft" --limit 1
```

## TUI

Launch by omitting `--filter`:

```sh
mdfu [--root DIR] [--hidden] [--limit N]
```

Type to narrow (bare words fuzzy, `key:value` hard-filters via the live
`FilterFunc`: scan → parse → query → rank on every keystroke).
`Enter` prints the selection (pipeable to editors/fzf-style flows).

Keybindings (from `internal/tui/tui.go`, `Model.Update`):

| Key | Action |
|---|---|
| `up` / `ctrl+k` | Move cursor up (wraps to bottom). |
| `down` / `ctrl+j` | Move cursor down (wraps to top). |
| `enter` | Confirm: multi-selection if any, else the cursor item. |
| `esc` / `ctrl+c` | Abort (no output, `Run` returns `nil, nil`). |
| `tab` | Toggle multi-select on the cursor item. |
| `ctrl+a` | Toggle archived visibility (default: hidden). |
| `ctrl+p` | Toggle the preview pane (first 30 body lines + Title/Path/Type/Tags/Status; side-by-side when ≥100 columns). |
| any other key | Edits the query and refilters. |

The status bar shows `matched/total • archived:hidden|shown • tab:multi • enter:select`.

## Frontmatter support

All formats normalize into one `model.Document` (`internal/model` contract):

- **Families:** OKF v0.1 (`type` + legacy `timestamp`), OKF v0.2 (`generated.at`,
  `verified{by,at}`, `sources[{id,resource,title,author}]`, `description`),
  Portent/Tolaria (`type: Task|Project|Operation|Responsibility|Event|Note|Topic|Person`,
  `organized`/`archived`, `belongs_to`/`related_to`, `resource`), Generic (frontmatter
  without `type`), None (no frontmatter). Detection: a Portent `type` (or bare
  `organized`/`archived` keys) → `portent`; any other `type` → `okf`; frontmatter
  without `type` → `generic`; none → `none`.
- **Title:** `title` → else first `# H1` → else filename without extension.
- **Type:** verbatim `type` (any case preserved; matching folds case).
- **Tags:** YAML list, single string, or comma-separated string; surrounding
  `[[wikilink]]` markers stripped (`[[Project Atlas]]` → `Project Atlas`).
- **Dates:** `CreatedAt` ← first of `created, date, timestamp, generated.at`;
  `UpdatedAt` ← first of `generated.at, timestamp, last_modified, updated, modified`.
  Layouts: RFC3339, `2006-01-02`, `2006-01`, datetimes without timezone, unix timestamps.
- **Lifecycle:** `status` lowercased; `organized`/`archived` coerced from bool/string/number.
  `IsArchived()` = `archived: true` **or** `status: archived`.
- **Relationships:** `belongs_to`/`belongs-to` → `BelongsTo`, `related_to`/`related-to` →
  `RelatedTo` (string or list, wikilinks stripped); `resource`; `sources[]`;
  `generated`/`verified` `{by, at}` attestations. `index.md`/`log.md` basenames set
  `Role: index|log`.
- **SearchBlob:** `title + description + tags + flattened scalars of Raw + body`
  (lowercased at match time). Bare-word search hits every scalar, including nested maps/lists.
- **Resilience:** only a leading `---` block counts as frontmatter; malformed YAML never
  fails hard — the error is recorded on `Document.ParseError` and the file (filename/H1
  title + body) stays searchable (`testdata/bad-yaml.md`).

## Screenshot

Real headless capture of the picker (`tui.NewModel(...).View()` with four
`testdata` titles plus one archived doc, preview on, archived hidden —
see `docs/screenshot.txt`):

```text
> Search... (bare words fuzzy, key:value hard-fil…
> [ ] Ship mdfu MVP [Task]  testdata/portent-task.md
  [ ] Monthly Active Users [Metric]  testdata/okf-v02-metric.md
  [ ] Legacy Retention Claim [Claim]  testdata/okf-v01-legacy.md
  [ ] Generic Note  testdata/generic.md
Preview
Title: Ship mdfu MVP
Path: testdata/portent-task.md
Type: Task
Tags: tolaria, launch, Project Atlas
Status: draft
---
Finish the parser track so the search track can rank fixtures.
4/5 • archived:hidden • tab:multi • enter:select
```

Regenerate it headlessly (throwaway program, deleted afterwards):

```sh
# Build a tui.Model from model.Document literals (+ real testdata titles),
# call View(), save to docs/screenshot.txt — no TTY needed.
cat docs/screenshot.txt
```

## Project layout

```text
cmd/mdfu/main.go      CLI flags (--root/--hidden/--no-ignore/--limit/--filter/--format)
cmd/mdfu/filter.go    --filter pipeline (scan→parse→query→rank→output)
internal/model/         Unified Document contract + SearchBlob (shared by all tracks)
internal/scan/          WalkDir: *.md discovery, hidden/symlink/.git handling
internal/parse/         Frontmatter split + YAML + normalizers → Document
internal/query/         Tokenizer + Query AST + date-range parser
internal/search/        Hard-filter (MatchesDoc) + fuzzy Rank + FilterArchived
internal/tui/           BubbleTea picker (keybindings, preview, archived toggle)
internal/output/        paths / json / vimgrep formatters + Snippet
testdata/               okf-v01, okf-v02, portent, generic, no-frontmatter, bad-yaml fixtures
docs/screenshot.txt     Headless TUI capture (this track)
```

How to run tests:

```sh
go build ./...
go test ./...
```

## Verification status

- Live-verified against the built binary (post Track F merge): all 8 `--filter`
  examples above plus `--format json|vimgrep`, `--limit 1`, invalid
  `--format bogus` → exit `2`, no-match → exit `1`; `go vet`/`go test ./...` green
  (unit + `tests/` functional/regression suites).
- `docs/screenshot.txt` is a genuine `Model.View()` render, not hand-drawn.
