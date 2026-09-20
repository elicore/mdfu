package tui

import (
	"os"
	"reflect"
	"regexp"
	"strings"
	"testing"
	"time"

	"github.com/charmbracelet/lipgloss"

	"github.com/elicore/mdfu/internal/model"
	"github.com/elicore/mdfu/internal/parse"
	"github.com/elicore/mdfu/internal/theme"
)

// forceANSIColors makes lipgloss emit SGR sequences in headless test runs:
// under `go test` stdout is a pipe, so the detected profile would otherwise be
// Ascii (no color). TERM=xterm covers the interactive-TTY case and
// CLICOLOR_FORCE the pipe case; both resolve to the ANSI profile. The default
// renderer is replaced first (its color profile is cached in a sync.Once that
// an earlier test may already have fired) and restored on cleanup, so other
// tests keep the profile they would have detected without this test.
func forceANSIColors(t *testing.T) {
	t.Helper()
	lipgloss.SetDefaultRenderer(lipgloss.NewRenderer(os.Stdout))
	t.Setenv("TERM", "xterm")
	t.Setenv("CLICOLOR_FORCE", "1")
	t.Cleanup(func() { lipgloss.SetDefaultRenderer(lipgloss.NewRenderer(os.Stdout)) })
}

func renderFMPanel(th theme.Theme, rows []fmField) string {
	lines := make([]string, 0, len(rows))
	for _, r := range rows {
		lines = append(lines, renderField(th, r, nil))
	}
	return strings.Join(lines, "\n")
}

func parseFixture(t *testing.T, path string) *model.Document {
	t.Helper()
	doc, err := parse.ParseFile(path)
	if err != nil {
		t.Fatalf("ParseFile(%s): %v", path, err)
	}
	return doc
}

func fmLabels(rows []fmField) []string {
	labels := make([]string, 0, len(rows))
	for _, r := range rows {
		labels = append(labels, r.label)
	}
	return labels
}

func fmByLabel(rows []fmField) map[string]fmField {
	out := make(map[string]fmField, len(rows))
	for _, r := range rows {
		out[r.label] = r
	}
	return out
}

// Given a parsed OKF v0.2 metric document
// When building the frontmatter rows
// Then normalized rows come first in spec order, then the remaining raw keys
// sorted alphabetically, and sources stays an object list (never pills).
func TestFrontmatterRowsOKFMetric(t *testing.T) {
	doc := parseFixture(t, "../../testdata/okf-v02-metric.md")
	rows := buildFrontmatterRows(doc)

	wantLabels := []string{"Type", "Status", "Description", "Tags", "Created", "Updated", "generated", "sources", "verified"}
	if got := fmLabels(rows); !reflect.DeepEqual(got, wantLabels) {
		t.Fatalf("labels = %v, want %v", got, wantLabels)
	}

	byLabel := fmByLabel(rows)
	if f := byLabel["Type"]; f.kind != fmScalar || f.scalar != "Metric" {
		t.Errorf("Type row = %+v, want scalar Metric", f)
	}
	if f := byLabel["Created"]; f.kind != fmScalar || f.scalar != "2024-05-01T00:00:00Z" {
		t.Errorf("Created row = %+v, want scalar 2024-05-01T00:00:00Z", f)
	}
	if f := byLabel["Tags"]; f.kind != fmPills || !reflect.DeepEqual(f.pills, []string{"growth", "kpi", "monthly"}) {
		t.Errorf("Tags row = %+v, want pills [growth kpi monthly]", f)
	}
	src, ok := byLabel["sources"]
	if !ok || src.kind != fmObject {
		t.Fatalf("sources row = %+v (ok=%v), want fmObject — object lists must not render as pills", src, ok)
	}
	if len(src.obj) != 2 {
		t.Fatalf("sources obj has %d elements, want 2", len(src.obj))
	}
	for _, p := range src.obj {
		if p.key != "" || p.field.kind != fmObject {
			t.Errorf("sources element = %+v, want anonymous fmObject group", p)
		}
	}
}

// Given the OKF v0.2 metric document
// When the panel is rendered
// Then attestation maps flatten inline as "by: …, at: …" and the sources
// object list joins its elements with "; ".
func TestFrontmatterRenderOKFMetric(t *testing.T) {
	doc := parseFixture(t, "../../testdata/okf-v02-metric.md")
	panel := stripANSI(renderFMPanel(theme.Default(), buildFrontmatterRows(doc)))

	for _, want := range []string{
		"Type: Metric",
		"by: data-pipeline, at: 2024-06-01T10:00:00Z",
		"by: alice, at: 2024-06-02T12:30:00Z",
	} {
		if !strings.Contains(panel, want) {
			t.Errorf("panel missing %q\n---\n%s", want, panel)
		}
	}

	var sources string
	for line := range strings.Lines(panel) {
		if strings.HasPrefix(line, "sources:") {
			sources = line
		}
	}
	if sources == "" {
		t.Fatalf("no sources row in panel\n---\n%s", panel)
	}
	i1 := strings.Index(sources, "id: warehouse")
	i2 := strings.Index(sources, "id: dashboard")
	sep := strings.Index(sources, "; ")
	if i1 < 0 || i2 < 0 {
		t.Fatalf("sources row missing element ids: %q", sources)
	}
	if sep < i1 || sep > i2 {
		t.Errorf("object list elements must be joined by \"; \" between the ids: %q", sources)
	}
}

// Given a raw value that is a JSON-encoded object string
// When the panel is rendered
// Then the string is flattened inline under its key.
func TestFrontmatterJSONStringFlattened(t *testing.T) {
	doc := &model.Document{Raw: map[string]any{
		"meta": `{"created_by":"Hermes","created_at":"2026-09-01T10:00:00Z"}`,
	}}
	panel := stripANSI(renderFMPanel(theme.Default(), buildFrontmatterRows(doc)))
	want := "meta: created_by: Hermes, created_at: 2026-09-01T10:00:00Z"
	if !strings.Contains(panel, want) {
		t.Fatalf("panel missing %q\n---\n%s", want, panel)
	}
}

// Given a document with normalized tags (and a conflicting raw tags string)
// When the Tags row is rendered with colors forced on
// Then the normalized tags render as pills carrying the pill style's SGR
// sequences and the raw tags string is never consulted.
func TestFrontmatterTagsRenderAsPills(t *testing.T) {
	forceANSIColors(t)
	th := theme.Default()
	doc := &model.Document{
		Tags: []string{"launch", "tolaria"},
		Raw:  map[string]any{"tags": "rawtag"},
	}
	rows := buildFrontmatterRows(doc)
	if len(rows) != 1 || rows[0].label != "Tags" || rows[0].kind != fmPills {
		t.Fatalf("rows = %+v, want a single Tags pill row", rows)
	}

	raw := renderField(th, rows[0], nil)
	stripped := stripANSI(raw)
	for _, tag := range []string{"launch", "tolaria"} {
		if !strings.Contains(stripped, tag) {
			t.Errorf("stripped pill row missing %q: %q", tag, stripped)
		}
	}
	if strings.Contains(stripped, "rawtag") {
		t.Errorf("pills must come from doc.Tags, never Raw[tags]: %q", stripped)
	}
	if !strings.Contains(stripped, "│") {
		t.Errorf("round pill shape should draw rounded side borders: %q", stripped)
	}

	probe := th.Pill.Render("probe")
	seqs := ansiRe.FindAllString(probe, -1)
	if len(seqs) == 0 {
		t.Fatal("pill probe emitted no SGR sequences; color profile was not forced")
	}
	for _, s := range seqs {
		if !strings.Contains(raw, s) {
			t.Errorf("rendered pill is missing the pill style's SGR %q\nraw: %q", s, raw)
		}
	}
}

// Given a document with no frontmatter and no normalized fields
// When building rows and rendering the panel
// Then there are zero rows and no rendered artifact.
func TestFrontmatterEmptyDocument(t *testing.T) {
	doc := &model.Document{}
	rows := buildFrontmatterRows(doc)
	if len(rows) != 0 {
		t.Fatalf("rows = %v, want none", rows)
	}
	if got := renderFMPanel(theme.Default(), rows); got != "" {
		t.Fatalf("panel = %q, want empty", got)
	}
}

// Given the same document
// When rows are built repeatedly
// Then the model is identical every time, and raw keys sort alphabetically.
func TestFrontmatterDeterministicOrdering(t *testing.T) {
	doc := parseFixture(t, "../../testdata/okf-v02-metric.md")
	first := buildFrontmatterRows(doc)
	for range 10 {
		if got := buildFrontmatterRows(doc); !reflect.DeepEqual(got, first) {
			t.Fatalf("non-deterministic rows:\nfirst: %v\ngot:   %v", first, got)
		}
	}

	scrambled := &model.Document{Raw: map[string]any{"zeta": 1, "Alpha": 2, "beta": 3}}
	if got, want := fmLabels(buildFrontmatterRows(scrambled)), []string{"Alpha", "beta", "zeta"}; !reflect.DeepEqual(got, want) {
		t.Fatalf("raw key order = %v, want %v", got, want)
	}
}

// Given raw values of every supported shape
// When building rows
// Then each maps to the documented kind and empty values are skipped.
func TestFrontmatterRawValueMapping(t *testing.T) {
	doc := &model.Document{Raw: map[string]any{
		"plain":   "hello",
		"num":     42,
		"flag":    true,
		"list":    []any{"a", "b"},
		"strs":    []string{"x", "y"},
		"nothing": nil,
		"blank":   "  ",
		"empty":   map[string]any{},
		"mixed":   []any{"a", map[string]any{"b": 1}},
	}}
	byLabel := fmByLabel(buildFrontmatterRows(doc))

	for _, skipped := range []string{"nothing", "blank", "empty"} {
		if _, ok := byLabel[skipped]; ok {
			t.Errorf("%q must be skipped for nil/empty values", skipped)
		}
	}
	if f := byLabel["plain"]; f.kind != fmScalar || f.scalar != "hello" {
		t.Errorf("plain = %+v, want scalar hello", f)
	}
	if f := byLabel["num"]; f.kind != fmScalar || f.scalar != "42" {
		t.Errorf("num = %+v, want scalar 42", f)
	}
	if f := byLabel["flag"]; f.kind != fmScalar || f.scalar != "true" {
		t.Errorf("flag = %+v, want scalar true", f)
	}
	if f := byLabel["list"]; f.kind != fmPills || !reflect.DeepEqual(f.pills, []string{"a", "b"}) {
		t.Errorf("list = %+v, want pills [a b]", f)
	}
	if f := byLabel["strs"]; f.kind != fmPills || !reflect.DeepEqual(f.pills, []string{"x", "y"}) {
		t.Errorf("strs = %+v, want pills [x y]", f)
	}
	if f := byLabel["mixed"]; f.kind != fmScalar {
		t.Errorf("mixed = %+v, want scalar fallback for a mixed list", f)
	}
}

// Given nested maps inside a raw object value
// When the panel is rendered
// Then nesting flattens to dotted keys up to depth 3 and deeper maps fall
// back to %v.
func TestFrontmatterNestedObjectDottedKeys(t *testing.T) {
	doc := &model.Document{Raw: map[string]any{
		"nested": map[string]any{"a": map[string]any{"b": 1}},
		"deep":   map[string]any{"a": map[string]any{"b": map[string]any{"c": map[string]any{"d": 1}}}},
	}}
	panel := stripANSI(renderFMPanel(theme.Default(), buildFrontmatterRows(doc)))
	if !strings.Contains(panel, "a.b: 1") {
		t.Errorf("nested map must flatten to dotted keys\n---\n%s", panel)
	}
	if !strings.Contains(panel, "a.b.c: map[d:1]") {
		t.Errorf("maps deeper than 3 levels must fall back to %%v\n---\n%s", panel)
	}
}

// Given a match regexp
// When a row is rendered
// Then matches are wrapped in the highlight SGR.
func TestFrontmatterRenderHighlight(t *testing.T) {
	doc := parseFixture(t, "../../testdata/okf-v02-metric.md")
	re := regexp.MustCompile("Metric")
	var out string
	for _, r := range buildFrontmatterRows(doc) {
		if r.label == "Type" {
			out = renderField(theme.Default(), r, re)
		}
	}
	if !strings.Contains(out, hlStart+"Metric"+hlReset) {
		t.Errorf("match not highlighted in %q", out)
	}
}

// Given a Portent task document
// When building rows
// Then the pill rows come from the normalized (wikilink-stripped,
// comma-split) lists and every raw key is consumed.
func TestFrontmatterPortentRows(t *testing.T) {
	doc := parseFixture(t, "../../testdata/portent-task.md")
	rows := buildFrontmatterRows(doc)

	wantLabels := []string{"Type", "Status", "Tags", "Belongs to", "Related to", "Created"}
	if got := fmLabels(rows); !reflect.DeepEqual(got, wantLabels) {
		t.Fatalf("labels = %v, want %v", got, wantLabels)
	}
	byLabel := fmByLabel(rows)
	for label, want := range map[string][]string{
		"Tags":       {"tolaria", "launch", "Project Atlas"},
		"Belongs to": {"Project Atlas"},
		"Related to": {"Launch Plan", "Retro Notes"},
	} {
		if f := byLabel[label]; f.kind != fmPills || !reflect.DeepEqual(f.pills, want) {
			t.Errorf("%s row = %+v, want pills %v", label, f, want)
		}
	}
}

func TestFrontmatterScalarString(t *testing.T) {
	ts := time.Date(2026, 9, 1, 10, 0, 0, 0, time.UTC)
	tests := []struct {
		name string
		in   any
		want string
	}{
		{"nil", nil, ""},
		{"string", "s", "s"},
		{"time", ts, "2026-09-01T10:00:00Z"},
		{"bool", true, "true"},
		{"int", 42, "42"},
		{"int64", int64(7), "7"},
		{"float", 3.5, "3.5"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := scalarString(tt.in); got != tt.want {
				t.Errorf("scalarString(%v) = %q, want %q", tt.in, got, tt.want)
			}
		})
	}
}
