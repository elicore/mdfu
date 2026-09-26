# Doing one task

This skill takes a single task from "pick it" to "commit it". It assumes tasks
already carry identifiers and that you are in the directory that holds them (or
that you pass `--path`).

## 1. Pick a task

List what is open and choose one identifier:

```
mdfu task list
```

Add `--tag` or `--priority` to narrow the list, or `--sort priority` to bring
the most important work to the top. Blocked tasks are hidden by default; pass
`--blocked` if you want to see them.

## 2. Read it in full

Look at the whole task, including its body, so you know the acceptance criteria
before you start:

```
mdfu task view PRJ-001
```

If you would rather read it in your editor at the exact line, use the `open`
command:

```
mdfu task open PRJ-001
```

## 3. Do the work

Make the change the task asks for. Keep the work scoped to this one identifier;
if you discover a second piece of work, add it as its own task (see the
`mdfu-task-add` skill) instead of widening this one.

## 4. Mark it done

A task is done when its checkbox becomes `- [x]`. Flip it in the editor you
already have open, or edit the file directly. The body stays where it is; only
the checkbox changes.

## 5. Check the scope

Before committing, confirm nothing else broke:

```
mdfu task validate
```

Duplicate identifiers exit non-zero and must be fixed. Metadata warnings are
informational and do not block you. You can also confirm the task no longer
appears as open:

```
mdfu task list
```

## 6. Archive and commit

Move finished tasks out of the active files so the working list stays short:

```
mdfu task archive PRJ-001
```

Omit the identifier to archive every done task in scope. Archiving writes the
block to the archive file first and only then removes it from its source, so a
failure leaves the source intact. Finally, commit the changed task files along
with the code change, using a message that names the task identifier.
