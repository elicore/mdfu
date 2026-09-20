---
title: CLI
description: Flags, output formats, exit codes, and testdata examples.
---

```sh
mdfu [--root DIR] [--hidden] [--no-ignore] [--limit N] [--archived]
       [--no-hyperlinks] [--config PATH] [--filter QUERY] [--format paths|json|vimgrep] [--version]
```

| Flag | Default | Meaning |
|---|---|---|
| `--root DIR` | `.` | Root to scan for `*.md` (see [Scan](/guide/scan/)). |
| `--hidden` | off | Include hidden files/dirs. |
| `--no-ignore` | off | Intended to disable gitignore respect — currently no-op, see [Scan](/guide/scan/). |
| `--limit N` | `50` | Max results (`0`/negative = unlimited where applied). |
| `--filter QUERY` | — | Non-interactive mode; omit for TUI. Query language: [Query Syntax](/guide/query-syntax/). |
| `--format F` | `paths` | `paths` (one path/line, fzf-compatible), `json` (`[{path,score,title,type,snippet}]` indented), `vimgrep` (`path:1:1:title`). Unknown → exit `2`. |
| `--archived` | off | Include archived docs (else `FilterArchived` hides them; they stay searchable). |
| `--no-hyperlinks` | off | TUI only: render markdown links as `label url` instead of OSC 8 terminal hyperlinks (use on terminals without hyperlink support). See [TUI](/guide/tui/#links). |
| `--config PATH` | `$XDG_CONFIG_HOME/mdfu/config.yaml` | TUI only: theme/config file; overrides `$MDFU_CONFIG` and the discovered XDG file. See [Configuration](/guide/configuration/). |
| `--version` | — | Print version (`-ldflags "-X main.version=…"`) and exit. |

Exit codes: `0` = ≥1 match, `1` = no match, `2` = usage error (e.g. bad `--format`).

## Examples (against `testdata/`)

```sh
mdfu --root testdata --filter "type:Task"              # → portent-task.md
mdfu --root testdata --filter "tag:launch"             # → portent-task.md
mdfu --root testdata --filter "status:Draft"           # → okf-v01-legacy.md + portent-task.md
mdfu --root testdata --filter "kumquat zebra"          # → bad-yaml.md (broken YAML stays searchable)
mdfu --root testdata --filter "author:bob"             # → generic.md (custom frontmatter)
mdfu --root testdata --filter "created:2024-05-01"     # → okf-v02-metric.md
mdfu --root testdata --filter "type:Claim tag:retention"
mdfu --root testdata --filter "tag:-launch"
mdfu --root testdata --filter "status:Draft" --format json
mdfu --root testdata --filter "status:Draft" --format vimgrep
mdfu --root testdata --filter "status:Draft" --limit 1
```

Fixtures: `portent-task.md` (Portent Task), `okf-v02-metric.md` (OKF v0.2 Metric), `okf-v01-legacy.md` (OKF v0.1 Claim), `generic.md` (`author:bob`), `nofrontmatter.md`, `bad-yaml.md`.

See also: [TUI](/guide/tui/) for interactive mode, and [Query Syntax](/guide/query-syntax/) for the filter language.
