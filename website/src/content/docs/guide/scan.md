---
title: Scan
description: Markdown discovery rules — hidden files, symlinks, gitignore, sorting.
---

Implementation: `internal/scan/scan.go:WalkMarkdown`.

- Matches `*.md` case-insensitively, returns sorted paths.
- `.git` always skipped, even with `--hidden`.
- Hidden files/dirs (`.`-prefixed) skipped unless `--hidden` (root itself exempt).
- Symlinks never followed; symlinked files/dirs skipped (avoids loops).
- `Limit > 0` truncates after sort (note: applies to discovery, distinct from `--limit` on results).
- `RespectGitignore` is currently **accepted but ignored** (`scan.go:31` TODO) — `--no-ignore` is a no-op until implemented. See the [Roadmap](/project/roadmap/).

See also: [CLI](/reference/cli/) for the `--hidden` / `--no-ignore` / `--root` flags, and [Architecture](/reference/architecture/) for where scan fits in the pipeline.
