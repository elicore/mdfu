package task

import (
	"path/filepath"
	"testing"
)

func TestParseCheckbox(t *testing.T) {
	tests := []struct {
		name    string
		line    string
		ok      bool
		id      string
		title   string
		checked bool
		seeded  bool
		unident bool
		tags    []string
	}{
		{name: "plain unidentified", line: "- [ ] buy milk", ok: true, title: "buy milk", unident: true},
		{name: "star bullet nested", line: "  * [x] nested done #home", ok: true, title: "nested done", checked: true, unident: true, tags: []string{"home"}},
		{name: "plus bullet uppercase", line: "+ [X] upper", ok: true, title: "upper", checked: true, unident: true},
		{name: "tab indent", line: "\t- [ ] tabbed", ok: true, title: "tabbed", unident: true},
		{name: "identified", line: "- [ ] PRJ-42 do it", ok: true, id: "PRJ-42", title: "do it"},
		{name: "identified with metadata", line: "* [x] PRJ-007 done #a !high @k:v", ok: true, id: "PRJ-007", title: "done", checked: true, tags: []string{"a"}},
		{name: "seed", line: "  + [ ] PRJ- placeholder", ok: true, title: "placeholder", seeded: true},
		{name: "not a task", line: "just text", ok: false},
		{name: "no space after bracket", line: "- [ ]x", ok: false},
		{name: "blockquote not supported", line: "> - [ ] quoted", ok: false},
		{name: "ordered list not a checkbox", line: "1. [ ] item", ok: false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, ok := ParseCheckbox(tt.line)
			if ok != tt.ok {
				t.Fatalf("ParseCheckbox(%q) ok = %v, want %v", tt.line, ok, tt.ok)
			}
			if !ok {
				return
			}
			if !got.Broad {
				t.Error("Broad = false, want true")
			}
			if got.ID != tt.id {
				t.Errorf("ID = %q, want %q", got.ID, tt.id)
			}
			if got.Title != tt.title {
				t.Errorf("Title = %q, want %q", got.Title, tt.title)
			}
			if got.Checked != tt.checked {
				t.Errorf("Checked = %v, want %v", got.Checked, tt.checked)
			}
			if got.Seeded != tt.seeded {
				t.Errorf("Seeded = %v, want %v", got.Seeded, tt.seeded)
			}
			if got.Unidentified != tt.unident {
				t.Errorf("Unidentified = %v, want %v", got.Unidentified, tt.unident)
			}
			if len(got.Tags) != len(tt.tags) {
				t.Fatalf("Tags = %v, want %v", got.Tags, tt.tags)
			}
			for i := range tt.tags {
				if got.Tags[i] != tt.tags[i] {
					t.Errorf("Tags = %v, want %v", got.Tags, tt.tags)
					break
				}
			}
			if got.HeaderRaw != tt.line {
				t.Errorf("HeaderRaw = %q, want %q", got.HeaderRaw, tt.line)
			}
		})
	}
}

func TestFlipCheckboxAny(t *testing.T) {
	tests := []struct {
		in   string
		want string
		ok   bool
	}{
		{in: "- [ ] a", want: "- [x] a", ok: true},
		{in: "  * [x] b", want: "  * [ ] b", ok: true},
		{in: "+ [X] c", want: "+ [ ] c", ok: true},
		{in: "\t- [ ] PRJ-1 title #t !high", want: "\t- [x] PRJ-1 title #t !high", ok: true},
		{in: "- [ ]", ok: false},
		{in: "not a checkbox", ok: false},
	}
	for _, tt := range tests {
		got, ok := FlipCheckboxAny(tt.in)
		if ok != tt.ok {
			t.Fatalf("FlipCheckboxAny(%q) ok = %v, want %v", tt.in, ok, tt.ok)
		}
		if ok && got != tt.want {
			t.Errorf("FlipCheckboxAny(%q) = %q, want %q", tt.in, got, tt.want)
		}
	}
}

func TestLoadCheckboxes(t *testing.T) {
	root := t.TempDir()
	t.Chdir(root)
	mustWrite(t, root, "tasks.md", "# Notes\n\n- [ ] plain\n  - [x] nested #home\n    nested body\n* [X] star\n+ [ ] PRJ-9 plus\n- [ ] PRJ- seed\n")

	tasks, err := LoadCheckboxes(Scope{Base: ".", Cwd: root})
	if err != nil {
		t.Fatalf("LoadCheckboxes: %v", err)
	}
	if len(tasks) != 5 {
		t.Fatalf("got %d tasks, want 5: %+v", len(tasks), tasks)
	}

	plain := tasks[0]
	if plain.ID != "" || plain.Title != "plain" || plain.File != "tasks.md" || plain.Line != 3 {
		t.Errorf("plain = %+v", plain)
	}
	if plain.Body != "" {
		t.Errorf("plain.Body = %q, want empty (nested checkbox ends the block)", plain.Body)
	}
	nested := tasks[1]
	if nested.Title != "nested" || !nested.Checked || nested.Body != "nested body" {
		t.Errorf("nested = %+v", nested)
	}
	if tasks[2].Title != "star" || !tasks[2].Checked {
		t.Errorf("star = %+v", tasks[2])
	}
	if tasks[3].ID != "PRJ-9" || tasks[3].Title != "plus" {
		t.Errorf("plus = %+v", tasks[3])
	}
	if !tasks[4].Seeded || tasks[4].SeedPrefix != "PRJ" {
		t.Errorf("seed = %+v", tasks[4])
	}
}

func TestLoadCheckboxesNestedBody(t *testing.T) {
	root := t.TempDir()
	t.Chdir(root)
	mustWrite(t, root, "tasks.md", "- [ ] parent\n  parent note\n  - [ ] child\n    child note\n- [ ] sibling\n")

	tasks, err := LoadCheckboxes(Scope{Base: ".", Cwd: root})
	if err != nil {
		t.Fatalf("LoadCheckboxes: %v", err)
	}
	if len(tasks) != 3 {
		t.Fatalf("got %d tasks, want 3: %+v", len(tasks), tasks)
	}
	if tasks[0].Title != "parent" || tasks[0].Body != "parent note" {
		t.Errorf("parent = %+v", tasks[0])
	}
	if tasks[1].Title != "child" || tasks[1].Body != "child note" {
		t.Errorf("child = %+v", tasks[1])
	}
	if tasks[2].Title != "sibling" || tasks[2].Body != "" {
		t.Errorf("sibling = %+v", tasks[2])
	}
}

func TestLoadCheckboxesFenceMask(t *testing.T) {
	root := t.TempDir()
	t.Chdir(root)
	mustWrite(t, root, "tasks.md", "- [ ] real\n  ```\n  - [ ] fake\n  ```\n- [ ] also real\n")

	tasks, err := LoadCheckboxes(Scope{Base: ".", Cwd: root})
	if err != nil {
		t.Fatalf("LoadCheckboxes: %v", err)
	}
	if len(tasks) != 2 {
		t.Fatalf("got %d tasks, want 2: %+v", len(tasks), tasks)
	}
	if tasks[0].Title != "real" || tasks[1].Title != "also real" {
		t.Errorf("titles = %q, %q", tasks[0].Title, tasks[1].Title)
	}
}

func TestMoveTaskBroad(t *testing.T) {
	root := t.TempDir()
	t.Chdir(root)
	mustWrite(t, root, "a.md", "- plain\n* [ ] star item\n  star body\n- [ ] other\n")
	mustWrite(t, root, "b.md", "# B\n")

	tasks, err := LoadCheckboxes(Scope{Base: ".", Cwd: root})
	if err != nil {
		t.Fatalf("LoadCheckboxes: %v", err)
	}
	var star Task
	for _, tk := range tasks {
		if tk.Title == "star item" {
			star = tk
		}
	}
	if star.Title == "" {
		t.Fatal("star item not loaded")
	}
	if err := MoveTask(star, "b.md"); err != nil {
		t.Fatalf("MoveTask: %v", err)
	}
	if got, want := readTestFile(t, filepath.Join(root, "b.md")), "# B\n* [ ] star item\n  star body\n"; got != want {
		t.Errorf("target = %q, want %q", got, want)
	}
	if got, want := readTestFile(t, filepath.Join(root, "a.md")), "- plain\n- [ ] other\n"; got != want {
		t.Errorf("source = %q, want %q", got, want)
	}
}

func TestArchiveTasksBroad(t *testing.T) {
	root := t.TempDir()
	t.Chdir(root)
	mustWrite(t, root, "tasks.md", "- [ ] open\n* [x] done broad\n  broad body\n")

	tasks, err := LoadCheckboxes(Scope{Base: ".", Cwd: root})
	if err != nil {
		t.Fatalf("LoadCheckboxes: %v", err)
	}
	var done Task
	for _, tk := range tasks {
		if tk.Checked {
			done = tk
		}
	}
	if done.Title == "" {
		t.Fatal("done broad task not loaded")
	}
	if err := ArchiveTasks([]Task{done}, root, Config{}); err != nil {
		t.Fatalf("ArchiveTasks: %v", err)
	}
	if got, want := readTestFile(t, filepath.Join(root, "tasks.md")), "- [ ] open\n"; got != want {
		t.Errorf("source = %q, want %q", got, want)
	}
	if got, want := readTestFile(t, filepath.Join(root, "_archive.md")), "* [x] done broad\n  broad body\n"; got != want {
		t.Errorf("archive = %q, want %q", got, want)
	}
}

func TestLoadScopeStaysStrict(t *testing.T) {
	root := t.TempDir()
	t.Chdir(root)
	mustWrite(t, root, "tasks.md", "  - [ ] indented\n* [ ] star\n- [X] upper\n")

	tasks, unidentified, err := LoadScope(Scope{Base: ".", Cwd: root})
	if err != nil {
		t.Fatalf("LoadScope: %v", err)
	}
	if len(tasks) != 0 || len(unidentified) != 0 {
		t.Fatalf("strict LoadScope leaked broad checkboxes: tasks=%+v unidentified=%+v", tasks, unidentified)
	}

	broad, err := LoadCheckboxes(Scope{Base: ".", Cwd: root})
	if err != nil {
		t.Fatalf("LoadCheckboxes: %v", err)
	}
	if len(broad) != 3 {
		t.Fatalf("LoadCheckboxes = %d, want 3", len(broad))
	}
}

func TestVerifyTaskBroad(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "tasks.md")
	t.Chdir(root)
	mustWrite(t, root, "tasks.md", "  * [ ] buy milk\n")

	tasks, err := LoadCheckboxes(Scope{Base: ".", Cwd: root})
	if err != nil {
		t.Fatalf("LoadCheckboxes: %v", err)
	}
	if len(tasks) != 1 {
		t.Fatalf("got %d tasks, want 1", len(tasks))
	}

	fe, err := Open(path)
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	if err := fe.VerifyTask(tasks[0]); err != nil {
		t.Fatalf("VerifyTask fresh = %v, want nil", err)
	}

	mustWrite(t, root, "tasks.md", "  * [x] buy milk\n")
	if err := fe.VerifyTask(tasks[0]); err == nil {
		t.Fatal("VerifyTask after change = nil, want stale error")
	}
}
