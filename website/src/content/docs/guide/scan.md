---
title: Scan
description: Markdown discovery rules — hidden files, symlinks, gitignore, sorting.
---

Implementation: `crates/mdfu-core/src/scan.rs` (`scan::walk_markdown`).

- Matches `*.md` case-insensitively, returns sorted paths.
- `.git` always skipped, even with `--hidden`.
- Hidden files/dirs (`.`-prefixed) skipped unless `--hidden` (root itself exempt).
- Symlinks never followed; symlinked files/dirs skipped (avoids loops).
- `Options::limit > 0` truncates after sort (note: applies to discovery, distinct from `--limit` on results).
- Gitignored files are skipped **by default**: `.gitignore` and `.ignore` files, including those in parent directories, are honored. `--no-ignore` turns this off and includes gitignored files. This is a deliberate divergence from the frozen Go baseline, which accepted the option but ignored it — see the [CLI](/reference/cli/#rust-port) port notes and the [Roadmap](/project/roadmap/).

See also: [CLI](/reference/cli/) for the `--hidden` / `--no-ignore` / `--root` flags, and [Architecture](/reference/architecture/) for where scan fits in the pipeline.