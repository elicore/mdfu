# Rust Port: Parity and Exit-Code Contract Matrices

Frozen Go baseline SHA: `0d1c2d97463697fe66ca638977cf0e5709a66020` (recorded via `git rev-parse HEAD` at execution start).

This document is the parity contract for the Go-to-Rust port. It is written
before any Rust code exists. It freezes the observable behavior of the Go tree
at the baseline SHA, inventories every Go test file as the executable spec, and
records the exit-code and error-string contract from `SPEC.md` and the CLI.

Nothing here is Rust-specific: it names what the port must keep.

## Classification legend

- **normative**: observable behavior must match. The Rust port must produce the
  same externally visible result (output bytes, exit codes, discovered files,
  ranking order, error strings) for the same inputs.
- **relaxed**: behavior must match, but the test asserts an exact
  string/SGR/box/measurement produced by a Go library (lipgloss, yaml.v3) or a
  Go-specific representation. The named relaxation is what the Rust port is
  free to change; the behavior behind it stays normative.
- **dropped**: Go-library-internal or Go-toolchain-internal test with no Rust
  analog. The test is not translated; the behavior it indirectly protected is
  covered elsewhere.

Every file under `internal/`, `cmd/mdfu/`, and `tests/` that matches
`*_test.go` gets exactly one row. The inventory below is exhaustive at the
baseline SHA.

## Test-file inventory (41 files)

- `cmd/mdfu/main_test.go` : **normative**. Covers theme load, positional query joining, `--config default`, filter `--limit` defaults, and flag reordering; the root CLI flag surface and its limits are the contract.
- `cmd/mdfu/pipeline_test.go` : **normative**. Covers the scan to parse to query pipeline for body, tag, archived, bad-YAML, and reload cases.
- `cmd/mdfu/task_test.go` : **normative**. Covers the `task`/`tasks` router and non-TTY `tasks` parity with `task list`; the only escape assertion is the absence of the `ESC[?1049h` alternate-screen sequence, not exact SGR.
- `internal/open/open_test.go` : **normative**. Covers rejection of non-web URL schemes before launching a browser.
- `internal/output/output_test.go` : **normative**. Covers `paths`, `json`, and `vimgrep` byte output plus the snippet window.
- `internal/parse/parse_test.go` : **normative**. Covers OKF v0.1/v0.2 and Portent frontmatter split, normalization, role, archived, title fallback.
- `internal/query/query_test.go` : **normative**. Covers the `key:value` grammar, tag negation, quotes, generic keys, and date/range/comparison filters.
- `internal/scan/scan_test.go` : **normative**. Covers `WalkMarkdown` discovery rules (extension, hidden, git, symlinks, sort).
- `internal/search/search_test.go` : **normative**. Covers hard filters, scoring, title boost, tie-break, and archived filtering.
- `internal/task/archive_test.go` : **normative**. Covers the archive append-then-remove write and the E19 outside-base precondition.
- `internal/task/args_test.go` : **dropped**. Tests `ReorderInterspersed`, a shim that works around Go's stdlib `flag` stopping at the first positional; the `flag` package has no Rust analog and clap handles interspersed flags natively, while the observable contract (flags may precede or follow the query, `docs/reference/cli.md:24`) is re-asserted by `cmd/mdfu/main_test.go` and the functional tests.
- `internal/task/blocker_test.go` : **normative**. Covers blocker detection and the blocked-warning rendering.
- `internal/task/body_test.go` : **normative**. Covers fence masking and body collection, including the fenced-code-block header rule (SPEC §L).
- `internal/task/checkbox_test.go` : **normative**. Covers the broad checkbox grammar (`ParseCheckbox`, `LoadCheckboxes`, `FlipCheckbox`) and its separation from the strict task scope.
- `internal/task/commands_test.go` : **normative**. Covers dispatch, the D1 `mdfu task:` prefix, the §I exit-code table, and the subcommand registry.
- `internal/task/config_test.go` : **normative**. Covers rc discovery, load, error paths (E5), and base-path precedence.
- `internal/task/discover_test.go` : **normative**. Covers walking, filtering, and `LoadAll` discovery.
- `internal/task/edit_test.go` : **normative**. Covers the temp-file plus rename round trip, stale detection (E17), and block append.
- `internal/task/fixtures_test.go` : **normative**. Covers golden fixture parsing and identified-ID extraction.
- `internal/task/format_test.go` : **normative**. Covers header grammar, metadata splitting, token parsing, render, and round trip, including that uppercase `X` and indented/plain checkboxes are not tasks.
- `internal/task/glob_test.go` : **normative**. Covers glob matching used by scope resolution.
- `internal/task/ids_test.go` : **normative**. Covers the `ids` command, the E13 invalid-prefix path, and its output.
- `internal/task/id_test.go` : **normative**. Covers ID resolution, prefix validation, duplicate numeric parts, padding, and the all-or-nothing assign plan.
- `internal/task/install_skills_test.go` : **normative**. Covers install-skills write/overwrite, its exit codes, and the embedded-asset guard that forbids foreign (`mdtask*`) skill names (D3); the guard carries over as a Rust source/asset test.
- `internal/task/list_test.go` : **relaxed**. Relaxation: exact Unicode box-drawing frame glyphs (`U+256D`, `U+2500`, `U+252C`, `U+2502`) and column padding width. The §D row content, ordering, and the no-ANSI-when-piped rule remain normative.
- `internal/task/move_test.go` : **normative**. Covers moving a task block and the stale-source path.
- `internal/task/open_test.go` : **normative**. Covers `open` editor invocation (E8) and post-edit behavior.
- `internal/task/render_test.go` : **relaxed**. Relaxation: exact box-drawing table bytes and the presence of any SGR sequence. The §D/§E table content, JSON shape, sort priority, and notes remain normative.
- `internal/task/set_test.go` : **normative**. Covers metadata set and the E14/E15 missing-argument errors.
- `internal/task/validate_test.go` : **normative**. Covers validate diagnostics, duplicate-ID errors, and warnings-only exit 0.
- `internal/task/view_test.go` : **relaxed**. Relaxation: the exact `ESC[38;5;245m`/`ESC[0m` SGR pair around the `file:line` prefix. View layout and content remain normative.
- `internal/theme/theme_test.go` : **relaxed**. Relaxation: `lipgloss.Style`/`lipgloss.TerminalColor` equality and `yaml.v3` unmarshal fingerprints. The underlying behavior (builtin defaults, YAML merge and override, XDG discovery, and the `HighlightSGR`/`LinkSGR` strings) remains normative and is re-asserted against the Rust color/style types.
- `internal/tui/components_test.go` : **relaxed**. Relaxation: exact lipgloss SGR bytes for theme styles (for example a custom `HighlightSGR`) and `lipgloss.Width`/`Height` measurements. Preview visibility, frontmatter panel, and OSC 8 behavior remain normative.
- `internal/tui/frontmatter_test.go` : **relaxed**. Relaxation: exact pill and highlight SGR bytes and lipgloss width. Row ordering, JSON flattening, nested dotted keys, and raw-value mapping remain normative.
- `internal/tui/preview_json_test.go` : **relaxed**. Relaxation: exact pill SGR bytes. The JSON metadata rows and border presence remain normative.
- `internal/tui/tasks_checkbox_test.go` : **normative**. Covers broad-checkbox task-model behavior (toggle, edit title/body, move, archive, duplicate keys); it only strips ANSI, never asserts exact SGR.
- `internal/tui/tasks_run_test.go` : **normative**. Covers the injected task filter constructor and the abort result; the `SetFilter|globalFilter` source grep is an architecture guard that carries over as a Rust source test.
- `internal/tui/tasks_test.go` : **relaxed**. Relaxation: exact `TaskBlocker`/`TaskDone` SGR bytes and lipgloss frame width/height. Toggle, edit, move, archive, and list behavior remain normative.
- `internal/tui/tui_test.go` : **relaxed**. Relaxation: exact highlight SGR, OSC 8 escape bytes, and lipgloss width/height bounds. Navigation, selection, filter modes, key bindings, and editor reload behavior remain normative.
- `tests/functional_test.go` : **normative**. End-to-end query, filter, ranking, and output contract against `testdata/`.
- `tests/regression_test.go` : **normative**. End-to-end regression contract (bad YAML, no frontmatter, roles, legacy timestamps, Portent tags, wikilinks, hidden/.git, empty vault, date layouts, false positives).

## Exit-code matrix

The root CLI and the task engine share the 0/1/2 vocabulary from `SPEC.md` §I.

| Exit Code | Condition | Source (file:line) |
| --- | --- | --- |
| 0 | Root `-h`/`--help` (flag parse returns `ErrHelp`) | `cmd/mdfu/main.go:61-62` |
| 0 | `--version` prints `mdfu <version>` | `cmd/mdfu/main.go:67-70` |
| 0 | `--config default` prints the builtin config YAML | `cmd/mdfu/main.go:77-80` |
| 0 | Filter mode produced at least one result | `cmd/mdfu/filter.go:40-41` |
| 0 | Interactive TUI completed and printed selected paths | `cmd/mdfu/main.go:159-164` |
| 0 | Task success, including `--sort` with an unrecognised value and `validate` with warnings only | `internal/task/SPEC.md:494` |
| 1 | Filter mode: zero results | `cmd/mdfu/filter.go:20-22` |
| 1 | Filter mode: scan/parse/query error | `cmd/mdfu/filter.go:16-19` |
| 1 | Filter mode: JSON formatting error | `cmd/mdfu/filter.go:31-34` |
| 1 | TUI: `loadDocuments` error | `cmd/mdfu/main.go:135-138` |
| 1 | TUI: `tui.Run` returned an error | `cmd/mdfu/main.go:155-158` |
| 1 | Any task, ID-resolution, IO, configuration, stale-file, archive, or validate-error condition; also an unknown subcommand reached via the explicit `mdfu task --` path | `internal/task/SPEC.md:495` |
| 1 | `fail()` maps a `*StaleError` or any ordinary error to 1 and writes nothing to stdout | `internal/task/commands.go:124-128` |
| 2 | Root flag parse error: unknown flag or a flag missing its value | `cmd/mdfu/main.go:60-65` |
| 2 | Invalid `--format` value (not `paths`/`json`/`vimgrep`) | `cmd/mdfu/main.go:72-75` |
| 2 | Theme resolve/load error | `cmd/mdfu/main.go:94-98` |
| 2 | Task usage error: unknown flag, flag missing its value, missing required argument, unexpected positional, or a root flag before the task subcommand | `internal/task/SPEC.md:496` |
| 2 | Root flag before the `task`/`tasks` candidate (E2) | `cmd/mdfu/task.go:195-198` |
| 2 | Unexpected positional in `mdfu tasks` (E3) | `cmd/mdfu/task.go:63-66` |
| 2 | Task subcommand flag parse error (`parseFlags`) | `internal/task/commands.go:250-257` |

## Error-string to exit-code mapping (E1 to E19, SPEC.md §J)

| ID | String (abridged) | Exit Code | Source |
| --- | --- | --- | --- |
| E1 | `unknown command '<cmd>'` | 1 | `internal/task/commands.go:101-103`, `internal/task/SPEC.md:495` |
| E2 | `--<flag> must come after the task subcommand` | 2 | `cmd/mdfu/task.go:195-198` |
| E3 | `mdfu tasks: unexpected argument '<arg>'` | 2 | `cmd/mdfu/task.go:63-66` |
| E4 | `--path '<x>' does not exist or is not a file/directory` | 1 | `internal/task/commands.go:177-181`, `internal/task/commands_test.go:154-160` |
| E5 | `cannot read <path>: <err>` | 1 | `internal/task/SPEC.md:495` (IO/config), `internal/task/config.go:61` |
| E6 | `Invalid JSON in <path>: <message>` | 1 | `internal/task/SPEC.md:495` (parse/config) |
| E7 | `skipping <dir>: <err>` (warning, exit code unaffected) | 0 | `internal/task/SPEC.md:558` |
| E8 | `$EDITOR is not set` | 1 | `internal/task/SPEC.md:495` (runtime), `internal/task/open.go:18-31` |
| E9 | `invalid task ID '<input>'` | 1 | `internal/task/SPEC.md:495` (ID resolution) |
| E10 | `task '<id>' not found` | 1 | `internal/task/SPEC.md:495` (task) |
| E11 | `ambiguous numeric ID '<n>' matches: <id1>, <id2>` | 1 | `internal/task/SPEC.md:495` (ID resolution) |
| E12 | `task '<id>' appears multiple times; expected exactly one match` | 1 | `internal/task/SPEC.md:495` (task) |
| E13 | `invalid prefix '<prefix>'` | 1 | `internal/task/ids.go:21,44`, `internal/task/ids_test.go:105` |
| E14 | `no task IDs provided` | 1 | `internal/task/set.go:21`, `internal/task/set_test.go:85-93` |
| E15 | `no metadata tokens provided` | 1 | `internal/task/set.go:24`, `internal/task/set_test.go:101-109` |
| E16 | `<relpath>:<line>: no prefix found for task ...` | 1 | `internal/task/SPEC.md:495` (ID resolution) |
| E17 | `file changed, task '<id>' not at expected line` | 1 | `internal/task/SPEC.md:495` (stale file), `internal/task/commands.go:124-128` |
| E18 | `task '<id>' is not done` | 1 | `internal/task/SPEC.md:495` (task) |
| E19 | `archive path '<path>' is outside the base directory` | 1 | `internal/task/SPEC.md:495` (archive), `internal/task/archive_test.go` |

Note: E14 and E15 are reconstructed as engine failures through `fail()`, so they
exit 1, not 2, even though they read as usage conditions. `SPEC.md` §I still
lists a missing required argument as a usage error for flags parsed at the
subcommand layer (exit 2); the metadata-token cases are validated inside the
`set` handler after the flags parse, so `fail()` wins.

## Documented divergences carried over (SPEC.md §K)

The port must keep exactly these five divergences, no more: D1 (`mdfu task: `
stderr prefix), D2 (unknown token after `mdfu task` is a fuzzy search), D3
(install-skills writes mdfu-named skills), D4 (mdfu-branded help/version), D5
(list note sentences name `mdfu task`).

## Verification

- Baseline: `git rev-parse HEAD` = `0d1c2d97463697fe66ca638977cf0e5709a66020`.
- Test files found by `find . -name '*_test.go' | wc -l` = 41.
- Inventory rows (lines starting with `- `) must be at least 41; this document
  has one row per test file.
- Classifications: 31 normative, 9 relaxed, 1 dropped.
- Every relaxed row names its exact relaxation; no task-format test is marked
  relaxed without one.
- Checker concept (`.omo/evidence/task-1-check.sh`): fail if any inventory row
  is missing `normative`, `relaxed`, or `dropped`, and fail if the row count is
  below the test-file count.

## Differential parity harness (todo 32)

`scripts/parity.sh` builds the Go binary from the frozen baseline SHA above
(via `git archive`, so the Go tree is never modified), builds the Rust release
binary, and runs `rust/crates/mdfu-cli/tests/parity.rs` against both binaries.
`make parity` is the target; `scripts/parity.sh --mutate` appends one stdout
byte inside a Go-binary shim and asserts the harness reports `PARITY-DIFF`.

Covered surface, all byte-exact (stdout, stderr, exit code, and the vault files
the command rewrote):

- Note filter over `testdata/` and a synthetic notes vault:
  `--format paths|json|vimgrep`; queries `kumquat zebra`, `kumquat`,
  `type:Metric`, `tag:retention`, `tags:growth,kpi`, `tag:-kpi`,
  `status:verified`, `created:A..B`, `bold` (HTML escaping), `durian`
  (filename fallback); flags `--archived`, `--hidden`, `--no-ignore`,
  `--limit 0/3/-1`; the zero-result, invalid-date, and invalid-format exit
  rows.
- Task subcommands in non-TTY mode over `testdata/tasks/` and a synthetic
  vault (`.mdfurc` exclude rules, `_archive.md`, seed and duplicate IDs):
  `list` (+ `--all`, `--blocked`, `--tag`, `--priority`, `--sort`, `--json`),
  `view` (+ `--json`), `set`, `ids`, `archive`, `validate`, and `mdfu tasks`.

Coverage record: 33 note-filter cases + 33 task cases (66 differential cases)
in 6 tests; the separate tests prove the score normalization is the only raw
note-JSON difference, that the comparator detects a one-byte tamper in each
compared dimension, that the missing-root divergence is recorded here, and
that this document keeps the exclusion list above. Evidence:
`.omo/evidence/task-32-rust-port-ratatui.diff`.

### Non-testable surfaces (exact list)

The harness deliberately does not compare the following; they need a real
terminal or assert Go-library-internal bytes with no Rust analog, and remain
covered only by the relaxed rows in the inventory above:

- **TTY box bytes**: the exact Unicode box-drawing glyphs and column padding of
  the `task list`/`tasks` table when attached to a TTY (`internal/task/list_test.go`
  and `render_test.go` relaxations). Non-TTY output contains no box frame, and
  the §D row content/order and the no-ANSI-when-piped rule stay normative.
- **SGR-exact output**: every exact ANSI SGR sequence (the `view` `file:line`
  prefix `ESC[38;5;245m`, task done/tag/priority colors, theme and lipgloss
  styles). Non-TTY output contains no SGR; the harness compares those bytes
  instead.
- **Interactive TUI**: the note picker and task TUI event loops, alternate
  screen, key handling, and rendered frames. Running either TUI without a TTY
  fails before any comparable output (both binaries try `/dev/tty`), so the
  harness never launches it.

Everything else on the covered surface is compared raw.

### Harness normalization (complete list)

Exactly one normalization is applied, to note-filter `--format json` stdout:

- `"score": 0,` (Go `encoding/json`: float64 zero renders as `0`) is compared
  as `"score": 0.0,` (Rust `serde_json` renders `0.0`). Source:
  `cmd/mdfu/pipeline.go` and `rust/crates/mdfu-cli/src/pipeline.rs` always set
  `Score: 0`; the value is constant and only its float rendering differs.
  `parity_note_json_score_normalization_is_the_only_difference` proves the raw
  outputs differ *only* on lines containing `"score"`.

Task JSON needs no normalization: `task list --json` and `task view --json`
are byte-identical including the trailing newline.

### Known output divergences (recorded, not normalized)

- `--limit -1` (space form): Go's `flag` package accepts the negative value;
  clap treats `-1` as an unexpected argument and exits 2. `--limit=-1` is
  byte-identical in both. This extends the task-28 root flag-parsing
  divergence; the harness uses the `=` form.
- Missing scan root stderr: Go `mdfu: lstat <root>: no such file or directory`
  vs Rust `mdfu: <root>: IO error for operation on <root>: No such file or
  directory (os error 2)`. Exit 1, empty stdout, and the `mdfu: ` prefix are
  identical; only the OS-error sentence differs (`os.Lstat` vs the `ignore`
  crate). Asserted by `parity_missing_root_is_a_documented_divergence`.
- CLI usage-error wording, help stream, and single-dash long flags (clap vs Go
  `flag`), recorded in task 28. The harness only covers the custom
  `--format` usage message, which is byte-identical.
