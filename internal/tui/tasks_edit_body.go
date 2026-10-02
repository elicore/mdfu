package tui

import (
	"errors"
	"strings"

	bubbleskey "github.com/charmbracelet/bubbles/key"
	tea "github.com/charmbracelet/bubbletea"

	"github.com/elicore/mdfu/internal/task"
)

// renderTaskBodyEditor shows the full-width body textarea and its status line.
func (m TaskModel) renderTaskBodyEditor() string {
	var b strings.Builder
	b.WriteString(m.editor.View())
	b.WriteString("\n")
	b.WriteString(m.renderTaskEditorStatus())
	return m.clampTaskFrame(b.String())
}

// updateEditBody handles taskModeEditBody. The checkbox and ID are not editable
// here: the editor only rewrites the block's body lines.
func (m TaskModel) updateEditBody(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch {
	case bubbleskey.Matches(msg, m.keys.Save):
		if err := m.saveBody(); err != nil {
			m.err = err
			m.status = err.Error()
		}
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Cancel):
		m.status = "cancelled"
		m.mode = taskModeBrowse
		m.editor.Blur()
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Quit):
		m.aborted = true
		return m, tea.Quit
	}
	var cmd tea.Cmd
	m.editor, cmd = m.editor.Update(msg)
	return m, cmd
}

// saveBody re-indents the editor text and replaces the block's body lines via
// task.FileEdit. The body line range is derived from the task's 1-based header
// Line and the number of body lines in its dedented Body.
func (m *TaskModel) saveBody() error {
	it, path, ok := m.focusedTaskAndPath()
	if !ok {
		return errors.New("no task selected")
	}
	if path == "" {
		return errors.New("no task file")
	}
	text := strings.TrimRight(m.editor.Value(), "\n")
	indent := it.BodyIndent
	if indent == "" {
		indent = "  "
	}
	bodyLines := buildBodyLines(text, indent)
	oldCount := bodyLineCount(it.Body)
	start := it.Line
	end := start + oldCount

	fe, err := task.Open(path)
	if err != nil {
		return err
	}
	if err := fe.VerifyTask(it.Task); err != nil {
		return err
	}
	if err := fe.ReplaceBodyRange(start, end, bodyLines); err != nil {
		return err
	}
	if err := fe.Commit(); err != nil {
		return err
	}

	parsed := it.Task
	if text == "" {
		parsed.Body = ""
		parsed.BodyIndent = ""
	} else {
		parsed.Body = text
		parsed.BodyIndent = indent
	}
	m.updateFocusedTask(parsed)
	m.mode = taskModeBrowse
	m.editor.Blur()
	m.status = "saved body"
	return nil
}

// focusedBodyIndent returns the indent the focused task's existing body uses,
// defaulting to two spaces.
func (m TaskModel) focusedBodyIndent() string {
	if m.cursor >= 0 && m.cursor < len(m.filtered) {
		if ind := m.filtered[m.cursor].BodyIndent; ind != "" {
			return ind
		}
	}
	return "  "
}

// buildBodyLines re-indents text by indent: blank lines stay blank and
// unindented, every other line is prefixed. An empty text yields no lines.
func buildBodyLines(text, indent string) []string {
	if text == "" {
		return nil
	}
	lines := strings.Split(strings.ReplaceAll(text, "\r\n", "\n"), "\n")
	out := make([]string, len(lines))
	for i, l := range lines {
		if strings.TrimSpace(l) == "" {
			out[i] = ""
			continue
		}
		out[i] = indent + l
	}
	return out
}

// bodyLineCount returns the number of lines a dedented body occupies: 0 when
// the body is empty, else 1 plus the newline count.
func bodyLineCount(body string) int {
	if body == "" {
		return 0
	}
	return 1 + strings.Count(body, "\n")
}
