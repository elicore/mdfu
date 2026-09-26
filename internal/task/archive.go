package task

import (
	"errors"
	"flag"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
)

// runArchive implements `mdfu task archive [ids...]` per SPEC.md §F.7.
//
// With no IDs it archives every done task in scope; with IDs it archives
// exactly those and rejects a named task that is not done (E18). The
// destination is `<base>/<archivePath>`. Every precondition is validated (E19,
// a directory destination, an unwritable destination), every source is checked
// for staleness (E17), all blocks are appended to the archive first, and only
// then are they removed from their sources. Archiving nothing is a silent exit
// 0 and no path ever writes to stdout.
func runArchive(env Env, args []string) Result {
	scope, rest, err := resolveScope(env, args)
	if err != nil {
		return fail(env, err)
	}

	fs := flag.NewFlagSet("archive", flag.ContinueOnError)
	ids, done := parseFlags(env, fs, rest)
	if done != nil {
		return *done
	}

	tasks, _, err := LoadScope(scope)
	if err != nil {
		return fail(env, err)
	}

	targets, stop := archiveTargets(env, tasks, ids)
	if stop != nil {
		return *stop
	}
	if len(targets) == 0 {
		return Result{Code: 0}
	}

	baseDir := scope.Base
	if scope.IsFile {
		baseDir = filepath.Dir(scope.File)
	}
	if err := ArchiveTasks(targets, baseDir, scope.Config); err != nil {
		return fail(env, err)
	}
	return Result{Code: 0}
}

// archiveTargets resolves the tasks to archive. With an empty ids slice every
// done task is selected. With ids each name is resolved first, and a named task
// that is open stops the run with E18. The result is in scope order (file order
// then line order).
func archiveTargets(env Env, tasks []Task, ids []string) ([]Task, *Result) {
	if len(ids) == 0 {
		var out []Task
		for _, t := range tasks {
			if t.Checked {
				out = append(out, t)
			}
		}
		return out, nil
	}

	want := map[string]bool{}
	for _, id := range ids {
		t, err := Resolve(id, tasks)
		if err != nil {
			res := fail(env, err)
			return nil, &res
		}
		if !t.Checked {
			errf(env, "task '%s' is not done", id)
			res := Result{Code: 1}
			return nil, &res
		}
		want[t.ID] = true
	}

	var out []Task
	for _, t := range tasks {
		if want[t.ID] {
			out = append(out, t)
		}
	}
	return out, nil
}

// preflightArchive validates the archive destination before any write: it must
// stay inside baseDir (E19), must not be a directory, and an existing file must
// be writable.
func preflightArchive(baseDir, archivePath string) error {
	rel, err := filepath.Rel(baseDir, archivePath)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return fmt.Errorf("archive path '%s' is outside the base directory", archivePath)
	}

	info, err := os.Stat(archivePath)
	if err != nil {
		return nil
	}
	if info.IsDir() {
		return fmt.Errorf("'%s' is a directory", archivePath)
	}
	f, err := os.OpenFile(archivePath, os.O_WRONLY, 0)
	if err != nil {
		return archiveWriteError(archivePath, err)
	}
	return f.Close()
}

// archiveWriteError maps a filesystem failure to the SPEC §F.7 message, using
// the exact permission-denied spelling.
func archiveWriteError(path string, err error) error {
	if errors.Is(err, fs.ErrPermission) {
		return fmt.Errorf("cannot write to '%s': permission denied", path)
	}
	return fmt.Errorf("cannot write to '%s': %v", path, err)
}

func init() {
	register(Subcommand{Name: "archive", Order: 70, Run: runArchive})
}
