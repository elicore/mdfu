package tui

import (
	"reflect"
	"strings"
	"testing"

	"github.com/charmbracelet/x/ansi"

	"github.com/elicore/mdfu/internal/model"
	"github.com/elicore/mdfu/internal/theme"
)

// Given a preview over a document with frontmatter rows and a body
// When a component is hidden via SetVisible or Toggle
// Then only that component's rows disappear from the render.
func TestPreviewComponentVisibility(t *testing.T) {
	doc := &model.Document{
		Path:    "notes/a.md",
		Title:   "My Note",
		DocType: "Note",
		Body:    "the body text\n",
	}
	p := NewPreview(doc, theme.Default(), PreviewOptions{})

	plain := stripANSI(p.Render(80))
	for _, want := range []string{"a.md", "My Note", "Type: Note", "the body text"} {
		if !strings.Contains(plain, want) {
			t.Fatalf("full preview missing %q, got:\n%s", want, plain)
		}
	}

	p.SetVisible(ComponentFrontmatter, false)
	if p.Visible(ComponentFrontmatter) {
		t.Fatal("frontmatter should report hidden")
	}
	plain = stripANSI(p.Render(80))
	if strings.Contains(plain, "Type:") || strings.Contains(plain, "notes/a.md") {
		t.Fatalf("hidden frontmatter still renders, got:\n%s", plain)
	}
	for _, want := range []string{"a.md", "My Note", "the body text"} {
		if !strings.Contains(plain, want) {
			t.Fatalf("preview without frontmatter missing %q, got:\n%s", want, plain)
		}
	}

	if got := p.Toggle(ComponentBody); got {
		t.Fatal("Toggle should report the new hidden state")
	}
	plain = stripANSI(p.Render(80))
	if strings.Contains(plain, "the body text") {
		t.Fatalf("hidden body still renders, got:\n%s", plain)
	}
	if !strings.Contains(plain, "a.md") || !strings.Contains(plain, "My Note") {
		t.Fatalf("filename and title must remain, got:\n%s", plain)
	}
	if got := p.Toggle(ComponentBody); !got || !p.Visible(ComponentBody) {
		t.Fatal("Toggle should report the restored visible state")
	}
	if p.Toggle("nope") || p.Visible("nope") {
		t.Fatal("unknown component IDs must report false")
	}

	wantIDs := []string{ComponentFilename, ComponentTitle, ComponentFrontmatter, ComponentBody}
	if got := p.IDs(); !reflect.DeepEqual(got, wantIDs) {
		t.Fatalf("IDs = %v, want %v", got, wantIDs)
	}
}

// Given a rendered preview
// Then the first line is the document basename, and the old "Preview"
// heading and "---" separator are gone.
func TestPreviewChrome(t *testing.T) {
	doc := &model.Document{Path: "notes/a.md", Title: "My Note", Body: "some body\n"}
	out := stripANSI(stripOSC8(previewTextHighlighted(doc, 80, nil, true)))
	lines := strings.Split(out, "\n")
	if lines[0] != "a.md" {
		t.Fatalf("first line = %q, want the basename %q", lines[0], "a.md")
	}
	for _, l := range lines {
		if l == "Preview" {
			t.Fatalf("old Preview heading still rendered, got:\n%s", out)
		}
		if l == "---" {
			t.Fatalf("old --- separator still rendered, got:\n%s", out)
		}
	}
}

// Given a document with empty Path, Title, and Body (and a nil document)
// Then rendering must not panic and still returns a string.
func TestPreviewEmptyDocument(t *testing.T) {
	if got := NewPreview(&model.Document{}, theme.Default(), PreviewOptions{}).Render(80); got != "(untitled)" {
		t.Fatalf("empty document render = %q, want %q", got, "(untitled)")
	}
	if got := previewTextHighlighted(nil, 80, nil, false); got != "(no preview)" {
		t.Fatalf("nil document render = %q, want %q", got, "(no preview)")
	}
}

// Given the OKF v0.2 metric fixture rendered through the component Preview
// Then the frontmatter panel shows the normalized rows, JSON-object values
// flattened inline, the sources object list joined by "; ", tags as pills,
// and the retained full Path row — and sources never renders as pills.
func TestPreviewFrontmatterPanel(t *testing.T) {
	forceANSIColors(t)
	doc := parseFixture(t, "../../testdata/okf-v02-metric.md")
	th := theme.Default()
	raw := NewPreview(doc, th, PreviewOptions{}).Render(80)
	panel := stripANSI(raw)

	for _, want := range []string{
		"Type: Metric",
		"generated: by: data-pipeline, at: 2024-06-01T10:00:00Z",
		"verified: by: alice, at: 2024-06-02T12:30:00Z",
	} {
		if !strings.Contains(panel, want) {
			t.Errorf("panel missing %q\n---\n%s", want, panel)
		}
	}

	lines := strings.Split(panel, "\n")
	rawLines := strings.Split(raw, "\n")
	lineByPrefix := func(ls []string, prefix string) string {
		for _, l := range ls {
			if strings.Contains(l, prefix) {
				return l
			}
		}
		return ""
	}

	tagsLine := lineByPrefix(lines, "Tags:")
	if tagsLine == "" {
		t.Fatalf("no Tags row in panel\n---\n%s", panel)
	}
	for _, tag := range []string{"growth", "kpi", "monthly"} {
		if !strings.Contains(tagsLine, tag) {
			t.Errorf("Tags row missing %q: %q", tag, tagsLine)
		}
	}

	sourcesLine := lineByPrefix(lines, "sources:")
	if sourcesLine == "" {
		t.Fatalf("no sources row in panel\n---\n%s", panel)
	}
	i1 := strings.Index(sourcesLine, "id: warehouse")
	i2 := strings.Index(sourcesLine, "id: dashboard")
	sep := strings.Index(sourcesLine, "; ")
	if i1 < 0 || i2 < 0 {
		t.Fatalf("sources row missing element ids: %q", sourcesLine)
	}
	if sep < i1 || sep > i2 {
		t.Errorf("object list elements must be joined by \"; \" between the ids: %q", sourcesLine)
	}

	pathLine := lineByPrefix(lines, "Path:")
	if !strings.HasPrefix(pathLine, "Path: ") || !strings.HasSuffix(pathLine, "testdata/okf-v02-metric.md") {
		t.Errorf("retained Path row = %q, want the full fixture path", pathLine)
	}

	probe := th.Pill.Render("probe")
	var seqs []string
	for _, s := range ansiRe.FindAllString(probe, -1) {
		if s != "\x1b[0m" {
			seqs = append(seqs, s)
		}
	}
	if len(seqs) == 0 {
		t.Fatal("pill probe emitted no color SGR sequences; color profile was not forced")
	}
	sourcesRaw := lineByPrefix(rawLines, "sources:")
	tagsRaw := lineByPrefix(rawLines, "Tags:")
	for _, s := range seqs {
		if strings.Contains(sourcesRaw, s) {
			t.Errorf("sources row must not render as pills; found pill SGR %q in %q", s, sourcesRaw)
		}
		if !strings.Contains(tagsRaw, s) {
			t.Errorf("sanity check: the Tags row of the same panel carries the pill SGR %q", s)
		}
	}
}

// Given the OKF v0.2 metric preview
// When the frontmatter component is hidden
// Then every labeled row, including the retained Path row, disappears while
// the filename, title, and body remain.
func TestPreviewFrontmatterPanelHidden(t *testing.T) {
	doc := parseFixture(t, "../../testdata/okf-v02-metric.md")
	p := NewPreview(doc, theme.Default(), PreviewOptions{})
	p.SetVisible(ComponentFrontmatter, false)
	panel := stripANSI(p.Render(80))

	for _, gone := range []string{"Path:", "Type:", "Status:", "Description:", "Tags:", "Created:", "Updated:", "generated:", "sources:", "verified:"} {
		if strings.Contains(panel, gone) {
			t.Errorf("hidden frontmatter still renders the %q row:\n%s", gone, panel)
		}
	}
	for _, want := range []string{"okf-v02-metric.md", "Monthly Active Users", "MAU grew 12%"} {
		if !strings.Contains(panel, want) {
			t.Errorf("preview without frontmatter missing %q\n---\n%s", want, panel)
		}
	}
}

// Given a document whose Resource is a web link
// When the preview renders with hyperlinks on and a query matching the URL
// Then the frontmatter panel emits no link marker runes, the Resource row is
// a clean OSC 8 hyperlink (the query highlight must not corrupt the link
// target), and with hyperlinks off no OSC 8 sequence appears at all.
func TestPreviewFrontmatterPanelHyperlinks(t *testing.T) {
	doc := &model.Document{
		Path:     "notes/a.md",
		Title:    "Note",
		Resource: "https://example.com/r",
		Body:     "Body without links.\n",
	}
	re := termsRegexp([]string{"example"})

	out := NewPreview(doc, theme.Default(), PreviewOptions{Highlight: re, Hyperlinks: true}).Render(80)
	for _, r := range out {
		if r >= linkStartBase && r <= linkEndRune {
			t.Fatalf("frontmatter preview emitted link marker rune U+%04X:\n%q", r, out)
		}
	}
	if !strings.Contains(out, ansi.SetHyperlink("https://example.com/r")) {
		t.Fatalf("Resource hyperlink target corrupted by query highlighting, got:\n%q", out)
	}
	visible := stripANSI(stripOSC8(out))
	if !strings.Contains(visible, "Resource: https://example.com/r") {
		t.Fatalf("expected the visible Resource row, got:\n%q", visible)
	}

	plain := NewPreview(doc, theme.Default(), PreviewOptions{Hyperlinks: false}).Render(80)
	if strings.Contains(plain, "\x1b]8;;") {
		t.Fatalf("hyperlinks off must emit no OSC 8 sequence, got:\n%q", plain)
	}
	if !strings.Contains(stripANSI(plain), "Resource: https://example.com/r") {
		t.Fatalf("hyperlinks off must keep the Resource target visible, got:\n%q", plain)
	}
}

// Given a theme with non-default highlight and link SGRs
// When rendering a preview with a query match and a body link
// Then the emitted bytes carry the theme's SGRs, not the defaults.
func TestPreviewUsesThemeSGR(t *testing.T) {
	doc := &model.Document{
		Path:  "notes/a.md",
		Title: "parser notes",
		Body:  "See [Charm parser](https://charm.sh) now.\n",
	}
	re := termsRegexp([]string{"parser"})
	th := theme.Default()
	th.HighlightSGR = "7;30;102"
	th.LinkSGR = "4;38;5;81"

	out := NewPreview(doc, th, PreviewOptions{Highlight: re, Hyperlinks: true}).Render(80)
	if !strings.Contains(out, "\x1b[7;30;102m") {
		t.Fatalf("expected the custom highlight SGR, got:\n%q", out)
	}
	if !strings.Contains(out, "\x1b[4;38;5;81m") {
		t.Fatalf("expected the custom link SGR, got:\n%q", out)
	}
	def := theme.Default()
	if strings.Contains(out, "\x1b["+def.HighlightSGR+"m") {
		t.Fatalf("default highlight SGR leaked into a themed render, got:\n%q", out)
	}
	if strings.Contains(out, "\x1b["+def.LinkSGR+"m") {
		t.Fatalf("default link SGR leaked into a themed render, got:\n%q", out)
	}
}
