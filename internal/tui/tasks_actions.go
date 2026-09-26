package tui

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"strings"

	tea "github.com/charmbracelet/bubbletea"

	"github.com/elicore/mdfu/internal/task"
)

// TaskResult is the tasks TUI's final result, mirroring the note picker's
// abort/result split: Aborted is true when the user pressed esc on an empty
// filter or q/ctrl+c; otherwise TaskID names the focused task.
type TaskResult struct {
	Aborted bool
	Action  string
	TaskID  string
}

// Result returns the final result. Aborted wins; otherwise the focused task's
// ID is reported (empty when the list is empty).
func (m *TaskModel) Result() TaskResult {
	if m.aborted {
		return TaskResult{Aborted: true}
	}
	r := TaskResult{Action: m.action}
	if m.cursor >= 0 && m.cursor < len(m.filtered) {
		r.TaskID = m.filtered[m.cursor].ID
	}
	return r
}

// taskEditorFinishedMsg is delivered after $EDITOR exits; the model reloads the
// task set so external edits appear.
type taskEditorFinishedMsg struct {
	err error
}

// focusedTaskAndPath returns the focused item and the file path to mutate. A
// blank path means the item carries no file (headless fixtures); callers fall
// back to in-memory behaviour.
func (m *TaskModel) focusedTaskAndPath() (TaskItem, string, bool) {
	if m.cursor < 0 || m.cursor >= len(m.filtered) {
		return TaskItem{}, "", false
	}
	it := m.filtered[m.cursor]
	path := it.File
	if path == "" && m.base != "" {
		if info, err := os.Stat(m.base); err == nil && !info.IsDir() {
			path = m.base
		}
	}
	return it, path, true
}

// updateFocusedTask replaces the focused item's engine task in both the
// filtered and backing slices after a successful write.
func (m *TaskModel) updateFocusedTask(t task.Task) {
	if m.cursor < 0 || m.cursor >= len(m.filtered) {
		return
	}
	old := m.filtered[m.cursor]
	m.filtered[m.cursor].Task = t
	for i := range m.items {
		if m.items[i].File == old.File && m.items[i].Line == old.Line {
			m.items[i].Task = t
			return
		}
	}
}

// toggleDoneTask flips the focused task's checkbox through task.FileEdit and
// refreshes the in-memory item only after a successful commit. Without a file
// path it degrades to the in-memory toggle used by headless fixtures.
func (m *TaskModel) toggleDoneTask() error {
	it, path, ok := m.focusedTaskAndPath()
	if !ok {
		return nil
	}
	if path == "" {
		m.toggleDone()
		return nil
	}
	fe, err := task.Open(path)
	if err != nil {
		return err
	}
	if err := fe.Verify(it.ID, it.Line); err != nil {
		return err
	}
	if err := fe.FlipCheckbox(it.Line); err != nil {
		return err
	}
	if err := fe.Commit(); err != nil {
		return err
	}
	it.Checked = !it.Checked
	if it.Checked {
		it.Task.Status = 'x'
	} else {
		it.Task.Status = ' '
	}
	m.updateFocusedTask(it.Task)
	return nil
}

// openInEditorCmd suspends the program to run $EDITOR +<line> <file>. On
// resume the callback delivers taskEditorFinishedMsg, which reloads the task
// set. tea.ExecProcess (bubbletea v1.3.10) is the suspension path chosen.
func (m *TaskModel) openInEditorCmd() tea.Cmd {
	it, path, ok := m.focusedTaskAndPath()
	if !ok {
		return nil
	}
	editor := os.Getenv("EDITOR")
	if strings.TrimSpace(editor) == "" {
		m.err = errors.New("$EDITOR is not set")
		m.status = "$EDITOR is not set"
		return nil
	}
	fields := strings.Fields(editor)
	argv := make([]string, 0, len(fields)+2)
	argv = append(argv, fields[1:]...)
	argv = append(argv, fmt.Sprintf("+%d", it.Line), path)
	c := exec.Command(fields[0], argv...)
	return tea.ExecProcess(c, func(err error) tea.Msg {
		return taskEditorFinishedMsg{err: err}
	})
}

// appendTargetPath picks the file a newly created task is appended to: the
// --path file when the scope is a single file, else the focused task's file.
func (m *TaskModel) appendTargetPath() string {
	if m.base != "" {
		if info, err := os.Stat(m.base); err == nil && !info.IsDir() {
			return m.base
		}
	}
	if m.cursor >= 0 && m.cursor < len(m.filtered) {
		return m.filtered[m.cursor].File
	}
	return ""
}

// reload re-reads the task scope from disk through the engine and refilters,
// keeping the cursor on the same task when it survives.
func (m *TaskModel) reload() {
	base := m.base
	if base == "" {
		m.refilter()
		return
	}
	cwd, _ := os.Getwd()
	scope := task.Scope{Base: base, Config: m.taskCfg, Cwd: cwd}
	if info, err := os.Stat(base); err == nil && !info.IsDir() {
		scope.IsFile = true
		scope.File = base
	}
	tasks, _, err := task.LoadScope(scope)
	if err != nil {
		m.err = err
		m.status = err.Error()
		return
	}
	items := make([]TaskItem, 0, len(tasks))
	for _, t := range tasks {
		items = append(items, TaskItem{Task: t})
	}
	m.items = items
	m.refilter()
}
