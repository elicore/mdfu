package tui

import (
	"fmt"
	"regexp"
	"strings"

	"github.com/charmbracelet/bubbles/help"
	"github.com/charmbracelet/lipgloss"

	"github.com/elicore/mdfu/internal/task"
)

// taskWideLayout is the terminal width at or above which the list and detail
// panes are joined side by side; below it they stack.
const taskWideLayout = 100

// View implements tea.Model for the tasks TUI: a filter line (in filter mode),
// a list pane, a detail pane, and a status/help footer. Edit modes render their
// own editor frame through renderTaskEditor (tasks_edit.go).
func (m TaskModel) View() string {
	if m.mode == taskModeEditTitle || m.mode == taskModeEditBody {
		return m.renderTaskEditor()
	}
	out := m.renderTaskBrowse()
	return m.clampTaskFrame(out)
}

// clampTaskFrame applies the note picker's hard terminal clamp and closes any
// OSC 8 hyperlink left open by truncation.
func (m TaskModel) clampTaskFrame(out string) string {
	if m.width > 0 || m.height > 0 {
		clamp := lipgloss.NewStyle()
		if m.width > 0 {
			clamp = clamp.MaxWidth(m.width)
		}
		if m.height > 0 {
			clamp = clamp.MaxHeight(m.height)
		}
		out = clamp.Render(out)
	}
	return closeOpenHyperlinks(out)
}

// renderTaskBrowse lays out the list and detail panes and the footer.
func (m TaskModel) renderTaskBrowse() string {
	var b strings.Builder
	if m.mode == taskModeFilter {
		b.WriteString(m.input.View())
		b.WriteString("\n")
	}
	if m.width >= taskWideLayout {
		listW := m.width/2 - 3
		if listW < 20 {
			listW = 20
		}
		detailW := m.width - listW - 6
		if detailW < 20 {
			detailW = 20
		}
		left := m.pane(m.fit(m.renderTaskList(), listW))
		right := m.pane(m.fit(limitLines(m.renderTaskDetail(detailW), m.visibleRows()), detailW))
		b.WriteString(lipgloss.JoinHorizontal(lipgloss.Top, left, "  ", right))
	} else {
		detailW := m.width - 4
		if detailW < 20 {
			detailW = 20
		}
		b.WriteString(m.pane(m.renderTaskList()))
		b.WriteString("\n")
		b.WriteString(m.pane(limitLines(m.renderTaskDetail(detailW), m.detailRows())))
	}
	b.WriteString("\n")
	b.WriteString(m.renderTaskFooter())
	return b.String()
}

// pane applies the task pane border style.
func (m TaskModel) pane(s string) string { return m.th.TaskPane.Render(s) }

// fit caps every line of s to w cells so a pane cannot wrap and inflate height.
func (m TaskModel) fit(s string, w int) string {
	if w <= 0 {
		return s
	}
	return lipgloss.NewStyle().MaxWidth(w).Render(s)
}

// renderTaskList renders the visible rows within the list budget.
func (m TaskModel) renderTaskList() string {
	if len(m.filtered) == 0 {
		return m.th.Dim.Render("(no tasks)")
	}
	maxRows := m.visibleRows()
	if maxRows < 1 {
		maxRows = 1
	}
	start := m.offset
	if start < 0 {
		start = 0
	}
	if start >= len(m.filtered) {
		start = 0
	}
	end := start + maxRows
	if end > len(m.filtered) {
		end = len(m.filtered)
	}
	more := end < len(m.filtered)
	if more && maxRows > 1 {
		end--
	}
	re := m.matchRe()
	hl := m.th.HighlightSGR
	var b strings.Builder
	for i := start; i < end; i++ {
		b.WriteString(m.renderTaskRow(m.filtered[i], i == m.cursor, re, hl))
		if i < end-1 {
			b.WriteString("\n")
		}
	}
	if end < len(m.filtered) {
		b.WriteString("\n" + m.th.Dim.Render(fmt.Sprintf("... %d more", len(m.filtered)-end)))
	}
	return b.String()
}

// renderTaskRow renders one list row: checkbox, ID, title, tags, explicit
// priority, and display properties. The cursor row wears TaskSelected, a done
// row TaskDone, an unresolved blocked_by value TaskBlocker.
func (m TaskModel) renderTaskRow(it TaskItem, cursor bool, re *regexp.Regexp, hl string) string {
	var sb strings.Builder
	if it.Checked {
		sb.WriteString("[x]")
	} else {
		sb.WriteString("[ ]")
	}
	if it.ID != "" {
		sb.WriteString(" ")
		sb.WriteString(m.th.TaskID.Render(highlightRe(it.ID, re, hl)))
	}
	sb.WriteString(" ")
	sb.WriteString(highlightRe(it.Title, re, hl))
	for _, tag := range it.Tags {
		sb.WriteString(" ")
		sb.WriteString(m.th.TaskTag.Render(highlightRe("#"+tag, re, hl)))
	}
	if it.Priority != "" {
		sb.WriteString(" ")
		sb.WriteString(m.th.TaskPriority.Render(highlightRe("!"+it.Priority, re, hl)))
	}
	keys, values := task.DisplayProperties(it.Task, m.allTasks())
	for _, k := range keys {
		sb.WriteString(" ")
		v := "@" + k + ":" + values[k]
		if k == "blocked_by" {
			sb.WriteString(m.th.TaskBlocker.Render(highlightRe(v, re, hl)))
		} else {
			sb.WriteString(highlightRe(v, re, hl))
		}
	}
	line := sb.String()
	switch {
	case cursor:
		line = m.th.TaskSelected.Render(line)
	case it.Checked:
		line = m.th.TaskDone.Render(line)
	}
	return line
}

// renderTaskDetail renders the focused task: header, labelled metadata rows,
// then the markdown body with OSC 8 links.
func (m TaskModel) renderTaskDetail(width int) string {
	if len(m.filtered) == 0 || m.cursor < 0 || m.cursor >= len(m.filtered) {
		return m.th.Dim.Render("(no task selected)")
	}
	if width < 20 {
		width = 20
	}
	it := m.filtered[m.cursor]
	style := m.th.MarkdownStyle
	re := m.matchRe()
	parts := make([]string, 0, 3)
	if strings.TrimSpace(it.HeaderRaw) != "" {
		parts = append(parts, strings.TrimRight(renderMarkdown(it.HeaderRaw, width, style), "\n"))
	}
	parts = append(parts, m.renderTaskMeta(it))
	if strings.TrimSpace(it.Body) != "" {
		body, links := extractAndHide(it.Body, true)
		rendered := renderMarkdown(body, width, style)
		rendered = highlightRe(rendered, re, m.th.HighlightSGR)
		rendered = patchLinks(rendered, links, true, m.th.LinkSGR)
		parts = append(parts, strings.TrimRight(rendered, "\n"))
	}
	return strings.Join(parts, "\n\n")
}

// renderTaskMeta renders labelled metadata rows for the detail pane.
func (m TaskModel) renderTaskMeta(it TaskItem) string {
	label := lipgloss.NewStyle().Foreground(lipgloss.Color("245"))
	rows := make([][2]string, 0, 8)
	if it.ID != "" {
		rows = append(rows, [2]string{"ID", it.ID})
	}
	rows = append(rows, [2]string{"Status", statusWord(it.Checked)})
	if it.Title != "" {
		rows = append(rows, [2]string{"Title", it.Title})
	}
	if it.Priority != "" {
		rows = append(rows, [2]string{"Priority", m.th.TaskPriority.Render("!" + it.Priority)})
	}
	if len(it.Tags) > 0 {
		tags := make([]string, len(it.Tags))
		for i, t := range it.Tags {
			tags[i] = "#" + t
		}
		rows = append(rows, [2]string{"Tags", m.th.TaskTag.Render(strings.Join(tags, " "))})
	}
	keys, values := task.DisplayProperties(it.Task, m.allTasks())
	for _, k := range keys {
		val := values[k]
		if k == "blocked_by" {
			val = m.th.TaskBlocker.Render(val)
		}
		rows = append(rows, [2]string{"@" + k, val})
	}
	if it.File != "" {
		rows = append(rows, [2]string{"File", fmt.Sprintf("%s:%d", it.File, it.Line)})
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

// renderTaskFooter renders the compact status line, or a bubbles/help full
// help while in help mode.
func (m TaskModel) renderTaskFooter() string {
	if m.mode == taskModeHelp {
		h := help.New()
		h.ShowAll = true
		if m.width > 0 {
			h.Width = m.width
		}
		return h.View(m.keys)
	}
	blocked := "hidden"
	if m.showBlocked {
		blocked = "shown"
	}
	s := fmt.Sprintf("%d/%d • blocked:%s • x:done • t:edit • ?:help", len(m.filtered), len(m.items), blocked)
	if m.status != "" {
		s += " • " + m.status
	}
	return m.th.Dim.Render(s)
}

// statusWord renders a checkbox state as "open" or "done".
func statusWord(checked bool) string {
	if checked {
		return "done"
	}
	return "open"
}

// matchRe compiles the current filter query into a highlight regexp.
func (m TaskModel) matchRe() *regexp.Regexp {
	return termsRegexp(queryTerms(m.input.Value()))
}

// allTasks flattens the model's items to engine tasks so blocker resolution and
// DisplayProperties see the full scope.
func (m TaskModel) allTasks() []task.Task {
	out := make([]task.Task, 0, len(m.items))
	for _, it := range m.items {
		out = append(out, it.Task)
	}
	return out
}
