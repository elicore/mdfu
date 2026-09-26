package task

import (
	"bytes"
	"regexp"
	"strconv"
	"strings"
	"testing"
)

func specTasks() []Task {
	return []Task{
		{
			ID: "EXMPL-001", Status: ' ', Title: "Fix the thing",
			Tags: []string{"launch"}, Priority: "high",
			PropertyOrder: []string{"status"}, Properties: map[string]string{"status": "doing"},
			File: "tasks.md", Line: 1,
		},
		{
			ID: "EXMPL-002", Status: 'x', Checked: true, Title: "Ship it",
			Tags: []string{"launch"},
			File: "tasks.md", Line: 2,
		},
	}
}

const wantTable = "╭───────────┬───────────────┬─────────┬───────┬───────────────╮\n" +
	"│ ID        │ TITLE         │ TAGS    │ PRI   │ PROPS         │\n" +
	"├───────────┼───────────────┼─────────┼───────┼───────────────┤\n" +
	"│ EXMPL-001 │ Fix the thing │ #launch │ !high │ @status:doing │\n" +
	"│ EXMPL-002 │ Ship it       │ #launch │       │               │\n" +
	"╰───────────┴───────────────┴─────────┴───────┴───────────────╯\n"

const wantLines = "[ ] EXMPL-001  Fix the thing  #launch  !high  @status:doing\n" +
	"[x] EXMPL-002  Ship it  #launch\n"

var ansiRe = regexp.MustCompile(`\x1b\[[0-9;]*m`)

func TestRenderTable(t *testing.T) {
	t.Run("TTY no color matches the SPEC literal", func(t *testing.T) {
		var buf bytes.Buffer
		opts := ListOptions{All: true, Terminal: Terminal{IsTTY: true, NoColor: true}}
		if err := RenderList(&buf, specTasks(), opts); err != nil {
			t.Fatalf("RenderList error = %v", err)
		}
		if buf.String() != wantTable {
			t.Errorf("table =\n%s\nwant\n%s", buf.String(), wantTable)
		}
	})

	t.Run("TTY color strips back to the literal", func(t *testing.T) {
		var buf bytes.Buffer
		opts := ListOptions{All: true, Terminal: Terminal{IsTTY: true}}
		if err := RenderList(&buf, specTasks(), opts); err != nil {
			t.Fatalf("RenderList error = %v", err)
		}
		if !strings.Contains(buf.String(), "\x1b[") {
			t.Errorf("TTY color output has no SGR sequence")
		}
		if got := ansiRe.ReplaceAllString(buf.String(), ""); got != wantTable {
			t.Errorf("stripped table =\n%s\nwant\n%s", got, wantTable)
		}
	})

	t.Run("empty result prints no table", func(t *testing.T) {
		var buf bytes.Buffer
		opts := ListOptions{All: true, Terminal: Terminal{IsTTY: true, NoColor: true}}
		if err := RenderList(&buf, nil, opts); err != nil {
			t.Fatalf("RenderList error = %v", err)
		}
		if buf.Len() != 0 {
			t.Errorf("output = %q, want empty", buf.String())
		}
	})
}

func TestRenderLines(t *testing.T) {
	var buf bytes.Buffer
	opts := ListOptions{All: true, Terminal: Terminal{IsTTY: false}}
	if err := RenderList(&buf, specTasks(), opts); err != nil {
		t.Fatalf("RenderList error = %v", err)
	}
	if buf.String() != wantLines {
		t.Errorf("lines = %q, want %q", buf.String(), wantLines)
	}
	if strings.ContainsRune(buf.String(), '\x1b') {
		t.Errorf("non-TTY output contains an ESC byte: %q", buf.String())
	}
}

func TestRenderJSON(t *testing.T) {
	t.Run("escapes no html and keeps tags and properties present", func(t *testing.T) {
		tasks := []Task{{
			ID: "X-1", Title: "<b>&</b>", Status: ' ',
			File: "f.md", Line: 3,
		}}
		var buf bytes.Buffer
		opts := ListOptions{All: true, JSON: true, Terminal: Terminal{IsTTY: true}}
		if err := RenderList(&buf, tasks, opts); err != nil {
			t.Fatalf("RenderList error = %v", err)
		}
		got := buf.String()
		if strings.Contains(got, `\u003c`) || strings.Contains(got, `\u0026`) {
			t.Errorf("JSON escaped HTML: %s", got)
		}
		for _, want := range []string{`"tags": []`, `"properties": {}`, `"priority": "medium"`, `"status": "open"`, `<b>&</b>`} {
			if !strings.Contains(got, want) {
				t.Errorf("JSON missing %q:\n%s", want, got)
			}
		}
		assertKeyOrder(t, got, `"id"`, `"title"`, `"status"`, `"priority"`, `"tags"`, `"properties"`, `"file"`, `"line"`)
	})

	t.Run("empty result is an empty array", func(t *testing.T) {
		var buf bytes.Buffer
		opts := ListOptions{JSON: true, Terminal: Terminal{IsTTY: false}}
		if err := RenderList(&buf, nil, opts); err != nil {
			t.Fatalf("RenderList error = %v", err)
		}
		if got, want := buf.String(), "[]\n"; got != want {
			t.Errorf("JSON = %q, want %q", got, want)
		}
	})

	t.Run("json suppresses notes", func(t *testing.T) {
		tasks := specTasks()
		var buf bytes.Buffer
		opts := ListOptions{
			All: true, JSON: true, Terminal: Terminal{IsTTY: true},
			HiddenBlocked: []Task{{ID: "B-1", HeaderRaw: "- [ ] B-1 Blocked", File: "f.md", Line: 9}},
			Unidentified:  []Unidentified{{File: "f.md", Line: 10, Raw: "- [ ] No ID"}},
		}
		if err := RenderList(&buf, tasks, opts); err != nil {
			t.Fatalf("RenderList error = %v", err)
		}
		if strings.Contains(buf.String(), "Warning:") || strings.Contains(buf.String(), "have no ID") {
			t.Errorf("JSON output contains a note: %s", buf.String())
		}
	})
}

func TestSortPriority(t *testing.T) {
	tasks := []Task{
		{ID: "A-1", Priority: "low"},
		{ID: "B-1", Priority: "crit"},
		{ID: "C-1"},
		{ID: "D-1", Priority: "high"},
		{ID: "E-1", Priority: "crit"},
	}

	t.Run("orders crit high medium low stably", func(t *testing.T) {
		var buf bytes.Buffer
		opts := ListOptions{All: true, JSON: true, Sort: "priority", Terminal: Terminal{IsTTY: false}}
		if err := RenderList(&buf, tasks, opts); err != nil {
			t.Fatalf("RenderList error = %v", err)
		}
		got := buf.String()
		want := []string{"B-1", "E-1", "D-1", "C-1", "A-1"}
		prev := -1
		for _, id := range want {
			idx := strings.Index(got, `"`+id+`"`)
			if idx < 0 {
				t.Fatalf("id %s missing", id)
			}
			if idx < prev {
				t.Fatalf("id %s out of order in:\n%s", id, got)
			}
			prev = idx
		}
	})

	t.Run("unknown sort leaves file order untouched", func(t *testing.T) {
		var buf bytes.Buffer
		opts := ListOptions{All: true, JSON: true, Sort: "bogus", Terminal: Terminal{IsTTY: false}}
		if err := RenderList(&buf, tasks, opts); err != nil {
			t.Fatalf("RenderList error = %v", err)
		}
		got := buf.String()
		prev := -1
		for _, id := range []string{"A-1", "B-1", "C-1", "D-1", "E-1"} {
			idx := strings.Index(got, `"`+id+`"`)
			if idx < 0 || idx < prev {
				t.Fatalf("file order broken for %s in:\n%s", id, got)
			}
			prev = idx
		}
	})
}

func TestRenderNotes(t *testing.T) {
	hidden := []Task{
		{ID: "B-1", HeaderRaw: "- [ ] B-1 Short", File: "a.md", Line: 4},
		{ID: "B-2", HeaderRaw: "- [ ] B-2 A much longer line", File: "b.md", Line: 12},
	}
	unidentified := []Unidentified{{File: "a.md", Line: 9, Raw: "- [ ] No ID"}}
	var buf bytes.Buffer
	opts := ListOptions{
		Terminal:      Terminal{IsTTY: false},
		HiddenBlocked: hidden,
		Unidentified:  unidentified,
	}
	if err := RenderList(&buf, nil, opts); err != nil {
		t.Fatalf("RenderList error = %v", err)
	}
	got := buf.String()
	if !strings.Contains(got, "Warning: 2 blocked task(s) hidden (use `mdfu task list --blocked` to show them):\n") {
		t.Errorf("missing note 1 header: %q", got)
	}
	if !strings.Contains(got, "1 task(s) have no ID (run `mdfu task ids` to assign one).\n") {
		t.Errorf("missing note 2: %q", got)
	}
	widest := 0
	for _, task := range hidden {
		if w := len(task.HeaderRaw); w > widest {
			widest = w
		}
	}
	for _, task := range hidden {
		loc := task.File + ":" + strconv.Itoa(task.Line)
		want := task.HeaderRaw + strings.Repeat(" ", widest-len(task.HeaderRaw)+2) + loc + "\n"
		if !strings.Contains(got, want) {
			t.Errorf("missing detail line %q in:\n%s", want, got)
		}
	}
}

// assertKeyOrder checks that each key occurs after the previous one.
func assertKeyOrder(t *testing.T, s string, keys ...string) {
	t.Helper()
	prev := -1
	for _, key := range keys {
		idx := strings.Index(s, key)
		if idx < 0 {
			t.Fatalf("key %s missing from %s", key, s)
		}
		if idx < prev {
			t.Fatalf("key %s is out of order in:\n%s", key, s)
		}
		prev = idx
	}
}
