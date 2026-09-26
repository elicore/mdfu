package tui

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"

	tea "github.com/charmbracelet/bubbletea"
)

// grepTasksSources searches every tasks_*.go source in the package directory
// and returns matching "file:line: text" entries. It fails when it read no
// bytes so a caller can never mistake an empty scan for a clean result.
func grepTasksSources(t *testing.T, re *regexp.Regexp) []string {
	t.Helper()
	files, err := filepath.Glob("tasks_*.go")
	if err != nil {
		t.Fatalf("glob tasks_*.go: %v", err)
	}
	var hits []string
	total := 0
	for _, name := range files {
		data, err := os.ReadFile(name)
		if err != nil {
			t.Fatalf("read %s: %v", name, err)
		}
		total += len(data)
		for i, line := range strings.Split(string(data), "\n") {
			if re.MatchString(line) {
				hits = append(hits, fmt.Sprintf("%s:%d: %s", name, i+1, strings.TrimSpace(line)))
			}
		}
	}
	if total == 0 {
		t.Fatal("grepTasksSources read no source bytes")
	}
	return hits
}

func TestRunTasks(t *testing.T) {
	t.Run("constructor helper injects the filter", func(t *testing.T) {
		items := []TaskItem{mkTaskItem("A-1", "alpha"), mkTaskItem("B-2", "beta")}
		called := false
		stub := func(query string, in []TaskItem) []TaskItem {
			called = true
			if query != "" {
				t.Errorf("initial query = %q, want empty", query)
			}
			out := make([]TaskItem, 0, len(in))
			for _, it := range in {
				if it.Title == "beta" {
					out = append(out, it)
				}
			}
			return out
		}
		m := buildTaskModel(items, TaskConfig{}, stub)
		if !called {
			t.Fatal("injected filter was not invoked by the constructor")
		}
		got := m.FilteredItems()
		if len(got) != 1 || got[0].Title != "beta" {
			t.Fatalf("FilteredItems = %v, want only beta", got)
		}
	})

	t.Run("abort path returns TaskResult Aborted", func(t *testing.T) {
		for _, k := range []tea.KeyMsg{key(tea.KeyEsc), key(tea.KeyCtrlC)} {
			m := buildTaskModel([]TaskItem{mkTaskItem("A-1", "alpha")}, TaskConfig{}, nil)
			m = applyTaskKey(m, k)
			if got := m.Result(); !got.Aborted {
				t.Errorf("Result() after %q = %+v, want Aborted:true", k.String(), got)
			}
		}
	})

	t.Run("task sources avoid the note picker global", func(t *testing.T) {
		forbidden := regexp.MustCompile("Set" + "Filter|global" + "Filter")
		if hits := grepTasksSources(t, forbidden); len(hits) > 0 {
			t.Fatalf("tasks sources reference the note picker global: %v", hits)
		}
		sanity := regexp.MustCompile("New" + "TaskModel" + "WithFilter")
		if hits := grepTasksSources(t, sanity); len(hits) == 0 {
			t.Fatal("sanity grep found nothing; the forbidden-token check would be vacuous")
		}
	})
}
