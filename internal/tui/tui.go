package tui

import (
	"fmt"
	"strings"

	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"

	"github.com/anomalyco/mdfu/internal/model"
)

// Item is the TUI's view of a searchable document.
// It intentionally mirrors only model.Document + score so the TUI
// depends solely on model + stdlib + bubbletea libs.
// Real filtering/scoring wires up at integration time via FilterFunc.
type Item struct {
	Doc   *model.Document
	Score float64
}

// FilterFunc narrows items by query. It is a hook so the real
// search implementation can be injected at integration time without
// the TUI importing internal/search or internal/query.
type FilterFunc func(query string, items []Item) []Item

// Config controls TUI behaviour.
type Config struct {
	Limit        int
	ShowArchived bool
	Preview      bool
}

// globalFilter allows integration code to wire the real search filter
// without changing the Run signature:
//
//	tui.SetFilter(searchFilter)
//	tui.Run(items, cfg)
var globalFilter FilterFunc

// SetFilter installs a process-wide filter used by NewModel/Run when no
// explicit filter is provided. Pass nil to restore default filtering.
func SetFilter(f FilterFunc) {
	globalFilter = f
}

// Model is the BubbleTea model for the picker. It is kept pure enough
// for headless unit tests: drive it with tea.KeyMsg values via Update.
type Model struct {
	items     []Item
	filtered  []Item
	filter    FilterFunc
	input     textinput.Model
	cursor    int
	offset    int
	selected  map[string]bool
	showPrev  bool
	showArch  bool
	limit     int
	width     int
	height    int
	confirmed bool
	aborted   bool
}

// NewModel builds a Model with default (substring) filtering.
func NewModel(items []Item, cfg Config) Model {
	return NewModelWithFilter(items, cfg, globalFilter)
}

// NewModelWithFilter builds a Model with an explicit filter hook.
// A nil filter falls back to default substring filtering.
func NewModelWithFilter(items []Item, cfg Config, f FilterFunc) Model {
	ti := textinput.New()
	ti.Placeholder = "Search... (bare words fuzzy, key:value hard-filter)"
	ti.Prompt = "> "
	ti.CharLimit = 500
	// Width is adjusted on WindowSizeMsg; pick a sane default for headless use.
	ti.Width = 50
	// Focus without needing a TTY for tests; ignore the blink command here
	// (Run/Init will request it again when running interactively).
	_ = ti.Focus()

	if f == nil {
		f = defaultFilter
	}
	cp := make([]Item, len(items))
	copy(cp, items)
	m := Model{
		items:    cp,
		filter:   f,
		input:    ti,
		selected: make(map[string]bool),
		showPrev: cfg.Preview,
		showArch: cfg.ShowArchived,
		limit:    cfg.Limit,
	}
	m.refilter()
	return m
}

// Init implements tea.Model.
func (m Model) Init() tea.Cmd {
	return textinput.Blink
}

// Update implements tea.Model. Keybindings:
// up/down or ctrl-k/ctrl-j navigate, enter confirm, esc/ctrl-c abort,
// tab toggle multi-select, ctrl-a toggle archived, ctrl-p toggle preview.
// All other keys go to the text input and trigger a refilter on change.
func (m Model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		if msg.Width > 10 {
			w := msg.Width - 4
			if w < 10 {
				w = 10
			}
			m.input.Width = w
		}
		m.clampCursor()
		return m, nil
	case tea.KeyMsg:
		switch msg.String() {
		case "up", "ctrl+k":
			if len(m.filtered) > 0 {
				if m.cursor > 0 {
					m.cursor--
				} else {
					// wrap to bottom for convenience
					m.cursor = len(m.filtered) - 1
				}
				m.ensureVisible()
			}
			return m, nil
		case "down", "ctrl+j":
			if len(m.filtered) > 0 {
				if m.cursor < len(m.filtered)-1 {
					m.cursor++
				} else {
					// wrap to top
					m.cursor = 0
				}
				m.ensureVisible()
			}
			return m, nil
		case "enter":
			m.confirmed = true
			return m, tea.Quit
		case "esc", "ctrl+c":
			m.aborted = true
			return m, tea.Quit
		case "tab":
			if len(m.filtered) > 0 && m.cursor >= 0 && m.cursor < len(m.filtered) {
				k := itemKey(m.filtered[m.cursor])
				if m.selected[k] {
					delete(m.selected, k)
				} else {
					m.selected[k] = true
				}
			}
			return m, nil
		case "ctrl+a":
			m.showArch = !m.showArch
			m.refilter()
			return m, nil
		case "ctrl+p":
			m.showPrev = !m.showPrev
			return m, nil
		}
		// Fall through to text input for typing/editing keys.
		prev := m.input.Value()
		var cmd tea.Cmd
		m.input, cmd = m.input.Update(msg)
		if m.input.Value() != prev {
			m.refilter()
		}
		return m, cmd
	}
	// Let the cursor blink etc. flow to the input.
	var cmd tea.Cmd
	m.input, cmd = m.input.Update(msg)
	return m, cmd
}

// View implements tea.Model.
func (m Model) View() string {
	var b strings.Builder
	b.WriteString(m.input.View())
	b.WriteString("\n")
	b.WriteString(m.renderList())
	if m.showPrev {
		b.WriteString("\n")
		b.WriteString(m.renderPreviewPane())
	}
	b.WriteString("\n")
	b.WriteString(m.renderStatus())
	return b.String()
}

// --- accessors for tests / integration ---

// Query returns the current query text.
func (m *Model) Query() string { return m.input.Value() }

// SetQuery sets the query text programmatically (headless-friendly) and refilters.
func (m *Model) SetQuery(q string) {
	m.input.SetValue(q)
	m.refilter()
}

// CursorIndex returns the cursor position within FilteredItems.
func (m *Model) CursorIndex() int { return m.cursor }

// FilteredItems returns the currently visible (filtered) items.
func (m *Model) FilteredItems() []Item { return m.filtered }

// AllItems returns all items.
func (m *Model) AllItems() []Item { return m.items }

// SelectedItems returns multi-selected items in filtered-display order
// (plus any selected items hidden by the current filter, appended in
// original order).
func (m *Model) SelectedItems() []Item {
	var out []Item
	seen := make(map[string]bool)
	for _, it := range m.filtered {
		k := itemKey(it)
		if m.selected[k] {
			out = append(out, it)
			seen[k] = true
		}
	}
	for _, it := range m.items {
		k := itemKey(it)
		if m.selected[k] && !seen[k] {
			out = append(out, it)
		}
	}
	return out
}

// Result returns the final selection: multi-selection if non-empty,
// else the cursor item, else nil.
func (m *Model) Result() []Item {
	if sel := m.SelectedItems(); len(sel) > 0 {
		return sel
	}
	if m.confirmed && len(m.filtered) > 0 && m.cursor >= 0 && m.cursor < len(m.filtered) {
		return []Item{m.filtered[m.cursor]}
	}
	return nil
}

// IsAborted reports whether the user aborted (esc/ctrl-c).
func (m *Model) IsAborted() bool { return m.aborted }

// IsConfirmed reports whether the user confirmed (enter).
func (m *Model) IsConfirmed() bool { return m.confirmed }

// ShowArchived reports archived visibility.
func (m *Model) ShowArchived() bool { return m.showArch }

// ShowPreview reports preview visibility.
func (m *Model) ShowPreview() bool { return m.showPrev }

// --- internal filtering ---

func (m *Model) refilter() {
	query := m.input.Value()
	var base []Item
	if m.filter != nil {
		base = m.filter(query, m.items)
	} else {
		base = defaultFilter(query, m.items)
	}
	if base == nil {
		base = []Item{}
	}
	// Hide archived unless toggled visible.
	if !m.showArch {
		kept := base[:0:0]
		// allocate only when needed
		out := make([]Item, 0, len(base))
		for _, it := range base {
			if it.Doc != nil && it.Doc.Archived != nil && *it.Doc.Archived {
				continue
			}
			out = append(out, it)
		}
		_ = kept
		base = out
	}
	if m.limit > 0 && len(base) > m.limit {
		base = base[:m.limit]
	}
	m.filtered = base
	m.clampCursor()
	m.ensureVisible()
}

func (m *Model) clampCursor() {
	if len(m.filtered) == 0 {
		m.cursor = 0
		m.offset = 0
		return
	}
	if m.cursor < 0 {
		m.cursor = 0
	}
	if m.cursor >= len(m.filtered) {
		m.cursor = len(m.filtered) - 1
	}
	if m.offset < 0 {
		m.offset = 0
	}
	if m.offset >= len(m.filtered) {
		m.offset = len(m.filtered) - 1
	}
}

func (m *Model) ensureVisible() {
	maxRows := m.visibleRows()
	if m.cursor < m.offset {
		m.offset = m.cursor
	}
	if m.cursor >= m.offset+maxRows {
		m.offset = m.cursor - maxRows + 1
	}
	if m.offset < 0 {
		m.offset = 0
	}
}

func (m *Model) visibleRows() int {
	if m.height > 0 {
		// reserve: input(2) + status(1) + preview(~8 if shown) + margins
		reserved := 4
		if m.showPrev {
			reserved += 9
		}
		n := m.height - reserved
		if n < 5 {
			n = 5
		}
		if n > 100 {
			n = 100
		}
		return n
	}
	return 20
}

func itemKey(it Item) string {
	if it.Doc != nil {
		if it.Doc.Path != "" {
			return "path:" + it.Doc.Path
		}
		return fmt.Sprintf("ptr:%p", it.Doc)
	}
	return fmt.Sprintf("nil:%v", it.Score)
}

// defaultFilter is a stdlib-only AND-substring fallback used until the real
// search filter is wired at integration time. Empty query returns all items.
func defaultFilter(query string, items []Item) []Item {
	q := strings.TrimSpace(query)
	if q == "" {
		out := make([]Item, len(items))
		copy(out, items)
		return out
	}
	toks := strings.Fields(strings.ToLower(q))
	out := make([]Item, 0, len(items))
	for _, it := range items {
		blob := itemBlob(it)
		ok := true
		for _, t := range toks {
			if !strings.Contains(blob, t) {
				ok = false
				break
			}
		}
		if ok {
			out = append(out, it)
		}
	}
	return out
}

func itemBlob(it Item) string {
	if it.Doc == nil {
		return ""
	}
	var b strings.Builder
	b.WriteString(strings.ToLower(it.Doc.Title))
	b.WriteString("\n")
	b.WriteString(strings.ToLower(it.Doc.Path))
	b.WriteString("\n")
	b.WriteString(strings.ToLower(it.Doc.DocType))
	b.WriteString("\n")
	b.WriteString(strings.ToLower(it.Doc.SearchBlob))
	b.WriteString("\n")
	for _, t := range it.Doc.Tags {
		b.WriteString(strings.ToLower(t))
		b.WriteString("\n")
	}
	return b.String()
}

// --- rendering ---

var (
	styleCursor   = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("212"))
	styleSelected = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("82"))
	styleDim      = lipgloss.NewStyle().Faint(true)
	styleTitle    = lipgloss.NewStyle().Bold(true)
	styleStatus   = lipgloss.NewStyle().Faint(true)
	stylePreviewH = lipgloss.NewStyle().Bold(true).Underline(true)
)

func (m Model) renderList() string {
	if len(m.filtered) == 0 {
		return styleDim.Render("(no matches)")
	}
	maxRows := m.visibleRows()
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
	var b strings.Builder
	for i := start; i < end; i++ {
		it := m.filtered[i]
		cursor := "  "
		if i == m.cursor {
			cursor = styleCursor.Render("> ")
		}
		sel := "  "
		if m.selected[itemKey(it)] {
			sel = styleSelected.Render("[x]")
		} else {
			sel = "[ ]"
		}
		title := "(untitled)"
		path := ""
		extra := ""
		if it.Doc != nil {
			if it.Doc.Title != "" {
				title = it.Doc.Title
			} else if it.Doc.Path != "" {
				title = it.Doc.Path
			}
			path = it.Doc.Path
			if it.Doc.DocType != "" {
				extra = " [" + it.Doc.DocType + "]"
			}
			if it.Doc.Archived != nil && *it.Doc.Archived {
				extra += " (archived)"
			}
		}
		line := fmt.Sprintf("%s%s %s%s", cursor, sel, title, extra)
		if i == m.cursor {
			line = styleCursor.Render(line)
		}
		b.WriteString(line)
		if path != "" && path != title {
			b.WriteString("  " + styleDim.Render(path))
		}
		if i < end-1 {
			b.WriteString("\n")
		}
	}
	if end < len(m.filtered) {
		b.WriteString("\n" + styleDim.Render(fmt.Sprintf("... %d more", len(m.filtered)-end)))
	}
	return b.String()
}

func (m Model) renderPreviewPane() string {
	if len(m.filtered) == 0 || m.cursor < 0 || m.cursor >= len(m.filtered) {
		return styleDim.Render("(no preview)")
	}
	it := m.filtered[m.cursor]
	if it.Doc == nil {
		return styleDim.Render("(no preview)")
	}
	text := PreviewText(it.Doc)
	// Side-by-side when wide enough, stacked otherwise.
	if m.width >= 100 {
		listW := m.width/2 - 2
		prevW := m.width - listW - 4
		if listW < 20 {
			listW = 20
		}
		if prevW < 20 {
			prevW = 20
		}
		left := lipgloss.NewStyle().Width(listW).Render(m.renderList())
		right := lipgloss.NewStyle().Width(prevW).Render(text)
		return lipgloss.JoinHorizontal(lipgloss.Top, left, "  ", right)
	}
	return lipgloss.NewStyle().Render(text)
}

func (m Model) renderStatus() string {
	arch := "hidden"
	if m.showArch {
		arch = "shown"
	}
	s := fmt.Sprintf("%d/%d • archived:%s • tab:multi • enter:select", len(m.filtered), len(m.items), arch)
	return styleStatus.Render(s)
}

// PreviewText renders Path/Title/Type/Tags plus a body excerpt of the
// first 30 lines. Dependency-free (stdlib only) — no glow.
func PreviewText(doc *model.Document) string {
	if doc == nil {
		return "(no preview)"
	}
	var b strings.Builder
	b.WriteString(stylePreviewH.Render("Preview"))
	b.WriteString("\n")
	b.WriteString(styleTitle.Render("Title: ") + doc.Title + "\n")
	b.WriteString("Path: " + doc.Path + "\n")
	if doc.DocType != "" {
		b.WriteString("Type: " + doc.DocType + "\n")
	}
	if len(doc.Tags) > 0 {
		b.WriteString("Tags: " + strings.Join(doc.Tags, ", ") + "\n")
	}
	if doc.Status != "" {
		b.WriteString("Status: " + doc.Status + "\n")
	}
	b.WriteString("---\n")
	b.WriteString(BodyExcerpt(doc.Body, 30))
	return b.String()
}

// BodyExcerpt returns the first n lines of body.
func BodyExcerpt(body string, n int) string {
	if n <= 0 {
		return ""
	}
	// Normalize CRLF so line counting is stable.
	body = strings.ReplaceAll(body, "\r\n", "\n")
	lines := strings.Split(body, "\n")
	if len(lines) > n {
		lines = lines[:n]
	}
	return strings.Join(lines, "\n")
}

// Run launches the interactive picker and returns the selection.
// Enter confirms (multi-selection if any, else cursor item).
// Esc/Ctrl-C aborts and returns (nil, nil).
func Run(items []Item, cfg Config) ([]Item, error) {
	return RunWithFilter(items, cfg, globalFilter)
}

// RunWithFilter is like Run but with an explicit filter hook.
// A nil filter uses the default substring filter. Integration wires the
// real search filter here.
func RunWithFilter(items []Item, cfg Config, f FilterFunc) ([]Item, error) {
	if f == nil {
		f = globalFilter
	}
	m := NewModelWithFilter(items, cfg, f)
	p := tea.NewProgram(m, tea.WithAltScreen())
	final, err := p.Run()
	if err != nil {
		return nil, err
	}
	fm, ok := final.(Model)
	if !ok {
		// bubbletea may return a pointer in some versions; handle both.
		if fmp, ok2 := any(final).(*Model); ok2 {
			fm = *fmp
		} else {
			return nil, fmt.Errorf("tui: unexpected model type %T", final)
		}
	}
	if fm.IsAborted() {
		return nil, nil
	}
	return fm.Result(), nil
}
