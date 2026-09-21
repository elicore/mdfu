package tui

import (
	"fmt"
	"regexp"
	"strings"
	"testing"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/x/ansi"

	"github.com/elicore/mdfu/internal/model"
)

var ansiRe = regexp.MustCompile(`\x1b\[[0-9;]*m`)

// osc8Re matches OSC 8 hyperlink open/close sequences.
var osc8Re = regexp.MustCompile(`\x1b]8;;[^\x07]*\x07`)

// stripANSI removes SGR sequences so tests can assert on the visible text of
// glamour-highlighted previews.
func stripANSI(s string) string { return ansiRe.ReplaceAllString(s, "") }

// stripOSC8 removes terminal hyperlink sequences.
func stripOSC8(s string) string { return osc8Re.ReplaceAllString(s, "") }

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

// A query change that keeps the selected document in the results must leave
// the cursor on that document, even when its row index moves.
func TestCursorTracksDocumentAcrossQueryChange(t *testing.T) {
	items := []Item{
		mkItem("a.md", "alpha"),
		mkItem("b.md", "beta"),
		mkItem("g.md", "gamma"),
		mkItem("d.md", "delta"),
	}
	m := NewModelWithFilter(items, Config{}, substringStub)
	m = applyKey(m, key(tea.KeyDown)) // beta
	m = applyKey(m, key(tea.KeyDown)) // gamma
	m = applyKey(m, key(tea.KeyDown)) // delta
	if got := m.FilteredItems()[m.CursorIndex()].Doc.Path; got != "d.md" {
		t.Fatalf("precondition: expected cursor on d.md, got %q", got)
	}

	m.SetQuery("ta") // matches beta and delta, dropping alpha and gamma
	if got := m.FilteredItems(); len(got) != 2 || got[0].Doc.Path != "b.md" || got[1].Doc.Path != "d.md" {
		t.Fatalf("expected [b.md d.md] after query, got %v", got)
	}
	if m.CursorIndex() != 1 {
		t.Fatalf("expected cursor on d.md at index 1, got %d", m.CursorIndex())
	}
	if got := m.FilteredItems()[m.CursorIndex()].Doc.Path; got != "d.md" {
		t.Fatalf("expected cursor to track d.md, got %q", got)
	}
}

// When the selected document is filtered out entirely, the cursor resets to
// the top of the new list rather than clamping to a different document.
func TestCursorResetsWhenDocumentFilteredOut(t *testing.T) {
	items := []Item{
		mkItem("a.md", "alpha"),
		mkItem("b.md", "beta"),
		mkItem("g.md", "gamma"),
		mkItem("d.md", "delta"),
	}
	m := NewModelWithFilter(items, Config{}, substringStub)
	m = applyKey(m, key(tea.KeyDown)) // beta
	m = applyKey(m, key(tea.KeyDown)) // gamma
	if got := m.FilteredItems()[m.CursorIndex()].Doc.Path; got != "g.md" {
		t.Fatalf("precondition: expected cursor on g.md, got %q", got)
	}

	m.SetQuery("ta") // beta and delta survive, gamma does not
	if m.CursorIndex() != 0 {
		t.Fatalf("expected cursor reset to 0, got %d", m.CursorIndex())
	}
	if got := m.FilteredItems()[0].Doc.Path; got != "b.md" {
		t.Fatalf("expected top item b.md, got %q", got)
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

func TestFrontmatterToggle(t *testing.T) {
	it := Item{Doc: &model.Document{
		Path:    "a.md",
		Title:   "alpha",
		DocType: "Note",
		Tags:    []string{"foo", "bar"},
		Body:    "body of alpha",
	}}
	m := NewModelWithFilter([]Item{it}, Config{Preview: true}, substringStub)
	if !m.ShowFrontmatter() {
		t.Fatal("expected frontmatter shown by default")
	}
	v := stripANSI(stripOSC8(m.View()))
	if !strings.Contains(v, "Type:") || !strings.Contains(v, "Tags:") {
		t.Fatalf("expected frontmatter rows in view, got:\n%s", v)
	}
	m = applyKey(m, tea.KeyMsg{Type: tea.KeyCtrlF})
	if m.ShowFrontmatter() {
		t.Fatal("expected frontmatter hidden after ctrl+f")
	}
	v = stripANSI(stripOSC8(m.View()))
	if strings.Contains(v, "Type:") || strings.Contains(v, "Tags:") {
		t.Fatalf("expected frontmatter rows gone after ctrl+f, got:\n%s", v)
	}
	if !strings.Contains(v, "fm:hidden") {
		t.Fatalf("expected fm:hidden in status, got:\n%s", v)
	}
	m = applyKey(m, tea.KeyMsg{Type: tea.KeyCtrlF})
	if !m.ShowFrontmatter() {
		t.Fatal("expected frontmatter shown after second ctrl+f")
	}
	v = stripANSI(stripOSC8(m.View()))
	if !strings.Contains(v, "Type:") || !strings.Contains(v, "Tags:") {
		t.Fatalf("expected frontmatter rows restored, got:\n%s", v)
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
	if !strings.Contains(v, "fm:shown") {
		t.Fatalf("expected fm:shown in view, got:\n%s", v)
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

func TestPreviewHidesLinkURLBehindOSC8(t *testing.T) {
	doc := &model.Document{
		Path:  "notes/a.md",
		Title: "Note",
		Body:  "See [Charm](https://charm.sh) for more.\n",
	}
	out := previewTextHighlighted(doc, 80, nil, true)
	if !strings.Contains(out, ansi.SetHyperlink("https://charm.sh")) {
		t.Fatalf("expected OSC 8 hyperlink, got:\n%q", out)
	}
	if !strings.Contains(out, ansi.ResetHyperlink()) {
		t.Fatalf("expected OSC 8 hyperlink to close, got:\n%q", out)
	}
	visible := stripANSI(stripOSC8(out))
	if !strings.Contains(visible, "Charm") {
		t.Fatalf("expected link label visible, got:\n%q", visible)
	}
	if strings.Contains(visible, "https://charm.sh") {
		t.Fatalf("expected raw URL hidden, got:\n%q", visible)
	}
}

func TestPreviewShowsLinkURLWithoutHyperlinks(t *testing.T) {
	doc := &model.Document{
		Path:  "notes/a.md",
		Title: "Note",
		Body:  "See [Charm](https://charm.sh) for more.\n",
	}
	out := previewTextHighlighted(doc, 80, nil, false)
	if strings.Contains(out, "\x1b]8;;") {
		t.Fatalf("expected no OSC 8 hyperlink, got:\n%q", out)
	}
	visible := stripANSI(stripOSC8(out))
	if !strings.Contains(visible, "Charm") || !strings.Contains(visible, "https://charm.sh") {
		t.Fatalf("expected label and URL visible, got:\n%q", visible)
	}
}

func TestPreviewRelativeLinkIsPlainLabel(t *testing.T) {
	doc := &model.Document{
		Path:  "notes/a.md",
		Title: "Note",
		Body:  "See [other](notes/other.md) for more.\n",
	}
	out := previewTextHighlighted(doc, 80, nil, true)
	if strings.Contains(out, "\x1b]8;;") {
		t.Fatalf("relative link should not be clickable, got:\n%q", out)
	}
	visible := stripANSI(stripOSC8(out))
	if !strings.Contains(visible, "other") || strings.Contains(visible, "notes/other.md") {
		t.Fatalf("expected only the label, got:\n%q", visible)
	}
}

func TestPreviewKeepsBalancedParensInLinkTarget(t *testing.T) {
	doc := &model.Document{
		Path:  "notes/a.md",
		Title: "Note",
		Body:  "See [docs](https://example.com/a_(b)) now.\n",
	}
	out := previewTextHighlighted(doc, 80, nil, true)
	if !strings.Contains(out, ansi.SetHyperlink("https://example.com/a_(b)")) {
		t.Fatalf("expected full destination with balanced parens, got:\n%q", out)
	}
	if got := docFirstLink(doc); got != "https://example.com/a_(b)" {
		t.Fatalf("docFirstLink = %q, want full URL", got)
	}
}

func TestPreviewLeavesCodeUntouched(t *testing.T) {
	doc := &model.Document{
		Path:  "notes/a.md",
		Title: "Note",
		Body:  "Inline `[x](https://inline.example)`.\n\n```\n[y](https://fenced.example)\n```\n",
	}
	out := previewTextHighlighted(doc, 80, nil, true)
	if strings.Contains(out, "\x1b]8;;") {
		t.Fatalf("code links must stay literal, got:\n%q", out)
	}
	visible := stripANSI(stripOSC8(out))
	if !strings.Contains(visible, "https://inline.example") || !strings.Contains(visible, "https://fenced.example") {
		t.Fatalf("expected code content preserved, got:\n%q", visible)
	}
}

func TestPreviewResourceIsClickable(t *testing.T) {
	doc := &model.Document{
		Path:     "notes/a.md",
		Title:    "Note",
		Resource: "https://example.com/r",
		Body:     "Body without links.\n",
	}
	out := previewTextHighlighted(doc, 80, nil, true)
	if !strings.Contains(out, ansi.SetHyperlink("https://example.com/r")) {
		t.Fatalf("expected Resource hyperlink, got:\n%q", out)
	}
	visible := stripANSI(stripOSC8(out))
	if !strings.Contains(visible, "Resource: https://example.com/r") {
		t.Fatalf("expected Resource row, got:\n%q", visible)
	}
}

func TestPreviewHighlightsLinkLabelWithoutBreakingHyperlink(t *testing.T) {
	doc := &model.Document{
		Path:  "notes/a.md",
		Title: "Note",
		Body:  "See [Charm parser](https://charm.sh).\n",
	}
	out := previewTextHighlighted(doc, 80, termsRegexp([]string{"parser"}), true)
	if !strings.Contains(out, ansi.SetHyperlink("https://charm.sh")) {
		t.Fatalf("expected OSC 8 hyperlink, got:\n%q", out)
	}
	if !strings.Contains(out, hlStart) {
		t.Fatalf("expected match highlight, got:\n%q", out)
	}
	if !strings.Contains(out, hlReset) {
		t.Fatalf("expected highlight to reset, got:\n%q", out)
	}
	visible := stripANSI(stripOSC8(out))
	if !strings.Contains(visible, "Charm parser") || strings.Contains(visible, "https://charm.sh") {
		t.Fatalf("expected highlighted label and hidden URL, got:\n%q", visible)
	}
}

func TestDocFirstLink(t *testing.T) {
	if got := docFirstLink(nil); got != "" {
		t.Fatalf("nil doc: got %q", got)
	}
	withResource := &model.Document{Resource: "https://res.example", Body: "[b](https://body.example)"}
	if got := docFirstLink(withResource); got != "https://res.example" {
		t.Fatalf("resource: got %q", got)
	}
	bodyOnly := &model.Document{Body: "text [b](https://body.example) more"}
	if got := docFirstLink(bodyOnly); got != "https://body.example" {
		t.Fatalf("body: got %q", got)
	}
	none := &model.Document{Resource: "notes/x.md", Body: "no links"}
	if got := docFirstLink(none); got != "" {
		t.Fatalf("no link: got %q", got)
	}
}

func TestExtractAndHideSkipsImages(t *testing.T) {
	src := "![diagram](https://img.example/d_(1).png) then [doc](https://doc.example)\n"
	out, links := extractAndHide(src, true)
	if len(links) != 1 || links[0].URL != "https://doc.example" {
		t.Fatalf("expected only the real link, got %+v", links)
	}
	if !strings.Contains(out, "![diagram](https://img.example/d_(1).png)") {
		t.Fatalf("image construct must pass through unchanged, got %q", out)
	}
	if got := docFirstLink(&model.Document{Body: src}); got != "https://doc.example" {
		t.Fatalf("docFirstLink = %q, want the body link", got)
	}
}

func TestCloseOpenHyperlinks(t *testing.T) {
	open := "before " + ansi.SetHyperlink("https://charm.sh") + "Charm"
	if got := closeOpenHyperlinks(open); !strings.HasSuffix(got, ansi.ResetHyperlink()) {
		t.Fatalf("expected dangling hyperlink to be closed, got %q", got)
	}
	closed := open + ansi.ResetHyperlink()
	if got := closeOpenHyperlinks(closed); got != closed {
		t.Fatalf("already-closed hyperlink changed: %q", got)
	}
	if got := closeOpenHyperlinks("no links here"); got != "no links here" {
		t.Fatalf("plain text changed: %q", got)
	}
}

func TestRenderPreviewClosesTruncatedHyperlink(t *testing.T) {
	body := "start [" + strings.Repeat("label ", 200) + "](https://charm.sh) end\n"
	doc := &model.Document{Path: "a.md", Title: "Note", Body: body}
	m := NewModelWithFilter([]Item{{Doc: doc}}, Config{Preview: true}, substringStub)
	next, _ := m.Update(tea.WindowSizeMsg{Width: 80, Height: 40})
	m = next.(Model)
	p := m.renderPreviewPane()
	if !strings.Contains(p, ansi.SetHyperlink("https://charm.sh")) {
		t.Fatalf("expected opener retained across truncation, got:\n%q", p)
	}
	if got := closeOpenHyperlinks(p); got != p {
		t.Fatalf("preview leaves a dangling hyperlink:\n%q", p)
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

func TestViewKeepsHyperlink(t *testing.T) {
	doc := &model.Document{Path: "a.md", Title: "Note", Body: "See [Charm](https://charm.sh).\n"}
	m := NewModelWithFilter([]Item{{Doc: doc}}, Config{Preview: true}, substringStub)
	next, _ := m.Update(tea.WindowSizeMsg{Width: 120, Height: 40})
	m = next.(Model)
	v := m.View()
	if !strings.Contains(v, ansi.SetHyperlink("https://charm.sh")) {
		t.Fatalf("expected hyperlink in rendered view, got:\n%q", v)
	}
	if strings.Contains(stripANSI(stripOSC8(v)), "https://charm.sh") {
		t.Fatalf("expected URL hidden in rendered view, got:\n%q", v)
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
