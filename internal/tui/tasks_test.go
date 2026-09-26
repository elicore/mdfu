package tui

import (
	"fmt"
	"testing"

	tea "github.com/charmbracelet/bubbletea"

	"github.com/elicore/mdfu/internal/task"
)

// mkTaskItem builds a TaskItem for the filter/keymap/model tables.
func mkTaskItem(id, title string) TaskItem {
	return TaskItem{Task: task.Task{ID: id, Title: title}}
}

// applyTaskKey drives a TaskModel headlessly, mirroring tui_test.go's applyKey.
func applyTaskKey(m TaskModel, k tea.KeyMsg) TaskModel {
	next, _ := m.Update(k)
	nm, ok := next.(TaskModel)
	if !ok {
		panic(fmt.Sprintf("expected TaskModel, got %T", next))
	}
	return nm
}

func TestTaskFilter(t *testing.T) {
	items := []TaskItem{
		{
			Task: task.Task{
				ID:         "PRJ-001",
				Title:      "Fix login bug",
				Tags:       []string{"auth"},
				Priority:   "P1",
				Properties: map[string]string{"blocked_by": "PRJ-009"},
			},
		},
		{
			Task: task.Task{
				ID:       "PRJ-002",
				Title:    "Write docs",
				Tags:     []string{"docs", "website"},
				Priority: "P2",
			},
		},
		{
			Task: task.Task{
				ID:       "OPS-003",
				Title:    "Rotate credentials",
				Tags:     []string{"secret"},
				Priority: "P0",
			},
		},
	}

	cases := []struct {
		name  string
		query string
		want  []string
	}{
		{"empty returns all", "", []string{"PRJ-001", "PRJ-002", "OPS-003"}},
		{"by id", "PRJ-001", []string{"PRJ-001"}},
		{"by id lowercase", "prj-001", []string{"PRJ-001"}},
		{"by id fragment", "ops-", []string{"OPS-003"}},
		{"by title", "login", []string{"PRJ-001"}},
		{"by title case-insensitive", "ROTATE", []string{"OPS-003"}},
		{"by tag", "website", []string{"PRJ-002"}},
		{"by tag case-insensitive", "AUTH", []string{"PRJ-001"}},
		{"by priority", "p1", []string{"PRJ-001"}},
		{"by priority ops", "p0", []string{"OPS-003"}},
		{"by property value", "prj-009", []string{"PRJ-001"}},
		{"by property key", "blocked", []string{"PRJ-001"}},
		{"multi-token AND", "docs website", []string{"PRJ-002"}},
		{"multi-token AND no partial", "docs auth", nil},
		{"priority + tag", "p2 website", []string{"PRJ-002"}},
		{"no match", "nonexistent-zzz", nil},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := FilterTaskItems(tc.query, items)
			var gotIDs []string
			for _, it := range got {
				gotIDs = append(gotIDs, it.ID)
			}
			if len(gotIDs) != len(tc.want) {
				t.Fatalf("query %q: got %v, want %v", tc.query, gotIDs, tc.want)
			}
			for i := range tc.want {
				if gotIDs[i] != tc.want[i] {
					t.Fatalf("query %q: got %v, want %v", tc.query, gotIDs, tc.want)
				}
			}
		})
	}
}

func TestTaskKeyMap(t *testing.T) {
	km := DefaultTaskKeyMap()

	want := []struct {
		name string
		b    []string
		keys []string
	}{
		{"up", km.Up.Keys(), []string{"up", "k"}},
		{"down", km.Down.Keys(), []string{"down", "j"}},
		{"pgup", km.PageUp.Keys(), []string{"pgup", "ctrl+b"}},
		{"pgdn", km.PageDown.Keys(), []string{"pgdn", "ctrl+f"}},
		{"home", km.Home.Keys(), []string{"home", "g"}},
		{"end", km.End.Keys(), []string{"end", "G"}},
		{"filter", km.Filter.Keys(), []string{"/"}},
		{"esc", km.Esc.Keys(), []string{"esc"}},
		{"tab", km.Tab.Keys(), []string{"tab"}},
		{"toggle done", km.ToggleDone.Keys(), []string{"x"}},
		{"toggle blocked", km.ToggleBlk.Keys(), []string{"b"}},
		{"open", km.Open.Keys(), []string{"o"}},
		{"edit body", km.EditBody.Keys(), []string{"e"}},
		{"edit title", km.EditTitle.Keys(), []string{"t"}},
		{"new", km.New.Keys(), []string{"n"}},
		{"move", km.Move.Keys(), []string{"m"}},
		{"archive", km.Archive.Keys(), []string{"a"}},
		{"help", km.Help.Keys(), []string{"?"}},
		{"quit", km.Quit.Keys(), []string{"q", "ctrl+c"}},
		{"save", km.Save.Keys(), []string{"ctrl+s"}},
		{"cancel", km.Cancel.Keys(), []string{"esc"}},
	}

	for _, tc := range want {
		for _, k := range tc.keys {
			if !containsString(tc.b, k) {
				t.Errorf("%s: Keys() = %v, missing %q", tc.name, tc.b, k)
			}
		}
	}

	if got := len(km.ShortHelp()); got == 0 {
		t.Errorf("ShortHelp() returned no bindings")
	}
	if got := len(km.FullHelp()); got == 0 {
		t.Errorf("FullHelp() returned no columns")
	}
}

func containsString(hay []string, needle string) bool {
	for _, s := range hay {
		if s == needle {
			return true
		}
	}
	return false
}

func TestTaskModel(t *testing.T) {
	newModel := func() TaskModel {
		items := []TaskItem{
			mkTaskItem("PRJ-001", "alpha"),
			mkTaskItem("PRJ-002", "beta"),
			mkTaskItem("PRJ-003", "gamma"),
		}
		return NewTaskModel(items, TaskConfig{})
	}

	t.Run("browse x toggles done", func(t *testing.T) {
		m := newModel()
		if m.FilteredItems()[0].Checked {
			t.Fatal("precondition: first task should start open")
		}
		m = applyTaskKey(m, runeKey('x'))
		if m.TaskMode() != taskModeBrowse {
			t.Fatalf("x changed mode to %v", m.TaskMode())
		}
		if !m.FilteredItems()[0].Checked {
			t.Fatalf("x did not toggle done: %+v", m.FilteredItems()[0])
		}
		// toggle back
		m = applyTaskKey(m, runeKey('x'))
		if m.FilteredItems()[0].Checked {
			t.Fatalf("second x did not untoggle done")
		}
	})

	t.Run("browse b toggles blocked visibility", func(t *testing.T) {
		m := newModel()
		if m.ShowBlocked() {
			t.Fatal("precondition: showBlocked should start false")
		}
		m = applyTaskKey(m, runeKey('b'))
		if !m.ShowBlocked() {
			t.Fatalf("b did not toggle showBlocked")
		}
		m = applyTaskKey(m, runeKey('b'))
		if m.ShowBlocked() {
			t.Fatalf("second b did not untoggle showBlocked")
		}
	})

	t.Run("slash enters filter mode", func(t *testing.T) {
		m := newModel()
		m = applyTaskKey(m, runeKey('/'))
		if m.TaskMode() != taskModeFilter {
			t.Fatalf("slash: mode = %v, want taskModeFilter", m.TaskMode())
		}
	})

	t.Run("filter x types and does not toggle done", func(t *testing.T) {
		m := newModel()
		m = applyTaskKey(m, runeKey('/'))
		m = applyTaskKey(m, runeKey('x'))
		if m.Query() != "x" {
			t.Fatalf("filter x: Query() = %q, want %q", m.Query(), "x")
		}
		for i, it := range m.AllItems() {
			if it.Checked {
				t.Fatalf("filter x toggled done on item %d", i)
			}
		}
		if m.TaskMode() != taskModeFilter {
			t.Fatalf("filter x changed mode to %v", m.TaskMode())
		}
	})

	t.Run("cursor wraps both ways", func(t *testing.T) {
		m := newModel()
		m = applyTaskKey(m, key(tea.KeyUp))
		if got := m.CursorIndex(); got != 2 {
			t.Fatalf("up wrap: CursorIndex = %d, want 2", got)
		}
		m = applyTaskKey(m, runeKey('j'))
		if got := m.CursorIndex(); got != 0 {
			t.Fatalf("down wrap: CursorIndex = %d, want 0", got)
		}
	})

	t.Run("esc aborts on empty filter", func(t *testing.T) {
		m := newModel()
		m = applyTaskKey(m, key(tea.KeyEsc))
		if !m.IsAborted() {
			t.Fatalf("esc on empty filter did not abort")
		}
	})

	t.Run("esc clears filter first then aborts", func(t *testing.T) {
		m := newModel()
		m = applyTaskKey(m, runeKey('/'))
		m = applyTaskKey(m, runeKey('a'))
		m = applyTaskKey(m, key(tea.KeyEsc))
		if m.IsAborted() {
			t.Fatalf("esc in filter mode aborted instead of clearing")
		}
		if m.Query() != "" {
			t.Fatalf("esc in filter mode did not clear query: %q", m.Query())
		}
		if m.TaskMode() != taskModeBrowse {
			t.Fatalf("esc left mode at %v", m.TaskMode())
		}
		m = applyTaskKey(m, key(tea.KeyEsc))
		if !m.IsAborted() {
			t.Fatalf("second esc did not abort")
		}
	})
}
