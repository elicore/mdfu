# Adding a task

This skill walks through capturing one new task: from a raw checkbox line to an
identified, tagged task, and finally to a sanity check that the file still
validates. Run every command from the directory that contains the task file (or
pass the global `--path` flag).

## 1. Write the raw line

Add the task as an ordinary Markdown checkbox under the heading where it
belongs:

```
- [ ] Wire the new export button
```

You do not have to invent an identifier by hand. Leave the line unidentified and
let the tooling assign one.

## 2. See what still needs an identifier

Run the list command. The unidentified line is reported by the trailing note,
which tells you how many tasks are waiting for an identifier:

```
mdfu task list
```

## 3. Assign identifiers

The `ids` command assigns the next free numeric parts. Use `--prefix` to choose
the prefix explicitly; without it the command derives one from the file, from a
seed line, or by prompting:

```
mdfu task ids --prefix PRJ
```

It prints one rewritten header per assignment, in order. If you prefer to check
before writing, run `mdfu task list` again afterwards and confirm the new
identifier appears.

## 4. Add metadata

Once the task has an identifier, `set` appends metadata tokens. Tags start with
`#`, priorities with `!`, and properties with `@`. Separate the identifiers from
the tokens; both sides may be space or comma separated:

```
mdfu task set PRJ-001 '#release' '!high' '@owner:me'
```

An already-present tag is skipped, and a new priority replaces the previous one
in place. The title text is never rewritten.

## 5. Confirm the result

Print the task back to be sure the header and body read the way you expect, and
run the validator to catch identifier collisions or malformed metadata:

```
mdfu task view PRJ-001
mdfu task validate
```

`validate` writes any problems to stderr. Duplicate identifiers make it exit
non-zero; metadata warnings do not. Commit the changed files when you are
satisfied.
