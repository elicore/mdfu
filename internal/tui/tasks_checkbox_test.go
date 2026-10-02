package tui

import (
	"os"
	"strings"
	"testing"

	tea "github.com/charmbracelet/bubbletea"

	"github.com/elicore/mdfu/internal/task"
)

// loadTaskItemsAny loads every checkbox (id or not) through the broadened
// loader, matching what the tasks TUI entry point now passes to the model.
func loadTaskItemsAny(t *testing.T, base string) []TaskItem {
	t.Helper()
	cwd, _ := os.Getwd()
	tasks, err := task.LoadCheckboxes(task.Scope{Base: base, Cwd: cwd})
	if err != nil {
		t.Fatalf("LoadCheckboxes(%q): %v", base, err)
	}
	items := make([]TaskItem, 0, len(tasks))
	for _, tk := range tasks {
		items = append(items, TaskItem{Task: tk})
	}
	return items
}

func newAnyVault(t *testing.T, content string) []TaskItem {
	t.Helper()
	dir := t.TempDir()
	writeTaskFile(t, dir, "tasks.md", content)
	t.Chdir(dir)
	return loadTaskItemsAny(t, ".")
}

func TestTaskModelShowsAnyCheckbox(t *testing.T) {
	items := newAnyVault(t, "- [ ] plain\n  - [x] nested\n* [ ] star\n- [ ] PRJ-1 identified\n")
	m := NewTaskModelWithFilter(items, TaskConfig{Base: ".", ShowDone: true}, nil)
	if n := len(m.AllItems()); n != 4 {
		t.Fatalf("AllItems = %d, want 4: %+v", n, m.AllItems())
	}
	frame := stripANSI(m.View())
	for _, want := range []string{"plain", "nested", "star", "identified"} {
		if !strings.Contains(frame, want) {
			t.Fatalf("frame missing %q:\n%s", want, frame)
		}
	}
}

func TestTaskToggleAnyCheckbox(t *testing.T) {
	tests := []struct {
		name  string
		vault string
		want  string
	}{
		{name: "star bullet", vault: "  * [ ] buy milk\n", want: "  * [x] buy milk\n"},
		{name: "plus bullet", vault: "+ [ ] buy eggs\n", want: "+ [x] buy eggs\n"},
		{name: "uppercase X to open", vault: "- [X] shipped\n", want: "- [ ] shipped\n"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			items := newAnyVault(t, tt.vault)
			m := NewTaskModelWithFilter(items, TaskConfig{Base: ".", ShowDone: true}, nil)
			m = applyTaskKey(m, runeKey('x'))
			if got := readFile(t, "tasks.md"); got != tt.want {
				t.Fatalf("file = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestTaskToggleAnyCheckboxStale(t *testing.T) {
	items := newAnyVault(t, "  * [ ] buy milk\n")
	mutated := "# header\n  * [ ] buy milk\n"
	writeRaw(t, "tasks.md", mutated)
	m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
	m = applyTaskKey(m, runeKey('x'))
	if !strings.Contains(m.Status(), "file changed") {
		t.Fatalf("status = %q, want file changed", m.Status())
	}
	if got := readFile(t, "tasks.md"); got != mutated {
		t.Fatal("file modified on stale broad toggle")
	}
}

func TestTaskEditTitleBroadPreservesPrefix(t *testing.T) {
	items := newAnyVault(t, "  + [ ] old title\n")
	m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
	m = applyTaskKey(m, runeKey('t'))
	if m.TaskMode() != taskModeEditTitle {
		t.Fatalf("t mode = %v", m.TaskMode())
	}
	m.editor.SetValue("  + [x] new title")
	m = applyTaskKey(m, tea.KeyMsg{Type: tea.KeyCtrlS})
	if got, want := readFile(t, "tasks.md"), "  + [x] new title\n"; got != want {
		t.Fatalf("file = %q, want %q", got, want)
	}
}

func TestTaskEditBodyBroad(t *testing.T) {
	items := newAnyVault(t, "  - [ ] task\n    body line\n")
	m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
	m = applyTaskKey(m, runeKey('e'))
	if m.TaskMode() != taskModeEditBody {
		t.Fatalf("e mode = %v", m.TaskMode())
	}
	if got := m.editor.Value(); got != "body line" {
		t.Fatalf("editor seeded %q", got)
	}
	m.editor.SetValue("changed")
	m = applyTaskKey(m, tea.KeyMsg{Type: tea.KeyCtrlS})
	if got, want := readFile(t, "tasks.md"), "  - [ ] task\n    changed\n"; got != want {
		t.Fatalf("file = %q, want %q", got, want)
	}
}

func TestTaskMoveBroad(t *testing.T) {
	dir := t.TempDir()
	writeTaskFile(t, dir, "tasks.md", "* [ ] star item\n  star body\n- [ ] tail\n")
	writeTaskFile(t, dir, "target.md", "# Target\n\n")
	t.Chdir(dir)
	items := loadTaskItemsAny(t, ".")

	m := NewTaskModelWithFilter(items, TaskConfig{Base: "."}, nil)
	m = applyTaskKey(m, runeKey('m'))
	m.moveInput.SetValue("target.md")
	m = applyTaskKey(m, tea.KeyMsg{Type: tea.KeyEnter})

	if got, want := readFile(t, "tasks.md"), "- [ ] tail\n"; got != want {
		t.Fatalf("source = %q, want %q", got, want)
	}
	if got, want := readFile(t, "target.md"), "# Target\n\n* [ ] star item\n  star body\n"; got != want {
		t.Fatalf("target = %q, want %q", got, want)
	}
	cur := m.FilteredItems()[m.CursorIndex()]
	if cur.Title != "star item" || cur.File != "target.md" {
		t.Fatalf("cursor on %q in %q, want star item in target.md", cur.Title, cur.File)
	}
}

func TestTaskArchiveBroad(t *testing.T) {
	dir := t.TempDir()
	writeTaskFile(t, dir, "tasks.md", "- [ ] open\n* [x] done broad\n  broad body\n")
	t.Chdir(dir)
	items := loadTaskItemsAny(t, ".")

	m := NewTaskModelWithFilter(items, TaskConfig{
		Base:     ".",
		ShowDone: true,
		Config:   task.Config{ArchivePath: "_archive.md"},
	}, nil)
	m = applyTaskKey(m, runeKey('j'))
	m = applyTaskKey(m, runeKey('a'))

	if got, want := readFile(t, "tasks.md"), "- [ ] open\n"; got != want {
		t.Fatalf("source = %q, want %q", got, want)
	}
	if got, want := readFile(t, "_archive.md"), "* [x] done broad\n  broad body\n"; got != want {
		t.Fatalf("archive = %q, want %q", got, want)
	}
}

func TestTaskDuplicateLineKeys(t *testing.T) {
	items := newAnyVault(t, "- [ ] same\n- [ ] same\n")
	if len(items) != 2 {
		t.Fatalf("got %d items, want 2", len(items))
	}
	if taskItemKey(items[0]) == taskItemKey(items[1]) {
		t.Fatalf("duplicate lines share key %q", taskItemKey(items[0]))
	}
}
