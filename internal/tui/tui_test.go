package tui

import (
	"fmt"
	"strings"
	"testing"

	tea "github.com/charmbracelet/bubbletea"

	"github.com/elicore/mdfu/internal/model"
)

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
	txt := PreviewText(doc)
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
