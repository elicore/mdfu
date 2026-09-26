package task

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// runMove implements `mdfu task move <id> <file>`: relocate a task's whole
// block to the end of the target file. The target is written before the source
// is rewritten; a move onto the same realpath is a silent no-op.
func runMove(env Env, args []string) Result {
	scope, rest, err := resolveScope(env, args)
	if err != nil {
		return fail(env, err)
	}
	if len(rest) != 2 {
		errf(env, "move requires a task ID and a destination file")
		return Result{Code: 2}
	}

	tasks, _, err := scopeTaskSet(scope)
	if err != nil {
		return fail(env, err)
	}
	t, err := Resolve(rest[0], tasks)
	if err != nil {
		return fail(env, err)
	}
	return moveResolvedTask(env, t, rest[1])
}

// moveResolvedTask performs the mutation half of move against an already
// resolved task. It is split out so a stale source can be exercised between
// resolution and the write without touching the process filesystem in the
// command layer.
func moveResolvedTask(env Env, t Task, target string) Result {
	srcPath := t.File

	if info, err := os.Stat(target); err == nil && info.IsDir() {
		return fail(env, fmt.Errorf("'%s' is a directory", target))
	}
	if sameFileReal(srcPath, target) {
		return Result{Code: 0}
	}
	if err := assertWritablePath(srcPath); err != nil {
		return fail(env, err)
	}
	if err := assertWritablePath(target); err != nil {
		return fail(env, err)
	}

	src, err := Open(srcPath)
	if err != nil {
		return fail(env, err)
	}
	start := t.Line - 1
	if start < 0 || start >= len(src.lines) || strings.TrimSuffix(src.lines[start], "\r") != t.HeaderRaw {
		return fail(env, &StaleError{File: srcPath, ID: t.ID})
	}
	if err := src.Verify(t.ID, t.Line); err != nil {
		return fail(env, err)
	}
	end, ok := blockRange(src.lines, start)
	if !ok || end <= start {
		return fail(env, &StaleError{File: srcPath, ID: t.ID})
	}
	block := strings.Join(src.lines[start:end], "\n")

	if err := AppendBlock(target, block); err != nil {
		return fail(env, err)
	}
	if err := src.ReplaceLines(start, end, nil); err != nil {
		return fail(env, err)
	}
	if err := src.Commit(); err != nil {
		return fail(env, err)
	}
	return Result{Code: 0}
}

// assertWritablePath reports a permission-denied error when path, or the
// nearest existing ancestor of a not-yet-created path, carries no write bit.
func assertWritablePath(path string) error {
	info, err := os.Stat(path)
	if err == nil {
		if info.Mode().Perm()&0o222 == 0 {
			return fmt.Errorf("cannot write to '%s': permission denied", path)
		}
		return nil
	}
	if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	dir := filepath.Dir(path)
	for {
		if di, statErr := os.Stat(dir); statErr == nil {
			if di.Mode().Perm()&0o222 == 0 {
				return fmt.Errorf("cannot write to '%s': permission denied", path)
			}
			return nil
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return nil
		}
		dir = parent
	}
}

// sameFileReal reports whether two paths resolve to the same file, comparing
// realpaths when both exist and cleaned absolute paths otherwise.
func sameFileReal(a, b string) bool {
	ra, errA := filepath.EvalSymlinks(a)
	rb, errB := filepath.EvalSymlinks(b)
	if errA == nil && errB == nil {
		return filepath.Clean(ra) == filepath.Clean(rb)
	}
	absA, absErrA := filepath.Abs(a)
	absB, absErrB := filepath.Abs(b)
	if absErrA != nil || absErrB != nil {
		return filepath.Clean(a) == filepath.Clean(b)
	}
	return filepath.Clean(absA) == filepath.Clean(absB)
}

func init() {
	register(Subcommand{Name: "move", Order: 40, Run: runMove})
}
