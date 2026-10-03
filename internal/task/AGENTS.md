# internal/task — mdtask-compat task engine

## OVERVIEW

Pure-Go parser/planner/CLI engine for checkbox tasks in Markdown files, reimplementing the external `mdtask` format. **`SPEC.md` here is normative and frozen** — tests assert byte-identical output against it.

## WHERE TO LOOK

| Task | File(s) |
|------|---------|
| Subcommand registry + `Env`/`Result` types | `commands.go` (never edit dispatch — new cmds register via `init()`) |
| Header parsing / task grammar | `format.go` (`ParseHeader`), spec §A |
| ID assignment, resolution, dupes | `id.go`, `ids.go` |
| Task discovery in files | `discover.go` |
| Rendering output | `render.go` (byte-identical to SPEC §D/E) |
| Config (`.mdtaskrc`/`.mdfurc`) | `config.go` (`ResolveBasePath`) |
| Scope resolution (`./dir`, globs) | `scope.go`, `glob.go` |
| File writes / archive safety | `write.go`, `archive.go` (E17/E19 preconditions) |
| Embedded agent skills | `skills/` + `install_skills.go` (via `embed`) |
| Human docs of format | `skills/mdfu-task/SKILL.md` |

## CONVENTIONS

- Exit codes per SPEC §I: 0 ok / 1 runtime / 2 usage. `fail(env, err)` helper.
- `Env` injects Stdout/Stderr/Cwd/Now/Terminal — tests pass buffers, never the real host.
- Tests are in-package, table-driven, with literal expected strings from SPEC.md.
- Every prevalidated precondition before writing (no partial writes; archive = append-then-remove).

## ANTI-PATTERNS

- Don't re-parse body lines as tasks (SPEC §A — body is opaque).
- Don't append a trailing newline that wasn't in the file (SPEC §A).
- Don't write "mdtask"-family strings into `skills/` assets — `install_skills_test.go` fails the build.
- Don't change SPEC.md output without updating the corresponding table test in the same change.
