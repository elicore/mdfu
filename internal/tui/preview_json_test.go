package tui

import (
	"strings"
	"testing"

	"github.com/elicore/mdfu/internal/theme"
)

// Given the okf-json-metadata fixture with a JSON-encoded string frontmatter
// value and standard OKF maps
// When the file is parsed from disk and rendered through the full Preview
// pipeline with the default theme
// Then the meta row flattens to "created_by: Hermes, created_at:
// 2026-09-01T10:00:00Z", tags render as pills carrying the pill style's SGRs,
// and the raw JSON string never appears verbatim.
func TestPreviewJSONMetadata(t *testing.T) {
	forceANSIColors(t)
	th := theme.Default()
	doc := parseFixture(t, "../../testdata/okf-json-metadata.md")
	raw := NewPreview(doc, th, PreviewOptions{}).Render(80)
	panel := stripANSI(raw)

	want := "meta: created_by: Hermes, created_at: 2026-09-01T10:00:00Z"
	if !strings.Contains(panel, want) {
		t.Errorf("panel missing %q\n---\n%s", want, panel)
	}
	if strings.Contains(panel, `{"created_by"`) {
		t.Errorf("raw JSON string must not render verbatim\n---\n%s", panel)
	}

	var tagsRow string
	for line := range strings.Lines(panel) {
		if strings.HasPrefix(line, "Tags: ") {
			tagsRow = line
		}
	}
	if tagsRow == "" {
		t.Fatalf("panel missing Tags row\n---\n%s", panel)
	}
	for _, tag := range []string{"hermes", "ingest"} {
		if !strings.Contains(tagsRow, tag) {
			t.Errorf("stripped Tags row missing %q: %q", tag, tagsRow)
		}
	}
	if !strings.Contains(tagsRow, "│") {
		t.Errorf("round pill shape should draw rounded side borders: %q", tagsRow)
	}

	probe := th.Pill.Render("probe")
	seqs := ansiRe.FindAllString(probe, -1)
	if len(seqs) == 0 {
		t.Fatal("pill probe emitted no SGR sequences; color profile was not forced")
	}
	for _, s := range seqs {
		if !strings.Contains(raw, s) {
			t.Errorf("rendered preview is missing the pill style's SGR %q\nraw: %q", s, raw)
		}
	}
}
