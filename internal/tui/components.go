// Preview components: the document preview is a fixed-order stack of
// instantiable, hideable blocks (filename, title, frontmatter, body), each
// rendering one section of a document with the theme's styles and SGRs.
package tui

import (
	"path/filepath"
	"regexp"
	"strings"

	"github.com/elicore/mdfu/internal/model"
	"github.com/elicore/mdfu/internal/theme"
)

// Component is one hideable block of the document preview.
type Component interface {
	ID() string
	Visible() bool
	SetVisible(bool)
	Render(width int) string
}

// Component IDs in fixed render order.
const (
	ComponentFilename    = "filename"
	ComponentTitle       = "title"
	ComponentFrontmatter = "frontmatter"
	ComponentBody        = "body"
)

// PreviewOptions carries the render settings shared by every component.
type PreviewOptions struct {
	Highlight  *regexp.Regexp
	Hyperlinks bool
}

// Preview composes the document preview components in fixed order.
type Preview struct {
	components []Component
	byID       map[string]Component
}

// NewPreview builds the preview stack for doc. Components capture doc, th,
// and opts at construction, so a Preview is created, hidden, or re-created
// per selected document.
func NewPreview(doc *model.Document, th theme.Theme, opts PreviewOptions) *Preview {
	if doc == nil {
		doc = &model.Document{}
	}
	base := previewBase{doc: doc, th: th, opts: opts, visible: true}
	comps := []Component{
		&filenameBar{base},
		&titleBlock{base},
		&frontmatterPanel{base},
		&bodyPanel{base},
	}
	byID := make(map[string]Component, len(comps))
	for _, c := range comps {
		byID[c.ID()] = c
	}
	return &Preview{components: comps, byID: byID}
}

// Render joins the visible components' non-empty renders with single
// newlines and closes any hyperlink left open by truncation.
func (p *Preview) Render(width int) string {
	parts := make([]string, 0, len(p.components))
	for _, c := range p.components {
		if c.Visible() {
			if r := c.Render(width); r != "" {
				parts = append(parts, r)
			}
		}
	}
	return closeOpenHyperlinks(strings.Join(parts, "\n"))
}

// SetVisible shows or hides the component with the given ID.
func (p *Preview) SetVisible(id string, v bool) {
	if c, ok := p.byID[id]; ok {
		c.SetVisible(v)
	}
}

// Visible reports whether the component with the given ID is shown.
func (p *Preview) Visible(id string) bool {
	c, ok := p.byID[id]
	return ok && c.Visible()
}

// Toggle flips a component's visibility and reports the new state; unknown
// IDs report false.
func (p *Preview) Toggle(id string) bool {
	c, ok := p.byID[id]
	if !ok {
		return false
	}
	v := !c.Visible()
	c.SetVisible(v)
	return v
}

// IDs returns the component IDs in render order.
func (p *Preview) IDs() []string {
	ids := make([]string, 0, len(p.components))
	for _, c := range p.components {
		ids = append(ids, c.ID())
	}
	return ids
}

// previewBase holds the construction-time context shared by every preview
// component plus its visibility flag.
type previewBase struct {
	doc     *model.Document
	th      theme.Theme
	opts    PreviewOptions
	visible bool
}

func (c *previewBase) Visible() bool     { return c.visible }
func (c *previewBase) SetVisible(v bool) { c.visible = v }

// filenameBar is the preview's first line: the document's basename in the
// theme's filename style.
type filenameBar struct{ previewBase }

func (c *filenameBar) ID() string { return ComponentFilename }

func (c *filenameBar) Render(int) string {
	if c.doc.Path == "" {
		return ""
	}
	return highlightRe(c.th.Filename.Render(filepath.Base(c.doc.Path)), c.opts.Highlight, c.th.HighlightSGR)
}

// titleBlock renders the document title in the theme's title style, without
// the old "Title:" label.
type titleBlock struct{ previewBase }

func (c *titleBlock) ID() string { return ComponentTitle }

func (c *titleBlock) Render(int) string {
	title := c.doc.Title
	if title == "" {
		title = "(untitled)"
	}
	return highlightRe(c.th.Title.Render(title), c.opts.Highlight, c.th.HighlightSGR)
}

// frontmatterPanel renders the retained full Path row followed by the
// normalized frontmatter rows; a web Resource value becomes an OSC 8
// hyperlink when hyperlinks are on. No link marker runes are emitted here.
type frontmatterPanel struct{ previewBase }

func (c *frontmatterPanel) ID() string { return ComponentFrontmatter }

func (c *frontmatterPanel) Render(int) string {
	rows := buildFrontmatterRows(c.doc)
	fields := make([]fmField, 0, len(rows)+1)
	if c.doc.Path != "" {
		fields = append(fields, fmField{label: "Path", kind: fmScalar, scalar: c.doc.Path})
	}
	for _, row := range rows {
		if row.label == "Resource" && row.kind == fmScalar && c.opts.Hyperlinks && isWebLink(row.scalar) {
			row.scalar = osc8Link(row.scalar, row.scalar, c.th.LinkSGR)
		}
		fields = append(fields, row)
	}
	lines := make([]string, 0, len(fields))
	for _, f := range fields {
		lines = append(lines, renderField(c.th, f, c.opts.Highlight))
	}
	return strings.Join(lines, "\n")
}

// bodyPanel renders the markdown body excerpt; it is the only component that
// carries link marker runes (extractAndHide -> patchLinks).
type bodyPanel struct{ previewBase }

func (c *bodyPanel) ID() string { return ComponentBody }

func (c *bodyPanel) Render(width int) string {
	body, links := extractAndHide(matchWindow(c.doc.Body, c.opts.Highlight, c.th.BodyLines), c.opts.Hyperlinks)
	rendered := renderMarkdown(body, width)
	return patchLinks(highlightRe(rendered, c.opts.Highlight, c.th.HighlightSGR), links, c.opts.Hyperlinks, c.th.LinkSGR)
}
