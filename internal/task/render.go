package task

import (
	"encoding/json"
	"fmt"
	"io"
	"sort"
	"strings"

	"github.com/charmbracelet/lipgloss"
)

// Terminal describes the output environment. There is deliberately no width
// field: the box table sizes its columns to content and the terminal width is
// never read.
type Terminal struct {
	IsTTY   bool
	NoColor bool
}

// ListOptions controls RenderList.
//
// HiddenBlocked is []Task rather than the plan's int: SPEC §H note 1 prints one
// detail line per hidden task, so the caller passes the hidden tasks themselves
// and note 1 reads their File, Line, and HeaderRaw fields. This is a deliberate,
// documented deviation from the plan's field type.
type ListOptions struct {
	All            bool
	Blocked        bool
	Sort           string
	TagFilter      []string
	PriorityFilter []string
	JSON           bool
	Terminal       Terminal
	HiddenBlocked  []Task
	Unidentified   []Unidentified
}

// RenderList renders visible tasks in one of the three mutually exclusive
// modes from SPEC.md §F: the TTY box table, the non-TTY line format, or JSON.
// The two trailing notes of §H are written to w after the rows, unprefixed, and
// are suppressed entirely by --json. Zero visible rows print neither a table
// nor lines, though the notes still print when applicable.
func RenderList(w io.Writer, tasks []Task, opts ListOptions) error {
	visible := selectVisible(tasks, opts)
	if opts.Sort == "priority" {
		sortByPriority(visible)
	}

	if opts.JSON {
		return renderJSON(w, tasks, visible)
	}

	if len(visible) > 0 {
		if opts.Terminal.IsTTY {
			renderTable(w, tasks, visible, opts.Terminal)
		} else {
			renderLines(w, tasks, visible)
		}
	}
	renderNotes(w, opts)
	return nil
}

// selectVisible applies the default selection (open, unblocked) and the tag and
// priority filters. tasks is the full scope so blockers resolve correctly even
// when a done blocker is not itself visible.
func selectVisible(tasks []Task, opts ListOptions) []Task {
	out := make([]Task, 0, len(tasks))
	for _, t := range tasks {
		if !opts.All && t.Checked {
			continue
		}
		if !opts.Blocked && HasUnresolvedBlockers(t, tasks) {
			continue
		}
		if !matchesTags(t, opts.TagFilter) {
			continue
		}
		if !matchesPriority(t, opts.PriorityFilter) {
			continue
		}
		out = append(out, t)
	}
	return out
}

// matchesTags reports whether t carries every requested tag (AND); a leading
// # on a filter is ignored.
func matchesTags(t Task, filters []string) bool {
	for _, filter := range filters {
		filter = strings.TrimPrefix(filter, "#")
		found := false
		for _, tag := range t.Tags {
			if tag == filter {
				found = true
				break
			}
		}
		if !found {
			return false
		}
	}
	return true
}

// matchesPriority reports whether t's effective priority is any of the
// requested priorities (OR); a leading ! on a filter is ignored.
func matchesPriority(t Task, filters []string) bool {
	if len(filters) == 0 {
		return true
	}
	priority := effectivePriority(t.Priority)
	for _, filter := range filters {
		if effectivePriority(strings.TrimPrefix(filter, "!")) == priority {
			return true
		}
	}
	return false
}

// effectivePriority maps an absent or unknown priority to medium and keeps the
// three explicit ranks.
func effectivePriority(priority string) string {
	switch priority {
	case "crit", "high", "low":
		return priority
	default:
		return "medium"
	}
}

// sortByPriority stably orders crit, high, medium, low; unknown values sort as
// medium.
func sortByPriority(tasks []Task) {
	rank := map[string]int{"crit": 0, "high": 1, "medium": 2, "low": 3}
	sort.SliceStable(tasks, func(i, j int) bool {
		return rank[effectivePriority(tasks[i].Priority)] < rank[effectivePriority(tasks[j].Priority)]
	})
}

// jsonTask is the SPEC §F.1 JSON object. Field order is the required key order.
type jsonTask struct {
	ID         string            `json:"id"`
	Title      string            `json:"title"`
	Status     string            `json:"status"`
	Priority   string            `json:"priority"`
	Tags       []string          `json:"tags"`
	Properties map[string]string `json:"properties"`
	File       string            `json:"file"`
	Line       int               `json:"line"`
}

// renderJSON writes the indented JSON array. SetEscapeHTML(false) keeps a title
// such as <b>&</b> literal; tags and properties are always present as [] and {}
// respectively.
func renderJSON(w io.Writer, all, visible []Task) error {
	enc := json.NewEncoder(w)
	enc.SetEscapeHTML(false)
	enc.SetIndent("", "  ")

	out := make([]jsonTask, 0, len(visible))
	for _, t := range visible {
		tags := make([]string, 0, len(t.Tags))
		for _, tag := range t.Tags {
			tags = append(tags, sanitize(tag))
		}
		keys, values := DisplayProperties(t, all)
		props := make(map[string]string, len(keys))
		for _, key := range keys {
			props[sanitize(key)] = sanitize(values[key])
		}
		status := "open"
		if t.Checked {
			status = "done"
		}
		out = append(out, jsonTask{
			ID:         t.ID,
			Title:      sanitize(t.Title),
			Status:     status,
			Priority:   effectivePriority(t.Priority),
			Tags:       tags,
			Properties: props,
			File:       t.File,
			Line:       t.Line,
		})
	}
	return enc.Encode(out)
}

// renderLines writes the non-TTY line format of SPEC §F.2.
func renderLines(w io.Writer, all, visible []Task) {
	for _, t := range visible {
		checkbox := "[ ]"
		if t.Checked {
			checkbox = "[x]"
		}
		line := checkbox + " " + t.ID + "  " + sanitize(t.Title)

		if len(t.Tags) > 0 {
			tags := make([]string, len(t.Tags))
			for i, tag := range t.Tags {
				tags[i] = "#" + sanitize(tag)
			}
			line += "  " + strings.Join(tags, " ")
		}
		if t.Priority != "" {
			line += "  !" + sanitize(t.Priority)
		}
		keys, values := DisplayProperties(t, all)
		for _, key := range keys {
			line += "  @" + sanitize(key) + ":" + sanitize(values[key])
		}
		fmt.Fprintln(w, line)
	}
}

// tableHeaders is the column order of the TTY box table.
var tableHeaders = []string{"ID", "TITLE", "TAGS", "PRI", "PROPS"}

// renderTable draws the rounded box table of SPEC §F.3.
func renderTable(w io.Writer, all, visible []Task, term Terminal) {
	color := term.IsTTY && !term.NoColor

	rows := make([][]string, 0, len(visible))
	for _, t := range visible {
		rows = append(rows, tableRow(t, all))
	}

	widths := make([]int, len(tableHeaders))
	for i, header := range tableHeaders {
		widths[i] = lipgloss.Width(header)
	}
	for _, row := range rows {
		for i, cell := range row {
			if width := lipgloss.Width(cell); width > widths[i] {
				widths[i] = width
			}
		}
	}

	fmt.Fprintln(w, tableBorder(widths, "╭", "┬", "╮"))
	fmt.Fprintln(w, tableRowLine(tableHeaders, widths))
	fmt.Fprintln(w, tableBorder(widths, "├", "┼", "┤"))
	for i, row := range rows {
		if color {
			row = styleRow(visible[i], row)
		}
		fmt.Fprintln(w, tableRowLine(row, widths))
	}
	fmt.Fprintln(w, tableBorder(widths, "╰", "┴", "╯"))
}

// tableRow renders one task's raw (unstyled) cell values.
func tableRow(t Task, all []Task) []string {
	tags := make([]string, len(t.Tags))
	for i, tag := range t.Tags {
		tags[i] = "#" + sanitize(tag)
	}
	priority := ""
	if t.Priority != "" {
		priority = "!" + sanitize(t.Priority)
	}
	keys, values := DisplayProperties(t, all)
	props := make([]string, len(keys))
	for i, key := range keys {
		props[i] = "@" + sanitize(key) + ":" + sanitize(values[key])
	}
	return []string{
		sanitize(t.ID),
		truncateCells(sanitize(t.Title), 120),
		strings.Join(tags, " "),
		priority,
		strings.Join(props, " "),
	}
}

// styleRow applies the SPEC §G SGR colors to a raw row. Done rows use the dim
// color for the ID and title; tags, priority, and properties keep their own
// colors, and an unresolved blocked_by property is highlighted.
func styleRow(t Task, row []string) []string {
	styled := make([]string, len(row))
	copy(styled, row)

	if t.Checked {
		styled[0] = sgr("38;5;240", row[0])
		styled[1] = sgr("38;5;240", row[1])
	} else {
		styled[0] = sgr("38;5;245", row[0])
	}
	if row[2] != "" {
		styled[2] = sgr("38;5;62", row[2])
	}
	if row[3] != "" {
		styled[3] = sgr("38;5;212", row[3])
	}
	if strings.Contains(row[4], "@blocked_by:") {
		styled[4] = sgr("38;5;203", row[4])
	}
	return styled
}

// sgr wraps text in one SGR span.
func sgr(params, text string) string {
	return "\x1b[" + params + "m" + text + "\x1b[0m"
}

// tableBorder renders a horizontal rule with the given corner and join runes.
func tableBorder(widths []int, left, mid, right string) string {
	var b strings.Builder
	b.WriteString(left)
	for i, width := range widths {
		if i > 0 {
			b.WriteString(mid)
		}
		b.WriteString(strings.Repeat("─", width+2))
	}
	b.WriteString(right)
	return b.String()
}

// tableRowLine renders one "│ cell │ cell │" line, padding each cell to its
// column width.
func tableRowLine(cells []string, widths []int) string {
	var b strings.Builder
	b.WriteString("│")
	for i, cell := range cells {
		pad := widths[i] - lipgloss.Width(cell)
		if pad < 0 {
			pad = 0
		}
		b.WriteString(" ")
		b.WriteString(cell)
		b.WriteString(strings.Repeat(" ", pad))
		b.WriteString(" │")
	}
	return b.String()
}

// truncateCells truncates s to at most max display cells, keeping the first
// max-1 cells and appending "…" when it overflows.
func truncateCells(s string, max int) string {
	if lipgloss.Width(s) <= max {
		return s
	}
	var b strings.Builder
	width := 0
	for _, r := range s {
		w := lipgloss.Width(string(r))
		if width+w > max-1 {
			break
		}
		b.WriteRune(r)
		width += w
	}
	b.WriteString("…")
	return b.String()
}

// renderNotes writes the two SPEC §H notes to stdout, unprefixed.
func renderNotes(w io.Writer, opts ListOptions) {
	if len(opts.HiddenBlocked) > 0 {
		raws := make([]string, len(opts.HiddenBlocked))
		maxRaw := 0
		for i, t := range opts.HiddenBlocked {
			raws[i] = sanitize(t.HeaderRaw)
			if width := lipgloss.Width(raws[i]); width > maxRaw {
				maxRaw = width
			}
		}
		fmt.Fprintf(w, "Warning: %d blocked task(s) hidden (use `mdfu task list --blocked` to show them):\n", len(opts.HiddenBlocked))
		for i, t := range opts.HiddenBlocked {
			pad := maxRaw - lipgloss.Width(raws[i]) + 2
			if pad < 0 {
				pad = 0
			}
			fmt.Fprintf(w, "%s%s%s:%d\n", raws[i], strings.Repeat(" ", pad), t.File, t.Line)
		}
	}
	if len(opts.Unidentified) > 0 {
		fmt.Fprintf(w, "%d task(s) have no ID (run `mdfu task ids` to assign one).\n", len(opts.Unidentified))
	}
}

// sanitize is the single place invalid UTF-8 is neutralized, at the render
// layer only.
func sanitize(s string) string {
	return strings.ToValidUTF8(s, "\uFFFD")
}
