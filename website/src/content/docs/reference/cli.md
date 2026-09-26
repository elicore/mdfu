---
title: CLI
description: Flags, output formats, exit codes, and testdata examples.
---

```sh
mdfu [QUERY...] [--root DIR] [--hidden] [--no-ignore] [--limit N] [--archived]
       [--no-hyperlinks] [--config PATH|default] [--format paths|json|vimgrep] [--version]
```

| Flag | Default | Meaning |
|---|---|---|
| `QUERY...` (positional) | — | Non-interactive filter query; any positional argument selects filter mode. Multiple words are joined with spaces. Omit for the TUI. Query language: [Query Syntax](/guide/query-syntax/). |
| `--root DIR` | `.` | Root to scan for `*.md` (see [Scan](/guide/scan/)). |
| `--hidden` | off | Include hidden files/dirs. |
| `--no-ignore` | off | Intended to disable gitignore respect — currently no-op, see [Scan](/guide/scan/). |
| `--limit N` | `20` filter / `50` TUI | Max results; `0`/negative = unlimited. Filter mode defaults to `20`, the interactive TUI to `50`. |
| `--format F` | `paths` | `paths` (one path/line, fzf-compatible), `json` (`[{path,score,title,type,snippet}]` indented), `vimgrep` (`path:1:1:title`). Unknown → exit `2`. |
| `--archived` | off | Include archived docs (else `FilterArchived` hides them; they stay searchable). |
| `--no-hyperlinks` | off | TUI only: render markdown links as `label url` instead of OSC 8 terminal hyperlinks (use on terminals without hyperlink support). See [TUI](/guide/tui/#links). |
| `--config PATH` | `$XDG_CONFIG_HOME/mdfu/config.yaml` | TUI only: theme/config file; overrides `$MDFU_CONFIG` and the discovered XDG file. Pass `default` to print the builtin default config to stdout and exit. See [Configuration](/guide/configuration/). |
| `--version` | — | Print version (`-ldflags "-X main.version=…"`) and exit. |

Flags may appear before or after the query (`mdfu "status:Draft" --limit 5`); `--` ends flag parsing so a query can begin with a dash.

Exit codes: `0` = ≥1 match, `1` = no match, `2` = usage error (e.g. bad `--format`).

## Task commands

Two additional entry points read and edit checkbox tasks in your Markdown
files. See [Tasks](/guide/tasks/) for the file format and full command
reference, and [Tasks TUI](/guide/tasks-tui/) for the interactive browser.

| Command | Synopsis | Meaning |
|---|---|---|
| `mdfu task [command] [args…]` | `mdfu task list` | Non-interactive task CLI. Nine subcommands: `list`, `view`/`show`, `open`, `move`, `set`, `ids`, `archive`, `validate`, `install-skills`. Bare `mdfu task` lists, and `mdfu task <id>` views (an ID is `PREFIX-<n>` or a bare number). |
| `mdfu tasks [selection…]` | — | Interactive tasks TUI (a TTY is required; non-TTY falls back to `mdfu task list`). Accepts the same selection flags as `list`: `--all`, `--blocked`, `--sort`, repeatable `--tag`/`--priority`, and positional `#tag`/`!priority`. |

Both commands are intercepted before the root flagset is parsed. Before the
`task`/`tasks` candidate only `--config <path>` is accepted (it names the theme
for the TUI); any other root flag written before the subcommand is
`mdfu task: --<flag> must come after the task subcommand` at exit 2. A root
value flag (`--root`, `--limit`, `--format`) followed by a value is skipped so
`mdfu --root V task list` still reaches its candidate. An unknown token after
`mdfu task` is a fuzzy search rather than an error (divergence D2).

A bare `--` before the candidate bypasses the router entirely, so
`mdfu -- task` is the escape hatch that performs a normal fuzzy search for
`task`. Inside the task surface, `mdfu task -- <command>` is the explicit
dispatch marker: it reaches the task command table directly (and prints the
branded usage block for an unknown command, exit 1).

## Examples (against `testdata/`)

```sh
mdfu --root testdata "type:Task"              # → portent-task.md
mdfu --root testdata "tag:launch"             # → portent-task.md
mdfu --root testdata "status:Draft"           # → okf-v01-legacy.md + portent-task.md
mdfu --root testdata "kumquat zebra"          # → bad-yaml.md (broken YAML stays searchable)
mdfu --root testdata "author:bob"             # → generic.md (custom frontmatter)
mdfu --root testdata "created:2024-05-01"     # → okf-v02-metric.md
mdfu --root testdata "type:Claim tag:retention"
mdfu --root testdata "tag:-launch"
mdfu --root testdata "status:Draft" --format json
mdfu --root testdata "status:Draft" --format vimgrep
mdfu --root testdata "status:Draft" --limit 1
mdfu --config default                          # print the builtin default config
```

Fixtures: `portent-task.md` (Portent Task), `okf-v02-metric.md` (OKF v0.2 Metric), `okf-v01-legacy.md` (OKF v0.1 Claim), `generic.md` (`author:bob`), `nofrontmatter.md`, `bad-yaml.md`.

See also: [TUI](/guide/tui/) for interactive mode, and [Query Syntax](/guide/query-syntax/) for the filter language.
