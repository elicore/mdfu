package tui

import (
	"reflect"
	"strings"
	"testing"

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
