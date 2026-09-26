package task

import (
	"reflect"
	"strings"
	"testing"
)

// taskCore is the comparable projection of a Task used by the parse tests.
type taskCore struct {
	ID            string
	Status        byte
	Title         string
	Tags          []string
	Priority      string
	Checked       bool
	Seeded        bool
	Unidentified  bool
	PropertyOrder []string
	Properties    map[string]string
	MetaSep       string
	SeedPrefix    string
	HeaderRaw     string
}

func core(t Task) taskCore {
	tags := t.Tags
	if len(tags) == 0 {
		tags = nil
	}
	order := t.PropertyOrder
	if len(order) == 0 {
		order = nil
	}
	props := t.Properties
	if len(props) == 0 {
		props = nil
	}
	return taskCore{
		ID:            t.ID,
		Status:        t.Status,
		Title:         t.Title,
		Tags:          tags,
		Priority:      t.Priority,
		Checked:       t.Checked,
		Seeded:        t.Seeded,
		Unidentified:  t.Unidentified,
		PropertyOrder: order,
		Properties:    props,
		MetaSep:       t.MetaSep,
		SeedPrefix:    t.SeedPrefix,
		HeaderRaw:     t.HeaderRaw,
	}
}

func TestParseHeader(t *testing.T) {
	tests := []struct {
		name string
		line string
		ok   bool
		want Task
	}{
		{
			name: "identified with metadata",
			line: "- [ ] BASIC-1 Draft the launch announcement\t\t#tag !high @status:doing",
			ok:   true,
			want: Task{
				ID: "BASIC-1", Status: ' ', Title: "Draft the launch announcement",
				Tags: []string{"#tag"}, Priority: "high",
				PropertyOrder: []string{"status"}, Properties: map[string]string{"status": "doing"},
				MetaSep: "\t\t",
			},
		},
		{
			name: "checked",
			line: "- [x] BASIC-3 Archive the beta feedback thread",
			ok:   true,
			want: Task{ID: "BASIC-3", Status: 'x', Checked: true, Title: "Archive the beta feedback thread"},
		},
		{
			name: "uppercase X is not a task",
			line: "- [X] A-1 Title",
			ok:   false,
		},
		{
			name: "leading indentation is not a task",
			line: "  - [ ] A-1 Title",
			ok:   false,
		},
		{
			name: "plain bullet is not a task",
			line: "- Just a bullet point",
			ok:   false,
		},
		{
			name: "blockquote checkbox is not a task",
			line: "> - [ ] Quoted pseudo-task",
			ok:   false,
		},
		{
			name: "unidentified",
			line: "- [ ] No ID here",
			ok:   true,
			want: Task{Status: ' ', Title: "No ID here", Unidentified: true},
		},
		{
			name: "seed",
			line: "- [ ] SEED- A seed line",
			ok:   true,
			want: Task{Status: ' ', Title: "A seed line", Seeded: true, SeedPrefix: "SEED"},
		},
		{
			name: "carriage return is stripped",
			line: "- [ ] EDGE-1 Preserve the raw file\r",
			ok:   true,
			want: Task{ID: "EDGE-1", Status: ' ', Title: "Preserve the raw file"},
		},
		{
			name: "hash number stays in title",
			line: "- [ ] META-1 Fix issue #123",
			ok:   true,
			want: Task{ID: "META-1", Status: ' ', Title: "Fix issue #123"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, ok := ParseHeader(tt.line)
			if ok != tt.ok {
				t.Fatalf("ParseHeader(%q) ok = %v, want %v", tt.line, ok, tt.ok)
			}
			if !ok {
				return
			}
			want := tt.want
			want.HeaderRaw = strings.TrimSuffix(tt.line, "\r")
			if !reflect.DeepEqual(core(got), core(want)) {
				t.Errorf("ParseHeader(%q)\n got = %+v\nwant = %+v", tt.line, core(got), core(want))
			}
		})
	}
}

func TestSplitMeta(t *testing.T) {
	tests := []struct {
		name      string
		rest      string
		wantTitle string
		wantMeta  string
	}{
		{"tab separated", "Draft the launch announcement\t\t#tag !high @status:doing", "Draft the launch announcement", "#tag !high @status:doing"},
		{"three tabs", "Header has three tabs\t\t\t#edge", "Header has three tabs", "#edge"},
		{"single space token", "Review the migration draft #docs", "Review the migration draft", "#docs"},
		{"hash number stays in title", "Fix issue #123", "Fix issue #123", ""},
		{"no metadata", "Gather screenshots for the changelog", "Gather screenshots for the changelog", ""},
		{"prose after single space", "Take the long way home", "Take the long way home", ""},
		{"multiple tokens single spaced", "Title #a #b", "Title", "#a #b"},
		{"empty", "", "", ""},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			title, meta := splitMeta(tt.rest)
			if title != tt.wantTitle || meta != tt.wantMeta {
				t.Errorf("splitMeta(%q) = (%q, %q), want (%q, %q)", tt.rest, title, meta, tt.wantTitle, tt.wantMeta)
			}
		})
	}
}

func TestParseTokens(t *testing.T) {
	tests := []struct {
		name         string
		meta         string
		wantTags     []string
		wantPriority string
		wantProps    map[string]string
		wantOrder    []string
	}{
		{
			name:         "all three forms",
			meta:         "#tag !high @status:doing",
			wantTags:     []string{"#tag"},
			wantPriority: "high",
			wantProps:    map[string]string{"status": "doing"},
			wantOrder:    []string{"status"},
		},
		{
			name:         "multiple tags and properties",
			meta:         "#a #b !crit @k:v @k2:v2",
			wantTags:     []string{"#a", "#b"},
			wantPriority: "crit",
			wantProps:    map[string]string{"k": "v", "k2": "v2"},
			wantOrder:    []string{"k", "k2"},
		},
		{
			name:      "hash number is not a tag",
			meta:      "#123",
			wantProps: map[string]string{},
		},
		{
			name:      "bare property is ignored",
			meta:      "@bare",
			wantProps: map[string]string{},
		},
		{
			name:         "unknown priority is returned",
			meta:         "!urgent",
			wantPriority: "urgent",
			wantProps:    map[string]string{},
		},
		{
			name:      "empty",
			meta:      "",
			wantProps: map[string]string{},
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tags, priority, props, order := parseTokens(tt.meta)
			if !reflect.DeepEqual(tags, tt.wantTags) {
				t.Errorf("tags = %v, want %v", tags, tt.wantTags)
			}
			if priority != tt.wantPriority {
				t.Errorf("priority = %q, want %q", priority, tt.wantPriority)
			}
			if !reflect.DeepEqual(props, tt.wantProps) {
				t.Errorf("props = %v, want %v", props, tt.wantProps)
			}
			if !reflect.DeepEqual(order, tt.wantOrder) {
				t.Errorf("order = %v, want %v", order, tt.wantOrder)
			}
		})
	}
}

func TestRenderHeader(t *testing.T) {
	t.Run("canonical separator when metadata added", func(t *testing.T) {
		got := renderHeader(Task{Status: ' ', ID: "A-1", Title: "Title", Tags: []string{"#tag"}})
		want := "- [ ] A-1 Title\t\t#tag"
		if got != want {
			t.Errorf("renderHeader = %q, want %q", got, want)
		}
	})

	t.Run("unidentified", func(t *testing.T) {
		got := renderHeader(Task{Status: ' ', Title: "No ID here", Unidentified: true})
		if want := "- [ ] No ID here"; got != want {
			t.Errorf("renderHeader = %q, want %q", got, want)
		}
	})

	t.Run("seed", func(t *testing.T) {
		got := renderHeader(Task{Status: ' ', Title: "A seed line", Seeded: true, SeedPrefix: "SEED"})
		if want := "- [ ] SEED- A seed line"; got != want {
			t.Errorf("renderHeader = %q, want %q", got, want)
		}
	})

	t.Run("checked", func(t *testing.T) {
		got := renderHeader(Task{Status: 'x', Checked: true, ID: "A-1", Title: "Done"})
		if want := "- [x] A-1 Done"; got != want {
			t.Errorf("renderHeader = %q, want %q", got, want)
		}
	})
}

func TestRoundTripHeader(t *testing.T) {
	lines := []string{
		"- [ ] BASIC-1 Draft the launch announcement\t\t#tag !high @status:doing",
		"- [ ] META-2 Review the migration draft #docs",
		"- [ ] META-1 Fix issue #123",
		"- [ ] BASIC-2 Gather screenshots for the changelog",
		"- [ ] No ID here",
		"- [ ] SEED- A seed line",
		"- [ ] EDGE-3 Header has three tabs\t\t\t#edge",
		"- [x] BASIC-3 Archive the beta feedback thread",
		"- [ ] PRI-4 Reread the incident report",
		"- [ ] Title #a #b",
		"- [ ] A-1 Title  #tag",
	}
	for _, line := range lines {
		t.Run(line, func(t *testing.T) {
			task, ok := ParseHeader(line)
			if !ok {
				t.Fatalf("ParseHeader(%q) ok = false", line)
			}
			if got := renderHeader(task); got != line {
				t.Errorf("round-trip = %q, want %q", got, line)
			}
		})
	}
}
