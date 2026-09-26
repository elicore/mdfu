package task

import (
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"
)

// loadFixture reads a fixture from ../../testdata/tasks and returns its lines
// together with every fence-unmasked header parsed from it.
func loadFixture(t *testing.T, name string) ([]string, []Task) {
	t.Helper()
	path := filepath.Join("..", "..", "testdata", "tasks", name)
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read fixture %s: %v", name, err)
	}
	lines := strings.Split(string(data), "\n")
	mask := fenceMask(lines)
	var tasks []Task
	for i, line := range lines {
		if mask[i] {
			continue
		}
		task, ok := ParseHeader(line)
		if !ok {
			continue
		}
		task.Line = i + 1
		tasks = append(tasks, task)
	}
	return lines, tasks
}

func TestFixtures(t *testing.T) {
	tests := []struct {
		name    string
		headers int
		ids     []string
	}{
		{"spec-basic.md", 3, []string{"BASIC-1", "BASIC-2", "BASIC-3"}},
		{"spec-metadata.md", 3, []string{"META-1", "META-2", "META-3"}},
		{"spec-fenced.md", 1, []string{"FENCE-9"}},
		{"spec-blockers.md", 4, []string{"BLK-1", "BLK-2", "BLK-3", "BLK-4"}},
		{"spec-unidentified.md", 3, nil},
		{"spec-priority.md", 4, []string{"PRI-1", "PRI-2", "PRI-3", "PRI-4"}},
		{"spec-headings.md", 4, []string{"HEAD-1", "HEAD-2", "HEAD-3", "HEAD-4"}},
		{"spec-prose.md", 3, []string{"PROSE-1", "PROSE-2", "PROSE-3"}},
		{"spec-edge.md", 3, []string{"EDGE-1", "EDGE-2", "EDGE-3"}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, tasks := loadFixture(t, tt.name)
			if len(tasks) != tt.headers {
				t.Fatalf("%s: parsed %d headers, want %d", tt.name, len(tasks), tt.headers)
			}
			var ids []string
			for _, task := range tasks {
				if task.ID != "" {
					ids = append(ids, task.ID)
				}
			}
			if !reflect.DeepEqual(ids, tt.ids) {
				t.Errorf("%s: IDs = %v, want %v", tt.name, ids, tt.ids)
			}
		})
	}

	t.Run("basic metadata and body", func(t *testing.T) {
		lines, tasks := loadFixture(t, "spec-basic.md")
		first := tasks[0]
		if first.ID != "BASIC-1" {
			t.Fatalf("first ID = %q, want BASIC-1", first.ID)
		}
		title, meta := splitMeta(strings.TrimPrefix(first.HeaderRaw, "- [ ] BASIC-1 "))
		if title != "Draft the launch announcement" || meta != "#tag !high @status:doing" {
			t.Errorf("splitMeta = (%q, %q)", title, meta)
		}
		if !reflect.DeepEqual(first.Tags, []string{"#tag"}) {
			t.Errorf("Tags = %v, want [#tag]", first.Tags)
		}
		if first.Priority != "high" {
			t.Errorf("Priority = %q, want high", first.Priority)
		}
		if first.Properties["status"] != "doing" {
			t.Errorf("status = %q, want doing", first.Properties["status"])
		}
		end, ok := blockRange(lines, first.Line-1)
		if !ok {
			t.Fatal("blockRange returned ok=false for BASIC-1")
		}
		body, indent := collectBody(lines, first.Line-1, end)
		if got := len(strings.Split(body, "\n")); got != 2 {
			t.Errorf("BASIC-1 body line count = %d, want 2 (body=%q)", got, body)
		}
		if indent != "  " {
			t.Errorf("BASIC-1 body indent = %q, want two spaces", indent)
		}
	})

	t.Run("metadata title keeps hash number", func(t *testing.T) {
		_, tasks := loadFixture(t, "spec-metadata.md")
		byID := map[string]Task{}
		for _, task := range tasks {
			byID[task.ID] = task
		}
		meta1 := byID["META-1"]
		if !strings.HasSuffix(meta1.Title, "#123") {
			t.Errorf("META-1 title = %q, want trailing #123", meta1.Title)
		}
		for _, tag := range meta1.Tags {
			if tag == "#123" {
				t.Errorf("META-1 parsed #123 as a tag")
			}
		}
		meta2 := byID["META-2"]
		if meta2.Title != "Review the migration draft" {
			t.Errorf("META-2 title = %q", meta2.Title)
		}
		if !reflect.DeepEqual(meta2.Tags, []string{"#docs"}) {
			t.Errorf("META-2 tags = %v, want [#docs]", meta2.Tags)
		}
	})

	t.Run("unidentified and seed forms", func(t *testing.T) {
		_, tasks := loadFixture(t, "spec-unidentified.md")
		seeds, unidentified := 0, 0
		for _, task := range tasks {
			switch {
			case task.Seeded:
				seeds++
				if task.SeedPrefix != "SEED" {
					t.Errorf("seed prefix = %q, want SEED", task.SeedPrefix)
				}
			case task.Unidentified:
				unidentified++
			}
		}
		if seeds != 1 || unidentified != 2 {
			t.Errorf("seed/unidentified = %d/%d, want 1/2", seeds, unidentified)
		}
	})

	t.Run("priority values", func(t *testing.T) {
		_, tasks := loadFixture(t, "spec-priority.md")
		var got []string
		for _, task := range tasks {
			got = append(got, task.Priority)
		}
		want := []string{"crit", "high", "low", ""}
		if !reflect.DeepEqual(got, want) {
			t.Errorf("priorities = %v, want %v", got, want)
		}
	})

	t.Run("fence hides pseudo tasks", func(t *testing.T) {
		lines, tasks := loadFixture(t, "spec-fenced.md")
		if len(tasks) != 1 || tasks[0].ID != "FENCE-9" {
			t.Fatalf("spec-fenced tasks = %+v, want only FENCE-9", tasks)
		}
		mask := fenceMask(lines)
		parsedInsideFence := 0
		for i, line := range lines {
			if !mask[i] {
				continue
			}
			if _, ok := ParseHeader(line); ok {
				parsedInsideFence++
			}
		}
		if parsedInsideFence != 3 {
			t.Errorf("checkbox-shaped lines inside fences = %d, want 3", parsedInsideFence)
		}
	})
}

// TestFixturesIdentifiedIDs is a small guard that the fixture list itself is not
// vacuous and remains sorted for stable reporting.
func TestFixturesIdentifiedIDs(t *testing.T) {
	_, tasks := loadFixture(t, "spec-headings.md")
	var ids []string
	for _, task := range tasks {
		ids = append(ids, task.ID)
	}
	sorted := append([]string(nil), ids...)
	sort.Strings(sorted)
	if !reflect.DeepEqual(ids, sorted) {
		t.Errorf("expected certified fixture order; got %v", ids)
	}
}
