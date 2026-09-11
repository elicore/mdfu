# mdfind — Fuzzy Finder for Markdown (OKF + Portent)

## Goal
Single static Go binary `mdfind` for fuzzy-finding markdown files by body free text,
frontmatter values as free text, and attribute-specific qualifiers. Supports OKF v0.1/v0.2
and Portent/Tolaria frontmatter via a unified normalized model.

## UX decisions (locked)
- Both interactive TUI (default) + ` --filter` non-interactive mode (fzf-compatible).
- Single-box qualifier syntax: bare words fuzzy-match; `key:value` tokens hard-filter.
- Unified normalized model (not strict per-format keys).
- No persistent index: walk + parse on each run (target 5–10k files, scan <500ms, <50ms/keystroke).

## Query language
- Bare words: AND, fuzzy against SearchBlob (title+tags+flattened frontmatter+body).
- `tag:foo`, `tags:a,b` (negate with `-`/`!` prefix). Case-insensitive.
- `type:Note` (normalized exact), `title:text` (fuzzy), `path:sub/`.
- Lifecycle: `status:draft`, `organized:true`, `archived:false`. `archived:true` hidden by default in TUI, still searchable.
- Dates: `created:`, `updated:`, `date:` (either), `before:`, `after:` with values `YYYY-MM-DD`, RFC3339, `YYYY-MM`, ranges `A..B`, comparisons `>=D`, `>D`, `<=D`, `<D`.
- Generic: any other `key:value` looks up Raw frontmatter (case-insensitive) + normalized fields, fuzzy on value.
- `|` for OR within a token group (v2 if costly); `!`/`-` negation required in v1.

## Shared contract (all tracks MUST respect — lives in internal/model)
```go
type FormatKind string // FormatOKF, FormatPortent, FormatGeneric, FormatNone
type Attestation struct { By string; At *time.Time }
type Source struct { ID, Resource, Title, Author string }
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

Normalization rules:
- title ← `title` else `# H1` else filename. type ← `type` (any case).
- tags: list | single string | comma string; strip `[[ ]]`.
- CreatedAt ← first of `created,date,timestamp,generated.at`; UpdatedAt ← first of `generated.at,timestamp,last_modified,updated,modified`. Parse RFC3339, `2006-01-02`, `2006-01`, `2006-01-02T15:04:05Z07:00`.
- status: lowercase `status`; plus `organized`/`archived` bools. archived:true → hidden by default.
- belongs_to (string|list), related_to (list|string); resource; sources[].{id,resource,title,author}.
- index.md/log.md → Role set, deprioritized in ranking, never dropped.
- SearchBlob = title + description + tags + flattened scalars of Raw + body (lowercased at match time, not stored lowercased).

## Architecture
```
cmd/mdfind/main.go
internal/scan/    WalkDir, gitignore, parallel load
internal/parse/   frontmatter split + YAML + normalizers → Document
internal/model/   Document struct + SearchBlob + FormatKind (CONTRACT, edit only by agreement)
internal/query/   tokenizer + Query AST + date-range parser
internal/search/  hard-filter + fuzzy score + rank
internal/tui/     BubbleTea app
internal/output/  paths/json/vimgrep formatters
testdata/{okf,portent,generic,edge}/
```

Deps: `gopkg.in/yaml.v3`, `charmbracelet/bubbletea+bubbles+lipgloss`, `sahilm/fuzzy`, stdlib rest. No tcell-based fuzzyfinder lib.

## Track split (each = one worktree + one subagent)
- Track A (scaffold+scan): DONE (`d2c7a30`, merged). `cmd` skeleton + `internal/scan` walker.
- Track B (parse+model): DONE (`df06910`, merged). Canonical `internal/model` + `internal/parse` + fixtures.
- Track C (query+search): DONE (`6bace15`, merged). `internal/query` + `internal/search`.
- Track D (cli-output): DONE as stub (`282802d`, merged). `internal/output` formatters real; `runFilterStub` still placeholder — to be replaced by Track F.
- Track E (tui): DONE as standalone (`63afefa`, merged). `internal/tui` BubbleTea picker; live `FilterFunc` wiring pending in Track F.
- Track F (wire-up): PENDING. Replace `runFilterStub` with real scan→parse→query→rank→output pipeline; wire TUI `Run` with live filter; honor `--root/--hidden/--limit`, archived-hidden-by-default. Branch `feat/wire`.
- Track G (tests): PENDING. Library-level functional + regression suite (no TTY). Branch `feat/tests`.
- Track H (docs): PENDING. README + examples + screenshot. Branch `feat/docs`.

Integration order: A+B → C → D+E → main. Each track must `go build ./... && go test ./...` green in its worktree before merge.

## Milestones / acceptance
- M1: `go build` ok; scan finds *.md respecting gitignore; parse fixtures produce expected normalized docs.
- M2: query table tests pass (tags/type/date/negation/generic); rank smoke test title-boost.
- M3: `--filter "tag:x"` prints ranked paths; `--format json` valid.
- M4: TUI opens, live filters <50ms on 5k synthetic files, Enter prints selection.
