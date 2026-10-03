# internal/tui — Bubble Tea TUIs

## OVERVIEW

Two separate Bubble Tea models in one package: the note picker (`tui.go`) and the task TUI (`tasks_*.go`). Charm stack: bubbletea + bubbles + lipgloss + glamour, styles from `internal/theme`.

## STRUCTURE

- `tui.go` (1.4k LOC) — note picker model; entry points `Run`, `RunWithFilter`, global `SetFilter`. Starts in list mode (search bar hidden); `/` enters search mode, search `esc` returns to list keeping the query, list-mode `esc` clears a non-empty query then a second `esc` aborts (`ctrl+c` always aborts). List keys are allowlisted. `e` opens the focused doc in `$EDITOR` via `tea.ExecProcess` then reloads it. Accessors `IsSearching()`, `Status()`.
- `tasks_model.go` / `tasks_view.go` / `tasks_actions.go` / `tasks_edit*.go` — task TUI model
- `tasks_run.go` — `RunTasks` entry; fallback to `task list` when stdout is not a TTY
- `links.go` — OSC 8 hyperlinks + web-link detection
- `frontmatter.go` / `components.go` — preview pane (fixed-order hideable blocks)
- `theme` consumed everywhere — never hardcode colors

## CONVENTIONS

- Filters and reload are **injected**: `FilterFunc` (notes), `TaskFilterFunc` (tasks), and `ReloadFunc func(path string) (*model.Document, error)` passed to constructors; `internal/tui` never imports `internal/parse`/`internal/search`.
- Test files mirror sources; helpers live in `fixtures_test.go`-style test files.

## ANTI-PATTERNS (ENFORCED BY TESTS)

- Task TUI code must NOT reference `SetFilter` or any global filter — `tasks_run_test.go` greps `tasks_*` sources and fails on hit. Use `buildTaskModel`/`NewTaskModelWithFilter`.
- Don't route the note picker's `SetFilter` through tasks code paths.
- Don't hardcode styles — go through `theme.Theme`.
