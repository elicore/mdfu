package tui

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"

	"github.com/elicore/mdfu/internal/task"
	"github.com/elicore/mdfu/internal/theme"
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

func applyTaskMsg(m TaskModel, msg tea.Msg) TaskModel {
	next, _ := m.Update(msg)
	nm, ok := next.(TaskModel)
	if !ok {
		panic(fmt.Sprintf("expected TaskModel, got %T", next))
	}
	return nm
}

func writeRaw(t *testing.T, path, content string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("write %s: %v", path, err)
	}
}

func writeTaskFile(t *testing.T, dir, name, content string) string {
	t.Helper()
	path := filepath.Join(dir, name)
	writeRaw(t, path, content)
	return path
}

func readFile(t *testing.T, path string) string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return string(data)
}

func loadTaskItems(t *testing.T, base string) []TaskItem {
	t.Helper()
	tasks, _, err := task.LoadAll(base, task.Config{})
	if err != nil {
		t.Fatalf("LoadAll(%q): %v", base, err)
	}
	items := make([]TaskItem, 0, len(tasks))
	for _, tk := range tasks {
		items = append(items, TaskItem{Task: tk})
	}
	return items
}

func newTaskVault(t *testing.T, content string) []TaskItem {
	t.Helper()
	dir := t.TempDir()
	writeTaskFile(t, dir, "tasks.md", content)
	t.Chdir(dir)
	return loadTaskItems(t, ".")
}

func findLoadedTask(t *testing.T, id string) task.Task {
	t.Helper()
	tasks, _, err := task.LoadAll(".", task.Config{})
	if err != nil {
		t.Fatalf("LoadAll: %v", err)
	}
	for _, tk := range tasks {
		if tk.ID == id {
			return tk
		}
	}
	t.Fatalf("task %s not found in %+v", id, tasks)
	return task.Task{}
}

func firstLine(s string) string {
	if i := strings.IndexByte(s, '\n'); i >= 0 {
		return s[:i]
	}
	return s
}

func sgrPrefix(s string) string {
	if i := strings.IndexByte(s, 'X'); i >= 0 {
		return s[:i]
	}
	return ""
}

func taskViewVault(t *testing.T) []TaskItem {
	t.Helper()
	dir := t.TempDir()
	writeTaskFile(t, dir, "tasks.md", strings.Join([]string{
		"- [ ] PRJ-001 Fix login bug #auth #ui !P1 @owner:me @blocked_by:PRJ-009",
		"  first body line",
		"  second body line",
		"  " + strings.Repeat("z", 500),
		"",
		"- [x] PRJ-002 Done thing #docs !P2",
		"  done body",
	}, "\n")+"\n")
	t.Chdir(dir)
	return loadTaskItems(t, ".")
}

func TestTaskToggleDone(t *testing.T) {
	items := newTaskVault(t, "- [ ] PRJ-001 alpha\n  body\n")
	m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
	m = applyTaskKey(m, runeKey('x'))
	if got := readFile(t, "tasks.md"); !strings.HasPrefix(got, "- [x] PRJ-001 alpha\n  body\n") {
		t.Fatalf("header not flipped on disk:\n%s", got)
	}
	if !m.FilteredItems()[0].Checked {
		t.Fatal("in-memory item not refreshed after commit")
	}
}

func TestTaskToggleStale(t *testing.T) {
	items := newTaskVault(t, "- [ ] PRJ-001 alpha\n")
	mutated := "- [ ] OPS-100 intruder\n" + readFile(t, "tasks.md")
	writeRaw(t, "tasks.md", mutated)
	m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
	m = applyTaskKey(m, runeKey('x'))
	if !strings.Contains(m.Status(), "file changed") {
		t.Fatalf("status = %q, want file changed", m.Status())
	}
	if got := readFile(t, "tasks.md"); got != mutated {
		t.Fatal("file modified on stale toggle")
	}
}

func TestTaskView(t *testing.T) {
	items := taskViewVault(t)
	cfg := TaskConfig{Base: ".", ShowBlocked: true, ShowDone: true}

	t.Run("list and detail content", func(t *testing.T) {
		m := NewTaskModelWithFilter(items, cfg, nil)
		m = applyTaskMsg(m, tea.WindowSizeMsg{Width: 120, Height: 40})
		frame := stripANSI(m.View())
		for _, want := range []string{"PRJ-001", "Fix login bug", "#auth", "#ui", "!P1", "first body line", "Done thing"} {
			if !strings.Contains(frame, want) {
				t.Fatalf("frame missing %q:\n%s", want, frame)
			}
		}
	})

	t.Run("wide joins and narrow stacks", func(t *testing.T) {
		m := NewTaskModelWithFilter(items, cfg, nil)
		wide := applyTaskMsg(m, tea.WindowSizeMsg{Width: 120, Height: 40}).View()
		if got := strings.Count(firstLine(wide), "╭"); got < 2 {
			t.Fatalf("wide first line has %d pane tops, want >=2:\n%q", got, firstLine(wide))
		}
		stack := applyTaskMsg(m, tea.WindowSizeMsg{Width: 80, Height: 40}).View()
		if got := strings.Count(firstLine(stack), "╭"); got != 1 {
			t.Fatalf("stacked first line has %d pane tops, want 1:\n%q", got, firstLine(stack))
		}
	})

	t.Run("detail pane holds full body", func(t *testing.T) {
		m := NewTaskModelWithFilter(items, cfg, nil)
		m = applyTaskMsg(m, tea.WindowSizeMsg{Width: 120, Height: 60})
		detail := stripANSI(m.renderTaskDetail(80))
		if !strings.Contains(detail, "first body line") || !strings.Contains(detail, "second body line") {
			t.Fatalf("detail missing full body:\n%s", detail)
		}
	})

	t.Run("clamps every frame", func(t *testing.T) {
		for _, sz := range [][2]int{{120, 40}, {100, 30}, {80, 30}, {40, 10}, {1, 1}, {20, 5}} {
			m := NewTaskModelWithFilter(items, cfg, nil)
			m = applyTaskMsg(m, tea.WindowSizeMsg{Width: sz[0], Height: sz[1]})
			f := m.View()
			if w := lipgloss.Width(f); w > sz[0] {
				t.Fatalf("size %dx%d: width %d exceeds", sz[0], sz[1], w)
			}
			if h := lipgloss.Height(f); h > sz[1] {
				t.Fatalf("size %dx%d: height %d exceeds", sz[0], sz[1], h)
			}
		}
	})

	t.Run("empty and 1x1 render", func(t *testing.T) {
		empty := NewTaskModelWithFilter(nil, cfg, nil)
		empty = applyTaskMsg(empty, tea.WindowSizeMsg{Width: 80, Height: 20})
		if empty.View() == "" {
			t.Fatal("empty model rendered nothing")
		}
		one := NewTaskModelWithFilter(items[:1], cfg, nil)
		one = applyTaskMsg(one, tea.WindowSizeMsg{Width: 1, Height: 1})
		if one.View() == "" {
			t.Fatal("1x1 render produced empty output")
		}
	})

	t.Run("more items than fit clamp without panic", func(t *testing.T) {
		many := make([]TaskItem, 0, 500)
		for i := 0; i < 500; i++ {
			many = append(many, TaskItem{Task: task.Task{
				ID: fmt.Sprintf("PRJ-%03d", i), Title: "task", Status: ' ', Line: i + 1, File: "tasks.md",
			}})
		}
		m := NewTaskModelWithFilter(many, cfg, nil)
		m = applyTaskMsg(m, tea.WindowSizeMsg{Width: 40, Height: 10})
		f := m.View()
		if w := lipgloss.Width(f); w > 40 {
			t.Fatalf("500-item frame width %d exceeds 40", w)
		}
		if h := lipgloss.Height(f); h > 10 {
			t.Fatalf("500-item frame height %d exceeds 10", h)
		}
	})

	t.Run("done and blocker styles", func(t *testing.T) {
		forceANSIColors(t)
		th := theme.Default()
		blocker := sgrPrefix(th.TaskBlocker.Render("X"))
		done := sgrPrefix(th.TaskDone.Render("X"))
		if blocker == "" || done == "" {
			t.Fatal("styles emitted no SGR under forced ANSI")
		}
		c := cfg
		c.Theme = &th
		m := NewTaskModelWithFilter(items, c, nil)
		m = applyTaskMsg(m, tea.WindowSizeMsg{Width: 120, Height: 40})
		frame := m.View()
		if !strings.Contains(frame, blocker) {
			t.Fatalf("frame missing TaskBlocker SGR %q", blocker)
		}
		if !strings.Contains(frame, done) {
			t.Fatalf("frame missing TaskDone SGR %q", done)
		}
	})
}

func TestTaskActions(t *testing.T) {
	t.Run("toggle open to done writes file", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 alpha\n  body\n- [x] PRJ-002 beta\n")
		before := readFile(t, "tasks.md")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('x'))
		if !m.FilteredItems()[0].Checked {
			t.Fatalf("in-memory item not toggled: %+v", m.FilteredItems()[0])
		}
		after := readFile(t, "tasks.md")
		if after == before {
			t.Fatal("file unchanged after toggle")
		}
		if !strings.HasPrefix(after, "- [x] PRJ-001 alpha\n  body\n") {
			t.Fatalf("header/body not flipped:\n%s", after)
		}
	})

	t.Run("toggle done to open writes file", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 alpha\n- [x] PRJ-002 beta\n")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: ".", ShowDone: true}, nil)
		m = applyTaskKey(m, runeKey('j'))
		if id := m.FilteredItems()[m.CursorIndex()].ID; id != "PRJ-002" {
			t.Fatalf("cursor on %s, want PRJ-002", id)
		}
		m = applyTaskKey(m, runeKey('x'))
		after := readFile(t, "tasks.md")
		if !strings.HasPrefix(after, "- [ ] PRJ-001 alpha\n- [ ] PRJ-002 beta") {
			t.Fatalf("done not flipped back:\n%s", after)
		}
	})

	t.Run("stale toggle leaves file untouched", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 alpha\n  body\n- [ ] PRJ-002 beta\n")
		mutated := "- [ ] OPS-100 intruder\n" + readFile(t, "tasks.md")
		writeRaw(t, "tasks.md", mutated)
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('x'))
		if !strings.Contains(m.Status(), "file changed") {
			t.Fatalf("status = %q, want file changed message", m.Status())
		}
		if got := readFile(t, "tasks.md"); got != mutated {
			t.Fatal("file modified despite stale error")
		}
	})

	t.Run("b toggles showBlocked and filtered count", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 alpha\n- [ ] PRJ-003 blocked @blocked_by:PRJ-999\n")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		if n := len(m.FilteredItems()); n != 1 {
			t.Fatalf("filtered = %d, want 1 (blocked hidden)", n)
		}
		m = applyTaskKey(m, runeKey('b'))
		if !m.ShowBlocked() {
			t.Fatal("b did not set showBlocked")
		}
		if n := len(m.FilteredItems()); n != 2 {
			t.Fatalf("filtered = %d, want 2", n)
		}
	})

	t.Run("abort result", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 alpha\n")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, key(tea.KeyEsc))
		if res := m.Result(); !res.Aborted {
			t.Fatalf("Result() = %+v, want Aborted", res)
		}
	})

	t.Run("open without EDITOR", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 alpha\n")
		t.Setenv("EDITOR", "")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('o'))
		if !strings.Contains(m.Status(), "$EDITOR is not set") {
			t.Fatalf("status = %q", m.Status())
		}
	})
}

func TestTaskEditTitle(t *testing.T) {
	t.Run("save preserves metadata and body", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 Fix login #auth !P1 @owner:me\n  keep me\n- [ ] PRJ-002 second\n")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('t'))
		if m.TaskMode() != taskModeEditTitle {
			t.Fatalf("t mode = %v", m.TaskMode())
		}
		if m.editor.Value() != m.FilteredItems()[0].HeaderRaw {
			t.Fatalf("editor seeded %q, want %q", m.editor.Value(), m.FilteredItems()[0].HeaderRaw)
		}
		m.editor.SetValue("- [ ] PRJ-001 Renamed task #auth !P1 @owner:me")
		m = applyTaskKey(m, tea.KeyMsg{Type: tea.KeyCtrlS})
		after := readFile(t, "tasks.md")
		if !strings.HasPrefix(after, "- [ ] PRJ-001 Renamed task #auth !P1 @owner:me\n  keep me\n") {
			t.Fatalf("header not replaced or body lost:\n%s", after)
		}
		tk := findLoadedTask(t, "PRJ-001")
		if tk.Title != "Renamed task" {
			t.Fatalf("title = %q", tk.Title)
		}
		if !containsString(tk.Tags, "auth") || tk.Priority != "P1" || tk.Properties["owner"] != "me" {
			t.Fatalf("metadata not preserved: %+v", tk)
		}
	})

	t.Run("invalid header refused", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 alpha\n")
		before := readFile(t, "tasks.md")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('t'))
		m.editor.SetValue("not a task header")
		m = applyTaskKey(m, tea.KeyMsg{Type: tea.KeyCtrlS})
		if !strings.Contains(m.Status(), "invalid task header") {
			t.Fatalf("status = %q", m.Status())
		}
		if m.TaskMode() != taskModeEditTitle {
			t.Fatalf("invalid save closed the editor: mode=%v", m.TaskMode())
		}
		if got := readFile(t, "tasks.md"); got != before {
			t.Fatal("file changed on invalid header")
		}
	})

	t.Run("esc cancels without write", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 alpha\n")
		before := readFile(t, "tasks.md")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('t'))
		m.editor.SetValue("- [ ] PRJ-001 changed")
		m = applyTaskKey(m, key(tea.KeyEsc))
		if got := readFile(t, "tasks.md"); got != before {
			t.Fatal("file changed on esc")
		}
	})

	t.Run("side panel tracks parsed metadata", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 alpha\n")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('t'))
		m.editor.SetValue("- [ ] PRJ-001 tuned #side !P9 @k:v")
		frame := stripANSI(m.View())
		for _, want := range []string{"tuned", "#side", "!P9", "@k", "v"} {
			if !strings.Contains(frame, want) {
				t.Fatalf("side panel missing %q:\n%s", want, frame)
			}
		}
	})

	t.Run("n appends a new block", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 alpha\n")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('n'))
		if m.TaskMode() != taskModeEditTitle {
			t.Fatalf("n mode = %v", m.TaskMode())
		}
		if m.editor.Value() != "- [ ] " {
			t.Fatalf("n seeded %q", m.editor.Value())
		}
		m.editor.SetValue("- [ ] NEW-001 added by n")
		m = applyTaskKey(m, tea.KeyMsg{Type: tea.KeyCtrlS})
		after := readFile(t, "tasks.md")
		if !strings.Contains(after, "- [ ] NEW-001 added by n") {
			t.Fatalf("new task not appended:\n%s", after)
		}
		if _, ok := task.ParseHeader("- [ ] NEW-001 added by n"); !ok {
			t.Fatal("appended header does not parse")
		}
	})
}

func TestTaskEditBody(t *testing.T) {
	t.Run("save two-line body", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 title\n  original\n\n- [ ] PRJ-002 other\n")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('e'))
		if m.TaskMode() != taskModeEditBody {
			t.Fatalf("e mode = %v", m.TaskMode())
		}
		if m.editor.Value() != "original" {
			t.Fatalf("editor seeded %q, want dedented body", m.editor.Value())
		}
		m.editor.SetValue("line one\nline two")
		m = applyTaskKey(m, tea.KeyMsg{Type: tea.KeyCtrlS})
		after := readFile(t, "tasks.md")
		if !strings.Contains(after, "- [ ] PRJ-001 title\n  line one\n  line two\n") {
			t.Fatalf("body block wrong:\n%s", after)
		}
	})

	t.Run("blank lines stay blank", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 title\n  original\n")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('e'))
		m.editor.SetValue("a\n\nb")
		m = applyTaskKey(m, tea.KeyMsg{Type: tea.KeyCtrlS})
		after := readFile(t, "tasks.md")
		if !strings.Contains(after, "- [ ] PRJ-001 title\n  a\n\n  b\n") {
			t.Fatalf("blank line not preserved:\n%s", after)
		}
	})

	t.Run("empty save on bodyless task is byte-identical", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 title\n- [ ] PRJ-002 other\n")
		before := readFile(t, "tasks.md")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('j'))
		if m.FilteredItems()[m.CursorIndex()].ID != "PRJ-002" {
			t.Fatalf("cursor not on PRJ-002")
		}
		m = applyTaskKey(m, runeKey('e'))
		if m.editor.Value() != "" {
			t.Fatalf("editor seeded %q, want empty", m.editor.Value())
		}
		m = applyTaskKey(m, tea.KeyMsg{Type: tea.KeyCtrlS})
		if got := readFile(t, "tasks.md"); got != before {
			t.Fatalf("empty save changed file:\n%q\nwant\n%q", got, before)
		}
	})

	t.Run("header-shaped body line stays body", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 title\n  original\n")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('e'))
		m.editor.SetValue("- [ ] X-001 nested")
		m = applyTaskKey(m, tea.KeyMsg{Type: tea.KeyCtrlS})
		after := readFile(t, "tasks.md")
		if !strings.Contains(after, "- [ ] PRJ-001 title\n  - [ ] X-001 nested\n") {
			t.Fatalf("nested header not written as body:\n%s", after)
		}
		tasks, _, err := task.LoadAll(".", task.Config{})
		if err != nil {
			t.Fatal(err)
		}
		if len(tasks) != 1 {
			t.Fatalf("parser found %d tasks, want 1: %+v", len(tasks), tasks)
		}
	})

	t.Run("four-space indent round-trips", func(t *testing.T) {
		items := newTaskVault(t, "- [ ] PRJ-001 title\n    four space body\n")
		before := readFile(t, "tasks.md")
		m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
		m = applyTaskKey(m, runeKey('e'))
		if m.editor.Value() != "four space body" {
			t.Fatalf("editor seeded %q", m.editor.Value())
		}
		m = applyTaskKey(m, tea.KeyMsg{Type: tea.KeyCtrlS})
		if got := readFile(t, "tasks.md"); got != before {
			t.Fatalf("round-trip changed file:\n%q\nwant\n%q", got, before)
		}
	})
}

func TestTaskMove(t *testing.T) {
	dir := t.TempDir()
	writeTaskFile(t, dir, "tasks.md", "- [ ] PRJ-001 alpha\n  body one\n- [ ] PRJ-002 beta\n")
	writeTaskFile(t, dir, "target.md", "# Target\n\n")
	t.Chdir(dir)
	items := loadTaskItems(t, ".")

	m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
	m = applyTaskKey(m, runeKey('m'))
	if m.TaskMode() != taskModeMovePrompt {
		t.Fatalf("m mode = %v, want move prompt", m.TaskMode())
	}
	if got := m.moveInput.Value(); got != "tasks.md" {
		t.Fatalf("move prompt seeded %q, want tasks.md", got)
	}
	m.moveInput.SetValue("target.md")
	m = applyTaskKey(m, tea.KeyMsg{Type: tea.KeyEnter})

	if got, want := readFile(t, "tasks.md"), "- [ ] PRJ-002 beta\n"; got != want {
		t.Fatalf("source = %q, want %q", got, want)
	}
	if got, want := readFile(t, "target.md"), "# Target\n\n- [ ] PRJ-001 alpha\n  body one\n"; got != want {
		t.Fatalf("target = %q, want %q", got, want)
	}
	cur := m.FilteredItems()[m.CursorIndex()]
	if cur.ID != "PRJ-001" || cur.File != "target.md" {
		t.Fatalf("cursor on %q in %q, want PRJ-001 in target.md", cur.ID, cur.File)
	}
}

func TestTaskArchive(t *testing.T) {
	dir := t.TempDir()
	writeTaskFile(t, dir, "tasks.md", "- [ ] PRJ-001 open\n- [x] PRJ-002 done\n  done body\n")
	t.Chdir(dir)
	items := loadTaskItems(t, ".")

	m := NewTaskModelWithFilter(items, TaskConfig{
		Base:     ".",
		ShowDone: true,
		Config:   task.Config{ArchivePath: "_archive.md"},
	}, nil)
	m = applyTaskKey(m, runeKey('j'))
	if id := m.FilteredItems()[m.CursorIndex()].ID; id != "PRJ-002" {
		t.Fatalf("cursor on %q, want PRJ-002", id)
	}
	m = applyTaskKey(m, runeKey('a'))

	if got, want := readFile(t, "tasks.md"), "- [ ] PRJ-001 open\n"; got != want {
		t.Fatalf("source = %q, want %q", got, want)
	}
	if got, want := readFile(t, "_archive.md"), "- [x] PRJ-002 done\n  done body\n"; got != want {
		t.Fatalf("archive = %q, want %q", got, want)
	}
}

func TestTaskArchiveNotDone(t *testing.T) {
	dir := t.TempDir()
	writeTaskFile(t, dir, "tasks.md", "- [ ] PRJ-001 open\n")
	t.Chdir(dir)
	items := loadTaskItems(t, ".")

	m := NewTaskModelWithFilter(items, TaskConfig{
		Base:   ".",
		Config: task.Config{ArchivePath: "_archive.md"},
	}, nil)
	m = applyTaskKey(m, runeKey('a'))

	if !strings.Contains(m.Status(), "task is not done") {
		t.Fatalf("status = %q, want task is not done", m.Status())
	}
	if _, err := os.Stat("_archive.md"); !os.IsNotExist(err) {
		t.Fatalf("archive created on open task: err=%v", err)
	}
	if got := readFile(t, "tasks.md"); got != "- [ ] PRJ-001 open\n" {
		t.Fatalf("source mutated: %q", got)
	}
}
