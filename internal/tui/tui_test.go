package tui

import (
	"fmt"
	"regexp"
	"strings"
	"testing"

	tea "github.com/charmbracelet/bubbletea"

	"github.com/elicore/mdfu/internal/model"
)

var ansiRe = regexp.MustCompile(`\x1b\[[0-9;]*m`)

// stripANSI removes SGR sequences so tests can assert on the visible text of
// glamour-highlighted previews.
func stripANSI(s string) string { return ansiRe.ReplaceAllString(s, "") }

func boolPtr(b bool) *bool { return &b }

func mkItem(path, title string) Item {
	return Item{Doc: &model.Document{Path: path, Title: title, Body: "body of " + title}}
}

// substringStub is an injected FilterFunc stub: case-insensitive
// substring match on title, mimicking the real search hook.
func substringStub(query string, items []Item) []Item {
	q := strings.TrimSpace(strings.ToLower(query))
	if q == "" {
		out := make([]Item, len(items))
		copy(out, items)
		return out
	}
	var out []Item
	for _, it := range items {
		if it.Doc == nil {
			continue
		}
		if strings.Contains(strings.ToLower(it.Doc.Title), q) {
			out = append(out, it)
		}
	}
	return out
}

func applyKey(m Model, k tea.KeyMsg) Model {
	next, _ := m.Update(k)
	nm, ok := next.(Model)
	if !ok {
		panic(fmt.Sprintf("expected Model, got %T", next))
	}
	return nm
}

func key(t tea.KeyType) tea.KeyMsg {
	return tea.KeyMsg{Type: t}
}

func runeKey(r rune) tea.KeyMsg {
	return tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune{r}}
}

func TestFilterNarrowsViaStub(t *testing.T) {
	items := []Item{mkItem("a.md", "alpha"), mkItem("b.md", "beta"), mkItem("c.md", "gamma")}
	m := NewModelWithFilter(items, Config{}, substringStub)
	if got := len(m.FilteredItems()); got != 3 {
		t.Fatalf("expected 3 items initially, got %d", got)
	}
	m.SetQuery("alpha")
	if got := len(m.FilteredItems()); got != 1 {
		t.Fatalf("expected 1 item after query, got %d", got)
	}
	if got := m.FilteredItems()[0].Doc.Title; got != "alpha" {
		t.Fatalf("expected alpha, got %q", got)
	}
	if q := m.Query(); q != "alpha" {
		t.Fatalf("expected query alpha, got %q", q)
	}
}

func TestTypingViaKeyMsgsNarrows(t *testing.T) {
	items := []Item{mkItem("a.md", "alpha"), mkItem("b.md", "beta"), mkItem("c.md", "gamma")}
	m := NewModelWithFilter(items, Config{}, substringStub)
	for _, r := range "alp" {
		m = applyKey(m, runeKey(r))
	}
	if q := m.Query(); q != "alp" {
		t.Fatalf("expected query alp, got %q", q)
	}
	if got := len(m.FilteredItems()); got != 1 {
		t.Fatalf("expected 1 filtered item, got %d (%v)", got, m.FilteredItems())
	}
}

func TestCursorMovement(t *testing.T) {
	items := []Item{mkItem("a.md", "alpha"), mkItem("b.md", "beta"), mkItem("c.md", "gamma")}
	m := NewModelWithFilter(items, Config{}, substringStub)
	if m.CursorIndex() != 0 {
		t.Fatalf("expected cursor 0, got %d", m.CursorIndex())
	}
	m = applyKey(m, key(tea.KeyDown))
	if m.CursorIndex() != 1 {
		t.Fatalf("expected cursor 1 after down, got %d", m.CursorIndex())
	}
	m = applyKey(m, tea.KeyMsg{Type: tea.KeyCtrlJ})
	if m.CursorIndex() != 2 {
		t.Fatalf("expected cursor 2 after ctrl+j, got %d", m.CursorIndex())
	}
	m = applyKey(m, key(tea.KeyUp))
	if m.CursorIndex() != 1 {
		t.Fatalf("expected cursor 1 after up, got %d", m.CursorIndex())
	}
	m = applyKey(m, tea.KeyMsg{Type: tea.KeyCtrlK})
	if m.CursorIndex() != 0 {
		t.Fatalf("expected cursor 0 after ctrl+k, got %d", m.CursorIndex())
	}
}

func TestToggleSelectMulti(t *testing.T) {
	items := []Item{mkItem("a.md", "alpha"), mkItem("b.md", "beta"), mkItem("c.md", "gamma")}
	m := NewModelWithFilter(items, Config{}, substringStub)
	// Select first item.
	m = applyKey(m, key(tea.KeyTab))
	if got := len(m.SelectedItems()); got != 1 {
		t.Fatalf("expected 1 selected, got %d", got)
	}
	// Move down and select second.
	m = applyKey(m, key(tea.KeyDown))
	m = applyKey(m, key(tea.KeyTab))
	if got := len(m.SelectedItems()); got != 2 {
		t.Fatalf("expected 2 selected, got %d", got)
	}
	// Toggle first off: move up, tab again.
	m = applyKey(m, key(tea.KeyUp))
	m = applyKey(m, key(tea.KeyTab))
	if got := len(m.SelectedItems()); got != 1 {
		t.Fatalf("expected 1 selected after toggle-off, got %d", got)
	}
	if got := m.SelectedItems()[0].Doc.Path; got != "b.md" {
		t.Fatalf("expected b.md selected, got %q", got)
	}
	// Confirm returns multi-selection.
	m = applyKey(m, key(tea.KeyEnter))
	if !m.IsConfirmed() {
		t.Fatal("expected confirmed")
	}
	res := m.Result()
	if len(res) != 1 || res[0].Doc.Path != "b.md" {
		t.Fatalf("unexpected result %v", res)
	}
}

func TestEnterSingleSelect(t *testing.T) {
	items := []Item{mkItem("a.md", "alpha"), mkItem("b.md", "beta")}
	m := NewModelWithFilter(items, Config{}, substringStub)
	m = applyKey(m, key(tea.KeyDown)) // cursor -> beta
	m = applyKey(m, key(tea.KeyEnter))
	res := m.Result()
	if len(res) != 1 || res[0].Doc.Title != "beta" {
		t.Fatalf("expected single beta result, got %v", res)
	}
}

func TestAbort(t *testing.T) {
	items := []Item{mkItem("a.md", "alpha")}
	m := NewModelWithFilter(items, Config{}, substringStub)
	m = applyKey(m, key(tea.KeyEsc))
	if !m.IsAborted() {
		t.Fatal("expected aborted on esc")
	}
	m2 := NewModelWithFilter(items, Config{}, substringStub)
	m2 = applyKey(m2, tea.KeyMsg{Type: tea.KeyCtrlC})
	if !m2.IsAborted() {
		t.Fatal("expected aborted on ctrl+c")
	}
}

func TestArchivedToggle(t *testing.T) {
	a := Item{Doc: &model.Document{Path: "a.md", Title: "alpha", Archived: boolPtr(false)}}
	b := Item{Doc: &model.Document{Path: "arch.md", Title: "archived note", Archived: boolPtr(true)}}
	c := Item{Doc: &model.Document{Path: "c.md", Title: "gamma"}}
	m := NewModelWithFilter([]Item{a, b, c}, Config{}, substringStub)
	if got := len(m.FilteredItems()); got != 2 {
		t.Fatalf("expected archived hidden (2 visible), got %d", got)
	}
	m = applyKey(m, tea.KeyMsg{Type: tea.KeyCtrlA})
	if !m.ShowArchived() {
		t.Fatal("expected archived shown after ctrl+a")
	}
	if got := len(m.FilteredItems()); got != 3 {
		t.Fatalf("expected 3 visible after toggle, got %d", got)
	}
	m = applyKey(m, tea.KeyMsg{Type: tea.KeyCtrlA})
	if m.ShowArchived() {
		t.Fatal("expected archived hidden after second toggle")
	}
	if got := len(m.FilteredItems()); got != 2 {
		t.Fatalf("expected 2 visible after toggle back, got %d", got)
	}
}

func TestPreviewToggle(t *testing.T) {
	items := []Item{mkItem("a.md", "alpha")}
	m := NewModelWithFilter(items, Config{Preview: false}, substringStub)
	if m.ShowPreview() {
		t.Fatal("expected preview off")
	}
	m = applyKey(m, tea.KeyMsg{Type: tea.KeyCtrlP})
	if !m.ShowPreview() {
		t.Fatal("expected preview on after ctrl+p")
	}
}

func TestStatusBarAndView(t *testing.T) {
	items := []Item{mkItem("a.md", "alpha"), mkItem("b.md", "beta")}
	m := NewModelWithFilter(items, Config{}, substringStub)
	v := m.View()
	if !strings.Contains(v, "2/2") {
		t.Fatalf("expected status N/M in view, got:\n%s", v)
	}
	if !strings.Contains(v, "archived:hidden") {
		t.Fatalf("expected archived:hidden in view, got:\n%s", v)
	}
	if !strings.Contains(v, "tab:multi") || !strings.Contains(v, "enter:select") {
		t.Fatalf("expected key hints in view, got:\n%s", v)
	}
}

func TestPreviewText(t *testing.T) {
	var body strings.Builder
	for i := 1; i <= 40; i++ {
		fmt.Fprintf(&body, "line %d\n", i)
	}
	doc := &model.Document{
		Path:    "notes/a.md",
		Title:   "My Note",
		DocType: "Note",
		Tags:    []string{"foo", "bar"},
		Body:    body.String(),
	}
	txt := stripANSI(PreviewText(doc))
	for _, want := range []string{"notes/a.md", "My Note", "Note", "foo", "bar", "line 1", "line 30"} {
		if !strings.Contains(txt, want) {
			t.Fatalf("expected preview to contain %q, got:\n%s", want, txt)
		}
	}
	if strings.Contains(txt, "line 31") {
		t.Fatalf("expected preview truncated to 30 lines, got:\n%s", txt)
	}
	if got := len(strings.Split(BodyExcerpt(body.String(), 30), "\n")); got != 30 {
		t.Fatalf("expected 30-line excerpt, got %d", got)
	}
}

func TestPreviewRendersHighlightedMarkdown(t *testing.T) {
	doc := &model.Document{
		Path:  "notes/a.md",
		Title: "Note",
		Body:  "# Heading\n\nSome **bold** text and `code`.\n",
	}
	out := PreviewText(doc)
	if !strings.Contains(out, "\x1b[") {
		t.Fatalf("expected ANSI highlighting in preview, got:\n%q", out)
	}
	plain := stripANSI(out)
	for _, raw := range []string{"# Heading", "**bold**", "`code`"} {
		if strings.Contains(plain, raw) {
			t.Fatalf("expected markdown syntax %q to be rendered, got:\n%s", raw, plain)
		}
	}
	for _, want := range []string{"Heading", "bold", "code"} {
		if !strings.Contains(plain, want) {
			t.Fatalf("expected %q in rendered preview, got:\n%s", want, plain)
		}
	}
}

func TestQueryTerms(t *testing.T) {
	got := queryTerms(`tag:launch,beta "ship mdfu" created:2024 bare tag:-skip`)
	want := map[string]bool{
		"launch": true, "beta": true, "ship mdfu": true, "ship": true,
		"mdfu": true, "bare": true,
	}
	for _, term := range got {
		if !want[term] {
			t.Fatalf("unexpected highlight term %q (got %v)", term, got)
		}
		delete(want, term)
	}
	if len(want) != 0 {
		t.Fatalf("missing highlight terms %v (got %v)", want, got)
	}
	for _, term := range got {
		if term == "2024" || term == "skip" {
			t.Fatalf("date/negated term should not be highlighted: %v", got)
		}
	}
}

func TestHighlightPreservesANSI(t *testing.T) {
	in := "\x1b[31mhello world\x1b[0m"
	got := highlight(in, []string{"world"})
	want := "\x1b[31mhello " + hlStart + "world" + hlReset + "\x1b[31m\x1b[0m"
	if got != want {
		t.Fatalf("highlight mismatch:\n got %q\nwant %q", got, want)
	}
}

func TestListHighlightsQueryMatch(t *testing.T) {
	items := []Item{mkItem("a.md", "alpha"), mkItem("b.md", "beta")}
	m := NewModelWithFilter(items, Config{}, substringStub)
	m.SetQuery("alp")
	v := m.View()
	if !strings.Contains(v, hlStart) {
		t.Fatalf("expected highlighted list match, got:\n%q", v)
	}
	if !strings.Contains(v, hlStart+"alp") {
		t.Fatalf("expected the matched query text to be emphasized, got:\n%q", v)
	}
}

func TestPreviewWindowsToBodyMatch(t *testing.T) {
	var body strings.Builder
	for i := 1; i <= 40; i++ {
		fmt.Fprintf(&body, "filler line %d\n", i)
	}
	body.WriteString("the deepterm appears here\n")
	doc := &model.Document{Path: "notes/a.md", Title: "Unrelated", Body: body.String()}
	out := PreviewTextHighlighted(doc, 80, []string{"deepterm"})
	if !strings.Contains(out, hlStart) {
		t.Fatalf("expected highlighted deep body match, got:\n%q", out)
	}
	plain := stripANSI(out)
	if !strings.Contains(plain, "deepterm") {
		t.Fatalf("expected preview to window onto the match, got:\n%s", plain)
	}
	if strings.Contains(plain, "filler line 1\n") {
		t.Fatalf("expected preview to skip leading lines before the match, got:\n%s", plain)
	}
}

func TestListShowsSnippetForBodyMatch(t *testing.T) {
	doc := &model.Document{
		Path:       "notes/a.md",
		Title:      "Unrelated",
		Body:       "line one\nline two\nthe deepterm appears here\n",
		SearchBlob: "line one line two the deepterm appears here",
	}
	m := NewModel([]Item{{Doc: doc}}, Config{})
	m.SetQuery("deepterm")
	v := m.View()
	if !strings.Contains(v, hlStart) {
		t.Fatalf("expected highlighted body snippet, got:\n%q", v)
	}
	if !strings.Contains(v, hlStart+"deepterm") {
		t.Fatalf("expected the matched body text to be emphasized, got:\n%q", v)
	}
}

func TestNoBodySnippetWhenTitleMatches(t *testing.T) {
	doc := &model.Document{
		Path:       "notes/a.md",
		Title:      "Deepterm Note",
		Body:       "nothing to see here\n",
		SearchBlob: "Deepterm Note nothing to see here",
	}
	m := NewModel([]Item{{Doc: doc}}, Config{})
	m.SetQuery("deepterm")
	if v := stripANSI(m.View()); strings.Contains(v, "nothing to see here") {
		t.Fatalf("expected no body snippet when the title already highlights, got:\n%s", v)
	}
}

func TestPreviewHighlightsQueryMatch(t *testing.T) {
	doc := &model.Document{
		Path:  "notes/a.md",
		Title: "Parser Notes",
		Body:  "Finish the parser track.\n",
	}
	out := PreviewTextHighlighted(doc, 80, []string{"parser"})
	if !strings.Contains(out, hlStart) {
		t.Fatalf("expected highlighted preview match, got:\n%q", out)
	}
	if plain := stripANSI(out); !strings.Contains(plain, "Parser") || !strings.Contains(plain, "parser") {
		t.Fatalf("expected matched text to remain visible, got:\n%s", plain)
	}
}

func TestCheckboxesOnlyWhenMultiSelecting(t *testing.T) {
	items := []Item{mkItem("a.md", "alpha"), mkItem("b.md", "beta")}
	m := NewModelWithFilter(items, Config{}, substringStub)
	if v := stripANSI(m.View()); strings.Contains(v, "[ ]") || strings.Contains(v, "[x]") {
		t.Fatalf("expected no checkboxes before multi-select, got:\n%s", v)
	}
	m = applyKey(m, key(tea.KeyTab))
	v := stripANSI(m.View())
	if !strings.Contains(v, "[x]") || !strings.Contains(v, "[ ]") {
		t.Fatalf("expected checkboxes once multi-select is active, got:\n%s", v)
	}
	// Deselecting the only item hides the column again.
	m = applyKey(m, key(tea.KeyTab))
	if v := stripANSI(m.View()); strings.Contains(v, "[ ]") || strings.Contains(v, "[x]") {
		t.Fatalf("expected checkboxes hidden after deselect, got:\n%s", v)
	}
}

func TestViewFitsTerminalHeight(t *testing.T) {
	long := &model.Document{Path: "a.md", Title: "alpha", Body: strings.Repeat("body line\n", 100)}
	items := []Item{{Doc: long}, mkItem("b.md", "beta")}
	m := NewModelWithFilter(items, Config{Preview: true}, substringStub)

	// Wide (side-by-side) layout must not duplicate the list or overflow,
	// which previously scrolled the search input off the top of the screen.
	next, _ := m.Update(tea.WindowSizeMsg{Width: 120, Height: 24})
	m = next.(Model)
	v := m.View()
	if got := len(strings.Split(v, "\n")); got > 24 {
		t.Fatalf("wide view = %d lines, want <= 24:\n%s", got, v)
	}
	if !strings.HasPrefix(v, "> ") {
		t.Fatalf("expected search input on first line, got:\n%s", v)
	}

	// Stacked (narrow) layout.
	next, _ = m.Update(tea.WindowSizeMsg{Width: 80, Height: 24})
	m = next.(Model)
	v = m.View()
	if got := len(strings.Split(v, "\n")); got > 24 {
		t.Fatalf("narrow view = %d lines, want <= 24:\n%s", got, v)
	}
	if !strings.HasPrefix(v, "> ") {
		t.Fatalf("expected search input on first line, got:\n%s", v)
	}
}

func TestLimit(t *testing.T) {
	items := []Item{mkItem("a.md", "alpha"), mkItem("b.md", "beta"), mkItem("c.md", "gamma")}
	m := NewModelWithFilter(items, Config{Limit: 2}, substringStub)
	if got := len(m.FilteredItems()); got != 2 {
		t.Fatalf("expected limit 2, got %d", got)
	}
}

func TestDefaultFilterEmptyQueryReturnsAll(t *testing.T) {
	items := []Item{mkItem("a.md", "alpha"), mkItem("b.md", "beta")}
	m := NewModel(items, Config{})
	if got := len(m.FilteredItems()); got != 2 {
		t.Fatalf("expected 2, got %d", got)
	}
}
