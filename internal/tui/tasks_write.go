package tui

import (
	"errors"
	"os"
	"path/filepath"
	"strings"

	"github.com/elicore/mdfu/internal/task"
)

// errTaskDestinationRequired is a TUI-only status string shown when the move
// prompt is confirmed empty; it is not a CLI error from SPEC.md.
var errTaskDestinationRequired = errors.New("destination file required")

// commitMove applies the move prompt target through the engine's MoveTask
// primitive and reloads the scope. reload() keys items by task ID, so the
// cursor stays on the moved task in its new file.
func (m *TaskModel) commitMove() error {
	it, _, ok := m.focusedTaskAndPath()
	if !ok {
		m.mode = taskModeBrowse
		m.moveInput.Blur()
		return nil
	}
	target := strings.TrimSpace(m.moveInput.Value())
	if target == "" {
		return errTaskDestinationRequired
	}
	if err := task.MoveTask(it.Task, target); err != nil {
		return err
	}
	m.status = "moved"
	m.mode = taskModeBrowse
	m.moveInput.Blur()
	m.reload()
	return nil
}

// archiveFocused archives the focused task only when it is done. An open task
// shows the TUI-only status "task is not done" and writes nothing. Engine
// errors (a *StaleError or a permission error) land in the status line.
func (m *TaskModel) archiveFocused() {
	it, _, ok := m.focusedTaskAndPath()
	if !ok {
		return
	}
	if !it.Checked {
		m.status = "task is not done"
		return
	}
	if err := task.ArchiveTasks([]task.Task{it.Task}, m.taskBaseDir(), m.taskCfg); err != nil {
		m.err = err
		m.status = err.Error()
		return
	}
	m.status = "archived"
	m.reload()
}

// taskBaseDir resolves the directory the archive destination is measured
// against: a single-file --path scope uses that file's directory, matching the
// CLI, and an unset base falls back to the focused task's directory.
func (m *TaskModel) taskBaseDir() string {
	base := m.base
	if base == "" {
		if _, path, ok := m.focusedTaskAndPath(); ok && path != "" {
			return filepath.Dir(path)
		}
		return "."
	}
	if info, err := os.Stat(base); err == nil && !info.IsDir() {
		return filepath.Dir(base)
	}
	return base
}
