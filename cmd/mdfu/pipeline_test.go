package main

import (
	"os"
	"path/filepath"
	"testing"
)

// buildTestVault creates a small vault covering the required shapes:
// an OKF-style Metric with tags, a Portent-style archived Task, a plain
// no-frontmatter file, and a bad-YAML file whose body stays searchable.
func buildTestVault(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	files := map[string]string{
		"metric.md": `---
type: Metric
title: Monthly Active Users
tags: [growth, kpi]
status: verified
---

# Monthly Active Users

MAU grew month over month, driven by onboarding improvements.
`,
		"task.md": `---
type: Task
title: Ship mdfu MVP
tags: launch
status: Draft
organized: true
archived: true
---

# Ship mdfu MVP

Finish the deploy checklist before launch day.
`,
		"plain.md": `# Lone Note

Just body, no frontmatter at all. Mentions honeycrisp apples.
`,
		"bad.md": `---
type: [unclosed
  title: "missing quote
tags: [a, b
---

# Recovered Title

Body still searchable: kumquat zebra xylophone.
`,
	}
	for name, content := range files {
		if err := os.WriteFile(filepath.Join(root, name), []byte(content), 0644); err != nil {
			t.Fatalf("write %s: %v", name, err)
		}
	}
	return root
}

func containsBase(paths []string, base string) bool {
	for _, p := range paths {
		if filepath.Base(p) == base {
			return true
		}
	}
	return false
}

func TestRunQueryBareWordFindsBody(t *testing.T) {
	root := buildTestVault(t)
	results, err := runQuery(root, false, "onboarding", false, 10)
	if err != nil {
		t.Fatalf("runQuery: %v", err)
	}
	var paths []string
	for _, r := range results {
		paths = append(paths, r.Path)
	}
	if !containsBase(paths, "metric.md") {
		t.Errorf("bare word %q should find metric.md, got %v", "onboarding", paths)
	}
}

func TestRunQueryTagFilter(t *testing.T) {
	root := buildTestVault(t)
	results, err := runQuery(root, false, "tag:growth", false, 10)
	if err != nil {
		t.Fatalf("runQuery: %v", err)
	}
	if len(results) != 1 || filepath.Base(results[0].Path) != "metric.md" {
		var paths []string
		for _, r := range results {
			paths = append(paths, r.Path)
		}
		t.Errorf("tag:growth should match only metric.md, got %v", paths)
	}
}

func TestRunQueryArchivedHiddenByDefault(t *testing.T) {
	root := buildTestVault(t)

	hidden, err := runQuery(root, false, "type:Task", false, 10)
	if err != nil {
		t.Fatalf("runQuery: %v", err)
	}
	for _, r := range hidden {
		if filepath.Base(r.Path) == "task.md" {
			t.Errorf("archived task.md should be hidden by default, got %v", hidden)
			break
		}
	}

	shown, err := runQuery(root, false, "type:Task", true, 10)
	if err != nil {
		t.Fatalf("runQuery includeArchived: %v", err)
	}
	var paths []string
	for _, r := range shown {
		paths = append(paths, r.Path)
	}
	if !containsBase(paths, "task.md") {
		t.Errorf("archived task.md should be visible with includeArchived=true, got %v", paths)
	}
}

func TestRunQueryBadYAMLStillSearchable(t *testing.T) {
	root := buildTestVault(t)
	results, err := runQuery(root, false, "kumquat", false, 10)
	if err != nil {
		t.Fatalf("runQuery: %v", err)
	}
	var paths []string
	for _, r := range results {
		paths = append(paths, r.Path)
	}
	if !containsBase(paths, "bad.md") {
		t.Errorf("bad-YAML file should stay searchable, got %v", paths)
	}
}
