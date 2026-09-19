package tui

import (
	"fmt"
	"regexp"
	"sort"
	"strings"
	"sync"
	"unicode"

	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/glamour"
	"github.com/charmbracelet/lipgloss"

	"github.com/elicore/mdfu/internal/model"
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
	// terms are the literal substrings derived from the current query and
	// emphasized in the list and preview panes; matchRe is their compiled
	// case-insensitive alternation (nil when there is nothing to highlight).
	terms   []string
	matchRe *regexp.Regexp
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
//
// The rendered height is kept within the terminal: the list and preview pane
// never render the list twice, and both are capped so the search input at the
// top cannot be scrolled off-screen.
func (m Model) View() string {
	var b strings.Builder
	b.WriteString(m.input.View())
	b.WriteString("\n")
	if m.showPrev && m.width >= 100 {
		// Wide layout: list and preview side by side (renderPreviewPane
		// renders the list itself, so it must not be emitted again here).
		b.WriteString(m.renderPreviewPane())
	} else {
		b.WriteString(m.renderList())
		if m.showPrev {
			b.WriteString("\n")
			b.WriteString(m.renderPreviewPane())
		}
	}
	b.WriteString("\n")
	b.WriteString(m.renderStatus())

	// Final safety net: hard-clamp to the terminal so no line wraps and the
	// total height cannot exceed the screen (which would scroll the search
	// input off the top).
	out := b.String()
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
	return out
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
	// Remember which document the cursor was on before the list is rebuilt so a
	// query change can keep the same document selected instead of silently
	// re-pointing the cursor at whichever document now occupies that row.
	prevKey := ""
	if m.cursor >= 0 && m.cursor < len(m.filtered) {
		prevKey = itemKey(m.filtered[m.cursor])
	}

	query := m.input.Value()
	m.terms = queryTerms(query)
	m.matchRe = termsRegexp(m.terms)
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

	// Re-anchor the cursor: follow the same document into the new result set
	// when it survives the change, otherwise start over at the top.
	if prevKey != "" {
		if idx := indexOfKey(base, prevKey); idx >= 0 {
			m.cursor = idx
		} else {
			m.cursor = 0
			m.offset = 0
		}
	}
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

// visibleRows returns how many result rows the list may render. The preview
// pane only competes for vertical space in the stacked (narrow) layout; in the
// side-by-side layout both panes share the full body budget.
func (m *Model) visibleRows() int {
	if m.height <= 0 {
		return 20
	}
	reserved := 2 // input + status
	if m.showPrev && m.width < 100 {
		reserved += m.previewRows()
	}
	n := m.height - reserved
	if n < 3 {
		n = 3
	}
	return n
}

// rowWidth returns the horizontal budget for a result row. In the wide
// side-by-side layout the list only owns the left half of the screen.
func (m Model) rowWidth() int {
	if m.width <= 0 {
		return 100
	}
	if m.showPrev && m.width >= 100 {
		w := m.width/2 - 2
		if w < 20 {
			w = 20
		}
		return w
	}
	return m.width
}

// previewRows returns the vertical budget for the stacked preview pane.
func (m *Model) previewRows() int {
	if m.height <= 0 {
		return 10
	}
	p := (m.height - 2) / 3
	if p < 3 {
		p = 3
	}
	if p > 12 {
		p = 12
	}
	return p
}

// limitLines truncates s to at most n lines.
func limitLines(s string, n int) string {
	if n <= 0 {
		return ""
	}
	lines := strings.Split(s, "\n")
	if len(lines) > n {
		lines = lines[:n]
	}
	return strings.Join(lines, "\n")
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

// indexOfKey returns the position of the first item with the given key, or -1
// when no item matches.
func indexOfKey(items []Item, key string) int {
	for i := range items {
		if itemKey(items[i]) == key {
			return i
		}
	}
	return -1
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
	// Reserve the final row for the "... N more" hint so the list never
	// renders taller than its budget.
	more := end < len(m.filtered)
	if more && maxRows > 1 {
		end--
	}
	multi := len(m.selected) > 0
	var b strings.Builder
	for i := start; i < end; i++ {
		it := m.filtered[i]
		cursor := "  "
		if i == m.cursor {
			cursor = styleCursor.Render("> ")
		}
		// The per-row checkbox column is only meaningful (and shown) once
		// multi-select is in use; until then it is visual noise.
		var sel string
		if multi {
			if m.selected[itemKey(it)] {
				sel = styleSelected.Render("[x]")
			} else {
				sel = "[ ]"
			}
		}
		title := "(untitled)"
		path := ""
		extra := ""
		snippet := ""
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
		rawTitle := title
		showPath := path != "" && path != rawTitle
		// When the query only matches the body, the visible fields carry no
		// highlight; surface a highlighted excerpt of the match so the list
		// still explains why the row ranked. Size it to the width left after
		// the title so the matched text is never truncated away.
		if it.Doc != nil && m.matchRe != nil &&
			!matchesAny(m.matchRe, title, path, extra, it.Doc.Description, strings.Join(it.Doc.Tags, " ")) {
			prefix := 2 // cursor
			if multi {
				prefix += 4 // separator + checkbox
			}
			sw := m.rowWidth() - prefix - lipgloss.Width(title) - lipgloss.Width(extra) - 4
			if sw < 24 {
				sw = 24
			}
			if sw > 80 {
				sw = 80
			}
			snippet = bodySnippet(it.Doc.Body, m.matchRe, sw)
		}
		title = highlightRe(title, m.matchRe)
		path = highlightRe(path, m.matchRe)
		extra = highlightRe(extra, m.matchRe)
		snippet = highlightRe(snippet, m.matchRe)
		var line string
		if multi {
			line = fmt.Sprintf("%s%s %s%s", cursor, sel, title, extra)
		} else {
			line = fmt.Sprintf("%s%s%s", cursor, title, extra)
		}
		if snippet != "" {
			// The snippet owns the remaining row width, so the path (which the
			// preview still shows) yields rather than truncating the match.
			line += "  " + snippet
			showPath = false
		}
		if i == m.cursor {
			line = styleCursor.Render(line)
		}
		b.WriteString(line)
		if showPath {
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
		text := previewTextHighlighted(it.Doc, prevW, m.matchRe)
		// MaxWidth (not Width) truncates the list lines so they cannot wrap
		// and inflate the pane height. The preview is allowed to wrap, but is
		// capped at the same row budget.
		left := lipgloss.NewStyle().MaxWidth(listW).Render(m.renderList())
		right := lipgloss.NewStyle().Width(prevW).MaxWidth(prevW).MaxHeight(m.visibleRows()).Render(limitLines(text, m.visibleRows()))
		return lipgloss.JoinHorizontal(lipgloss.Top, left, "  ", right)
	}
	text := previewTextHighlighted(it.Doc, m.width, m.matchRe)
	return lipgloss.NewStyle().Render(limitLines(text, m.previewRows()))
}

func (m Model) renderStatus() string {
	arch := "hidden"
	if m.showArch {
		arch = "shown"
	}
	s := fmt.Sprintf("%d/%d • archived:%s • tab:multi • enter:select", len(m.filtered), len(m.items), arch)
	return styleStatus.Render(s)
}

// PreviewText renders the default-width preview: frontmatter fields plus a
// syntax-highlighted markdown excerpt of the first 30 body lines.
func PreviewText(doc *model.Document) string {
	return PreviewTextWidth(doc, 80)
}

// PreviewTextWidth is PreviewText rendered to fit width terminal columns.
// The metadata header is drawn with lipgloss; the body excerpt is rendered
// as highlighted markdown via glamour.
func PreviewTextWidth(doc *model.Document, width int) string {
	return PreviewTextHighlighted(doc, width, nil)
}

// PreviewTextHighlighted is PreviewTextWidth with query match highlighting.
// Terms are emphasized in both the metadata header and the rendered markdown
// body; existing glamour/lipgloss styling is preserved.
func PreviewTextHighlighted(doc *model.Document, width int, terms []string) string {
	return previewTextHighlighted(doc, width, termsRegexp(terms))
}

// previewTextHighlighted is PreviewTextWidth with a precompiled match regexp.
func previewTextHighlighted(doc *model.Document, width int, re *regexp.Regexp) string {
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
	if doc.Description != "" {
		b.WriteString("Description: " + doc.Description + "\n")
	}
	if len(doc.Tags) > 0 {
		b.WriteString("Tags: " + strings.Join(doc.Tags, ", ") + "\n")
	}
	if doc.Status != "" {
		b.WriteString("Status: " + doc.Status + "\n")
	}
	b.WriteString("---\n")
	b.WriteString(renderMarkdown(matchWindow(doc.Body, re, 30), width))
	return highlightRe(b.String(), re)
}

// Match highlighting. Matches are shown as black text on a bright-yellow
// background. highlight works on text that already contains SGR sequences
// (lipgloss/glamour output): escape sequences pass through untouched and the
// active style is restored after each emphasized span.

const (
	hlStart = "\x1b[1;30;103m"
	hlReset = "\x1b[0m"
)

// ansiSGRRe matches CSI Select-Graphic-Rendition sequences.
var ansiSGRRe = regexp.MustCompile(`\x1b\[[0-9;]*m`)

// queryTerms extracts the literal substrings worth emphasizing from a raw
// query. It is deliberately syntax-light: bare words and key:value values are
// kept, while date expressions and negated tags are skipped.
func queryTerms(q string) []string {
	var terms []string
	add := func(t string) {
		t = strings.TrimSpace(t)
		if t == "" || !hasWordRune(t) {
			return
		}
		terms = append(terms, t)
		// A multi-word value (e.g. a quoted phrase) is also matched word by
		// word so it can be emphasized even when markdown splits the phrase
		// across styled runs.
		if fields := strings.Fields(t); len(fields) > 1 {
			for _, f := range fields {
				terms = append(terms, f)
			}
		}
	}
	for _, tok := range splitQueryTokens(q) {
		tok = strings.TrimSpace(tok)
		if tok == "" {
			continue
		}
		i := strings.Index(tok, ":")
		if i < 0 {
			add(stripWrappingQuotes(tok))
			continue
		}
		key := strings.ToLower(strings.TrimSpace(tok[:i]))
		val := tok[i+1:]
		switch key {
		case "created", "updated", "date", "before", "after":
			continue
		case "tag", "tags":
			for _, p := range strings.Split(val, ",") {
				p = stripWrappingQuotes(strings.TrimSpace(p))
				if p == "" || strings.HasPrefix(p, "-") || strings.HasPrefix(p, "!") {
					continue
				}
				add(p)
			}
		default:
			add(stripWrappingQuotes(strings.TrimSpace(val)))
		}
	}
	return terms
}

// hasWordRune reports whether s contains at least one letter or digit, so
// punctuation-only query fragments are not treated as highlight terms.
func hasWordRune(s string) bool {
	for _, r := range s {
		if unicode.IsLetter(r) || unicode.IsDigit(r) {
			return true
		}
	}
	return false
}

// splitQueryTokens splits on whitespace while respecting double quotes.
func splitQueryTokens(input string) []string {
	var tokens []string
	var cur strings.Builder
	inQuotes := false
	flush := func() {
		if cur.Len() > 0 {
			tokens = append(tokens, cur.String())
			cur.Reset()
		}
	}
	for _, r := range input {
		switch {
		case r == '"':
			inQuotes = !inQuotes
			cur.WriteRune(r)
		case unicode.IsSpace(r) && !inQuotes:
			flush()
		default:
			cur.WriteRune(r)
		}
	}
	flush()
	return tokens
}

// stripWrappingQuotes removes one pair of surrounding double quotes.
func stripWrappingQuotes(s string) string {
	s = strings.TrimSpace(s)
	if len(s) >= 2 && strings.HasPrefix(s, `"`) && strings.HasSuffix(s, `"`) {
		return s[1 : len(s)-1]
	}
	return s
}

// termsRegexp builds a case-insensitive alternation of the literal terms,
// longest first so the most specific match wins. Returns nil when there is
// nothing to highlight.
func termsRegexp(terms []string) *regexp.Regexp {
	seen := make(map[string]bool, len(terms))
	uniq := make([]string, 0, len(terms))
	for _, t := range terms {
		t = strings.TrimSpace(t)
		if t == "" {
			continue
		}
		lt := strings.ToLower(t)
		if seen[lt] {
			continue
		}
		seen[lt] = true
		uniq = append(uniq, t)
	}
	if len(uniq) == 0 {
		return nil
	}
	// Longest first so the most specific alternative wins.
	sort.Slice(uniq, func(i, j int) bool { return len(uniq[i]) > len(uniq[j]) })
	quoted := make([]string, len(uniq))
	for i, t := range uniq {
		quoted[i] = regexp.QuoteMeta(t)
	}
	re, err := regexp.Compile("(?i)(" + strings.Join(quoted, "|") + ")")
	if err != nil {
		return nil
	}
	return re
}

// highlight emphasizes terms in s, compiling their match regexp on the fly.
// Callers rendering many strings should prefer highlightRe with a cached regexp.
func highlight(s string, terms []string) string {
	return highlightRe(s, termsRegexp(terms))
}

// highlightRe emphasizes every occurrence matched by re in s. It is safe on
// text containing ANSI SGR sequences: they are preserved and the style active
// at the start of a span is re-applied after it ends.
func highlightRe(s string, re *regexp.Regexp) string {
	if re == nil || s == "" {
		return s
	}
	var out strings.Builder
	var active strings.Builder
	i := 0
	for i < len(s) {
		loc := ansiSGRRe.FindStringIndex(s[i:])
		if loc != nil && loc[0] == 0 {
			seq := s[i : i+loc[1]]
			out.WriteString(seq)
			updateActiveSGR(&active, seq)
			i += loc[1]
			continue
		}
		end := len(s)
		if loc != nil {
			end = i + loc[0]
		}
		writeHighlightedRun(&out, s[i:end], re, active.String())
		i = end
	}
	return out.String()
}

// writeHighlightedRun wraps term matches inside a single unstyled run (a span
// free of ANSI escapes) and restores active afterwards.
func writeHighlightedRun(out *strings.Builder, run string, re *regexp.Regexp, active string) {
	if run == "" {
		return
	}
	idxs := re.FindAllStringIndex(run, -1)
	if len(idxs) == 0 {
		out.WriteString(run)
		return
	}
	last := 0
	for _, m := range idxs {
		out.WriteString(run[last:m[0]])
		out.WriteString(hlStart)
		out.WriteString(run[m[0]:m[1]])
		out.WriteString(hlReset)
		out.WriteString(active)
		last = m[1]
	}
	out.WriteString(run[last:])
}

// updateActiveSGR tracks the cumulative SGR state so it can be re-applied
// after an emphasized span. A reset clears the accumulated state.
func updateActiveSGR(active *strings.Builder, seq string) {
	params := strings.TrimSuffix(strings.TrimPrefix(seq, "\x1b["), "m")
	reset := params == "" || params == "0"
	if !reset {
		for _, p := range strings.Split(params, ";") {
			if p == "0" {
				reset = true
				break
			}
		}
	}
	if reset {
		active.Reset()
		if params == "" || params == "0" {
			return
		}
	}
	active.WriteString(seq)
}

// previewRenderers caches one glamour renderer per word-wrap width; building a
// renderer is far more expensive than rendering a 30-line excerpt, and the pane
// width is stable while browsing.
var (
	previewRenderers = map[int]*glamour.TermRenderer{}
	previewRenderMu  sync.Mutex
	// markdownStyle is the glamour standard style ("dark" or "light"). It is
	// resolved once before the BubbleTea program starts so View never probes
	// the terminal (which would race with BubbleTea's input reader). Empty
	// falls back to "dark", which keeps headless rendering deterministic.
	markdownStyle string
)

// SetMarkdownStyle records the terminal background style used for markdown
// previews and drops any cached renderers. Call it before Run for light
// terminals; the default is dark.
func SetMarkdownStyle(style string) {
	previewRenderMu.Lock()
	if style != "light" {
		style = "dark"
	}
	markdownStyle = style
	previewRenderers = map[int]*glamour.TermRenderer{}
	previewRenderMu.Unlock()
}

// renderMarkdown converts a markdown excerpt to ANSI-highlighted text wrapped
// to width. On any error it falls back to the raw source so previews never
// disappear.
func renderMarkdown(src string, width int) string {
	if width < 20 {
		width = 20
	}
	// glamour's default document style indents each line by two columns
	// outside the word-wrap budget; subtract it so the rendered pane never
	// exceeds the width the caller allocated.
	wrap := width - 2
	if wrap < 10 {
		wrap = 10
	}
	previewRenderMu.Lock()
	r := previewRenderers[wrap]
	if r == nil {
		style := markdownStyle
		if style == "" {
			style = "dark"
		}
		nr, err := glamour.NewTermRenderer(
			glamour.WithStandardStyle(style),
			glamour.WithWordWrap(wrap),
			glamour.WithPreservedNewLines(),
		)
		if err != nil {
			previewRenderMu.Unlock()
			return src
		}
		r = nr
		previewRenderers[wrap] = r
	}
	previewRenderMu.Unlock()

	out, err := r.Render(src)
	if err != nil {
		return src
	}
	// The renderer frames the document with blank lines; drop the leading one
	// so the excerpt sits directly under the metadata separator.
	return strings.Trim(out, "\n")
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

// matchWindow returns an excerpt of at most n body lines that contains the
// first match of re, so the preview emphasizes a hit even when it lives well
// past the leading lines. Without a match (or a nil re) it degenerates to the
// leading excerpt. Ellipsis markers flag skipped leading/trailing lines.
func matchWindow(body string, re *regexp.Regexp, n int) string {
	if n <= 0 {
		return ""
	}
	body = strings.ReplaceAll(body, "\r\n", "\n")
	lines := strings.Split(body, "\n")
	if re == nil || len(lines) <= n {
		return BodyExcerpt(body, n)
	}
	loc := re.FindStringIndex(body)
	if loc == nil {
		return BodyExcerpt(body, n)
	}
	matchLine := strings.Count(body[:loc[0]], "\n")
	start := matchLine - 2
	if start < 0 {
		start = 0
	}
	// Cap the tail rather than sliding the window back: if the match sits near
	// the end of the document, keeping it near the top of the excerpt is what
	// makes it visible once the preview height is clamped.
	end := start + n
	if end > len(lines) {
		end = len(lines)
	}
	out := strings.Join(lines[start:end], "\n")
	if start > 0 {
		out = "…\n" + out
	}
	if end < len(lines) {
		out += "\n…"
	}
	return out
}

// bodySnippet returns a single-line excerpt of body centered on the first
// match of re, trimmed with ellipsis markers, or "" when there is no match.
// It gives the result list something concrete to highlight for body-only hits.
func bodySnippet(body string, re *regexp.Regexp, width int) string {
	if re == nil || width <= 0 {
		return ""
	}
	s := strings.Join(strings.Fields(body), " ")
	if s == "" {
		return ""
	}
	loc := re.FindStringIndex(s)
	if loc == nil {
		return ""
	}
	r := []rune(s)
	// Keep only a little lead-in so the match stays well inside the row even
	// when the title eats most of the available width.
	lead := width / 4
	if lead > 12 {
		lead = 12
	}
	start := len([]rune(s[:loc[0]])) - lead
	if start < 0 {
		start = 0
	}
	end := start + width
	if end > len(r) {
		end = len(r)
		start = end - width
		if start < 0 {
			start = 0
		}
	}
	out := string(r[start:end])
	if start > 0 {
		out = "…" + out
	}
	if end < len(r) {
		out += "…"
	}
	return out
}

// matchesAny reports whether re matches any of the non-empty strings.
func matchesAny(re *regexp.Regexp, ss ...string) bool {
	if re == nil {
		return false
	}
	for _, s := range ss {
		if s != "" && re.MatchString(s) {
			return true
		}
	}
	return false
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
	// Resolve the markdown preview theme while the terminal is still in cooked
	// mode; probing from inside View would race BubbleTea's input reader.
	if lipgloss.HasDarkBackground() {
		SetMarkdownStyle("dark")
	} else {
		SetMarkdownStyle("light")
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
