package tui

import (
	"errors"
	"strings"

	bubbleskey "github.com/charmbracelet/bubbles/key"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"

	"github.com/elicore/mdfu/internal/task"
)

// renderTaskEditor dispatches the two edit frames: title/metadata and body.
func (m TaskModel) renderTaskEditor() string {
	if m.mode == taskModeEditBody {
		return m.renderTaskBodyEditor()
	}
	return m.renderTaskTitleEditor()
}

// renderTaskTitleEditor shows the header textarea plus a live metadata panel.
func (m TaskModel) renderTaskTitleEditor() string {
	var b strings.Builder
	b.WriteString(m.editor.View())
	b.WriteString("\n")
	b.WriteString(m.renderTitleSidePanel())
	b.WriteString("\n")
	b.WriteString(m.renderTaskEditorStatus())
	return m.clampTaskFrame(b.String())
}

// renderTitleSidePanel parses the editor's current value and renders the ID,
// status, tags, priority, and properties it recognizes. It uses the broadened
// grammar so an edited non-mdtask checkbox still shows its metadata.
func (m TaskModel) renderTitleSidePanel() string {
	parsed, ok := task.ParseCheckbox(m.editor.Value())
	label := lipgloss.NewStyle().Foreground(lipgloss.Color("245"))
	rows := make([][2]string, 0, 8)
	if !ok {
		rows = append(rows, [2]string{"Header", m.th.Dim.Render("invalid task header")})
	} else {
		id := parsed.ID
		if id == "" {
			id = "(none)"
		}
		rows = append(rows, [2]string{"ID", id})
		rows = append(rows, [2]string{"Status", statusWord(parsed.Checked)})
		if parsed.Title != "" {
			rows = append(rows, [2]string{"Title", parsed.Title})
		}
		if len(parsed.Tags) > 0 {
			tags := make([]string, len(parsed.Tags))
			for i, t := range parsed.Tags {
				tags[i] = "#" + t
			}
			rows = append(rows, [2]string{"Tags", m.th.TaskTag.Render(strings.Join(tags, " "))})
		}
		if parsed.Priority != "" {
			rows = append(rows, [2]string{"Priority", m.th.TaskPriority.Render("!" + parsed.Priority)})
		}
		for _, k := range parsed.PropertyOrder {
			rows = append(rows, [2]string{"@" + k, parsed.Properties[k]})
		}
	}
	var b strings.Builder
	for i, r := range rows {
		if i > 0 {
			b.WriteString("\n")
		}
		b.WriteString(label.Render(r[0]))
		b.WriteString("  ")
		b.WriteString(r[1])
	}
	return b.String()
}

// renderTaskEditorStatus renders the edit-mode status line.
func (m TaskModel) renderTaskEditorStatus() string {
	var s string
	if m.mode == taskModeEditBody {
		s = "body indent: " + quoteIndent(m.focusedBodyIndent()) + " • ctrl+s save • esc cancel"
	} else {
		s = "ctrl+s save • esc cancel"
	}
	if m.status != "" {
		s += " • " + m.status
	}
	return m.th.Dim.Render(s)
}

// updateEditTitle handles taskModeEditTitle.
func (m TaskModel) updateEditTitle(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch {
	case bubbleskey.Matches(msg, m.keys.Save):
		if err := m.saveTitle(); err != nil {
			m.err = err
			m.status = err.Error()
		}
		return m, nil
	case bubbleskey.Matches(msg, m.keys.Cancel):
		m.status = "cancelled"
		m.mode = taskModeBrowse
		m.newTask = false
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

// saveTitle parses the edited header and either replaces the focused task's
// header line or appends a new block. An unparseable header is refused with the
// "invalid task header" status and writes nothing.
func (m *TaskModel) saveTitle() error {
	header := strings.TrimRight(m.editor.Value(), "\r\n")
	parsed, ok := task.ParseCheckbox(header)
	if !ok {
		return errors.New("invalid task header")
	}
	if m.newTask {
		return m.appendNewTask(header, parsed)
	}
	return m.replaceHeaderLine(header)
}

// replaceHeaderLine swaps the single header line through task.FileEdit.
func (m *TaskModel) replaceHeaderLine(header string) error {
	it, path, ok := m.focusedTaskAndPath()
	if !ok {
		return errors.New("no task selected")
	}
	if path == "" {
		return errors.New("no task file")
	}
	fe, err := task.Open(path)
	if err != nil {
		return err
	}
	if err := fe.VerifyTask(it.Task); err != nil {
		return err
	}
	if err := fe.ReplaceHeaderLine(it.Line, header); err != nil {
		return err
	}
	if err := fe.Commit(); err != nil {
		return err
	}
	parsed, _ := task.ParseCheckbox(header)
	parsed.Body = it.Body
	parsed.BodyIndent = it.BodyIndent
	parsed.File = it.File
	parsed.Line = it.Line
	m.updateFocusedTask(parsed)
	m.newTask = false
	m.mode = taskModeBrowse
	m.editor.Blur()
	m.status = "saved"
	return nil
}

// appendNewTask appends the edited block to the append target through the
// engine's AppendBlock. A task with no ID is written verbatim and the status
// line points at `mdfu task ids`.
func (m *TaskModel) appendNewTask(header string, parsed task.Task) error {
	path := m.appendTargetPath()
	if path == "" {
		return errors.New("no target file")
	}
	if err := task.AppendBlock(path, header); err != nil {
		return err
	}
	m.newTask = false
	m.mode = taskModeBrowse
	m.editor.Blur()
	if parsed.ID == "" {
		m.status = "added task without an ID — run mdfu task ids"
	} else {
		m.status = "added " + parsed.ID
	}
	m.reload()
	return nil
}

// quoteIndent renders a whitespace indent for display, mapping a tab to \t.
func quoteIndent(indent string) string {
	if indent == "" {
		return `""`
	}
	q := strings.ReplaceAll(indent, "\t", `\t`)
	return `"` + q + `"`
}
