# TUI search/refresh performance regression — investigation report

**Scope:** `internal/tui` (preview + list rendering, Ctrl+C handling)
**Symptom:** "The latest commits made search much, MUCH slower on refresh, sometimes the UI gets stuck."
**Outcome:** Four compounding performance defects and one latent nil-deref crash found and fixed. Worst-case TUI frame on the real vault went from an unbounded stall to 4–20 ms.

## How I identified the problems (method)

I did not start by reading the diff and guessing. The sequence was:

1. **Frame the search path first.** Ran `git log`, mapped the packages, read the TUI `Update`/`View`/`refilter` flow. This ruled out the obvious candidate (the query/ranking path) early: `mdfu --root ~/vault/Notes --filter M12` completed in ~0.28 s, so the cost was in the interactive render path, not in scan/parse/rank.
2. **Build an A/B baseline before touching code.** Added a git worktree at `origin/main` and a throwaway benchmark that renders a preview and a full frame. Result: HEAD was ~15x slower on synthetic docs (120 ms vs 8 ms). This confirmed a regression existed and gave a number to beat.
3. **Use a profiler, not intuition.** `go test -cpuprofile` + `go tool pprof -top/-peek` attributed ~98% of preview time to `regexp.FindStringIndex` inside `highlightRe`, and later to `muesli/reflow` `PrintableRuneWidth`. Each hotspot was confirmed before editing.
4. **When the user said it persisted, change the input, not the theory.** Replaced synthetic fixtures with a repro test over the real `~/vault/Notes` (2,558 files, 863 MB, including a 2.5 MB note). Run with a short `-timeout` so the stuck test dumped the exact goroutine stack identifying the document and call site.
5. **Separate costs by timing sub-parts.** Timed `renderList()` and `renderPreviewPane()` independently to prove the list was a constant ~30 ms and the preview a separate cost, rather than assuming.

## Findings and decisions

### Finding 1 — quadratic OSC probe in `highlightRe` (first regression)
**How found:** pprof showed `highlightRe -> FindStringIndex -> indexbytebody` at ~90% of preview time.
**Cause:** the new OSC handling called `ansiOSCRe.FindStringIndex(s[i:])` on the whole remaining string on *every* loop iteration. Glamour output is SGR-dense, so this is O(n²).
**Decision:** guard the probe so it only runs when the current bytes can begin an OSC (`ESC ]`). Behavior-preserving.
**Was it right?** Right, but *incomplete*. The synthetic benchmark improved 120 ms -> 7.7 ms, which made me believe the job was done. It wasn't: synthetic bodies had short lines and short tokens, so it masked the next two defects.

### Finding 2 — unbounded excerpt handed to glamour in `matchWindow`
**How found:** the real-vault repro hung and the stack showed `renderMarkdown` receiving a 1,811,792-byte source.
**Cause:** `matchWindow` limited the excerpt to 30 *lines*, but imported notes have giant lines (max 208 KB in a 124-line file), so megabytes still reached goldmark.
**Decision:** add `bodyWindow` to clip the body to a rune-aligned byte window centered on the match before rendering.
**Was it right?** Partially. It removed the hang (multi-second -> ~950 ms) but not the slowness. I re-profiled instead of declaring victory.

### Finding 3 — reflow word wrapper is O(word²) (`breakLongTokens`)
**How found:** the post-fix profile showed the remaining ~950 ms was `glamour -> wordwrap -> PrintableRuneWidth -> runewidth`. Reading `reflow/wordwrap/wordwrap.go` and `reflow/ansi/buffer.go` confirmed the wrapper re-measures the entire current word on every character.
**Cause:** the 16 KB window was one whitespace-free token (`maxTok == window`), so wrapping alone was quadratic.
**Decision:** break whitespace-free runs at 128 runes before handing text to the renderer.
**Was it right?** Right. 950 ms -> ~30 ms. Also shrunk the window to 8 KB for margin.

### Finding 4 — `bodySnippet` collapsed whole bodies every frame
**How found:** with the preview fixed, frames were still 35–60 ms. Timing `renderList()` vs `renderPreviewPane()` showed a constant ~30 ms in the *list*, independent of the selected doc.
**Cause:** for every body-only match, `bodySnippet` did `strings.Fields(body)` and `[]rune` over the entire body, every frame.
**Decision:** window around the match before collapsing whitespace, and cache snippets per `(doc, width)` for the current query (rebuilt on `refilter`).
**Was it right?** Right. Steady-state list cost dropped to ~0.5 ms; frames to 4–20 ms.

### Finding 5 — nil-regexp crash, caught in self-review
**How found:** reviewing the final diff, not by a test. `bodyWindow` called `re.FindStringIndex` before `matchWindow`'s `re == nil` check. An empty query plus a body over 8 KB would panic. My own repro always typed a query, so it never hit this.
**Decision:** guard for nil `re`, and add `TestPreviewLargeBodyIsBounded` covering both nil and non-nil regexes so it cannot regress.

### Ctrl+C / forced exit
**How identified:** the user asked for it explicitly; the root insight is that Bubble Tea only processes Ctrl-C *between* frames, so a blocked render leaves the process unquittable.
**Decision:** keep the existing graceful `esc`/`ctrl+c` quit, and add a signal handler that escalates to restoring the terminal and `os.Exit(130)` if the event loop has not stopped within 500 ms.
**Was it right?** Right. Verified interactively (tmux) that Ctrl+C exits cleanly; the escalation path is a safety net that only fires if a render ever overruns again.

## Things I got wrong

- **I trusted the synthetic benchmark too early.** After Finding 1 the numbers looked great and I reported success. The user's real corpus exposed Findings 2–4. Lesson: a benchmark only proves the cases it contains; validate against real data before declaring a performance bug fixed.
- **First assumption was the wrong subsystem.** I initially suspected ranking/filtering because that is what "search slower" implies. The `--filter` timing disproved it in under a minute and saved a lot of wasted profiling.

## Verification

- Before/after on the real vault: 2.5 MB note preview went from a multi-second hang to ~20 ms; worst preview over the 40 largest bodies and 40 largest frontmatters was 25.6 ms; full frames 4–20 ms.
- `go test ./...` 180 passed, `go test -race ./...` 180 passed, `gofmt`/`go vet` clean.
- Interactive tmux run: typed `M12`, navigated onto the 2.5 MB note, preview rendered immediately, Ctrl+C exited cleanly.

## Out of scope (noted, not changed)

`search.Rank` costs ~75 ms per keystroke over this vault (fuzzy scoring against multi-MB `SearchBlob`s). This predates the regression and is the PLAN.md T1/T2 "indexless rank optimization" work.

## Files changed

- `internal/tui/tui.go` — the five fixes above.
- `internal/tui/tui_test.go` — regression test for large bodies with/without a query.
