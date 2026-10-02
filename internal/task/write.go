package task

import (
	"fmt"
	"os"
	"sort"
	"strings"
)

// taskBlockRange returns the end of t's block, using the broadened range for a
// broadened checkbox item and the strict range otherwise.
func taskBlockRange(t Task, lines []string, start int) (int, bool) {
	if t.Broad {
		return blockRangeAny(lines, start)
	}
	return blockRange(lines, start)
}

// MoveTask relocates t's whole block to the end of target. It is the reusable
// mutation primitive behind `mdfu task move`, extracted so the TUI does not
// duplicate the ordering rules: validate the target is not a directory, treat a
// move onto the same realpath as a silent no-op, validate writability of both
// files, verify the source is not stale, write the target first, then rewrite
// the source with the block removed.
//
// Errors are returned unwrapped so callers can surface a *StaleError or a
// permission error verbatim with their own prefix.
func MoveTask(t Task, target string) error {
	srcPath := t.File

	if info, err := os.Stat(target); err == nil && info.IsDir() {
		return fmt.Errorf("'%s' is a directory", target)
	}
	if sameFileReal(srcPath, target) {
		return nil
	}
	if err := assertWritablePath(srcPath); err != nil {
		return err
	}
	if err := assertWritablePath(target); err != nil {
		return err
	}

	src, err := Open(srcPath)
	if err != nil {
		return err
	}
	start := t.Line - 1
	if start < 0 || start >= len(src.lines) || strings.TrimSuffix(src.lines[start], "\r") != t.HeaderRaw {
		return &StaleError{File: srcPath, ID: t.ID}
	}
	if err := src.VerifyTask(t); err != nil {
		return err
	}
	rangeFn := blockRange
	if t.Broad {
		rangeFn = blockRangeAny
	}
	end, ok := rangeFn(src.lines, start)
	if !ok || end <= start {
		return &StaleError{File: srcPath, ID: t.ID}
	}
	block := strings.Join(src.lines[start:end], "\n")

	if err := AppendBlock(target, block); err != nil {
		return err
	}
	if err := src.ReplaceLines(start, end, nil); err != nil {
		return err
	}
	return src.Commit()
}

// ArchiveTasks appends the blocks named by targets to the resolved archive file
// under baseDir and removes them from their sources. It is the reusable
// mutation primitive behind `mdfu task archive`; selecting which tasks to
// archive (including the "task is not done" rule for named IDs) stays with the
// caller.
//
// Every source is verified before a single byte is written, the archive is
// written before any source is rewritten, and per-file removals run in
// descending line order. An empty target list is a no-op.
func ArchiveTasks(targets []Task, baseDir string, cfg Config) error {
	if len(targets) == 0 {
		return nil
	}

	archivePath := resolveArchive(baseDir, cfg.ArchivePath)
	if err := preflightArchive(baseDir, archivePath); err != nil {
		return err
	}

	edits := map[string]*FileEdit{}
	var order []string
	openEdit := func(file string) (*FileEdit, error) {
		if fe, ok := edits[file]; ok {
			return fe, nil
		}
		fe, err := Open(file)
		if err != nil {
			return nil, err
		}
		edits[file] = fe
		order = append(order, file)
		return fe, nil
	}

	for _, t := range targets {
		fe, err := openEdit(t.File)
		if err != nil {
			return err
		}
		if err := fe.VerifyTask(t); err != nil {
			return err
		}
	}

	blocks := make([]string, 0, len(targets))
	for _, t := range targets {
		fe := edits[t.File]
		start := t.Line - 1
		end, ok := taskBlockRange(t, fe.lines, start)
		if !ok {
			end = start + 1
		}
		blocks = append(blocks, strings.Join(fe.lines[start:end], "\n"))
	}
	if err := AppendBlock(archivePath, strings.Join(blocks, "\n\n")); err != nil {
		return archiveWriteError(archivePath, err)
	}

	byFile := map[string][]Task{}
	for _, t := range targets {
		byFile[t.File] = append(byFile[t.File], t)
	}
	for _, file := range order {
		fe := edits[file]
		fileTasks := byFile[file]
		sort.Slice(fileTasks, func(i, j int) bool { return fileTasks[i].Line > fileTasks[j].Line })
		for _, t := range fileTasks {
			start := t.Line - 1
			end, ok := taskBlockRange(t, fe.lines, start)
			if !ok {
				end = start + 1
			}
			if err := fe.ReplaceLines(start, end, nil); err != nil {
				return err
			}
		}
		if err := fe.Commit(); err != nil {
			return archiveWriteError(file, err)
		}
	}
	return nil
}
