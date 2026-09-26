package task

import (
	"bytes"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// commandEnv returns an Env with buffers and a fresh temp Cwd, plus that Cwd so
// callers can lay down a task file. It satisfies command-layer tests that need
// to choose the terminal mode.
func commandEnv(t *testing.T, term Terminal) (Env, *bytes.Buffer, *bytes.Buffer, string) {
	t.Helper()
	dir := t.TempDir()
	var stdout, stderr bytes.Buffer
	env := Env{
		Stdout:   &stdout,
		Stderr:   &stderr,
		Stdin:    strings.NewReader(""),
		Terminal: term,
		Cwd:      dir,
		Now:      func() time.Time { return time.Unix(0, 0) },
	}
	return env, &stdout, &stderr, dir
}

const listVault = "- [ ] LIST-1 Alpha #one !high\n" +
	"- [ ] LIST-2 Beta #one #two !low\n" +
	"- [ ] LIST-3 Gamma #two\n" +
	"- [x] LIST-4 Delta #one\n"

const listBlockedVault = "- [x] BLK-1 Done\n" +
	"- [ ] BLK-2 Blocked by open @blocked_by:BLK-3\n" +
	"- [ ] BLK-3 Open @blocked_by:BLK-404\n" +
	"- [ ] BLK-4 Free\n"

// wantSpecTable is the SPEC.md §F.3 literal for the two-task set used by
// TestListTable.
const wantSpecTable = "╭───────────┬───────────────┬─────────┬───────┬───────────────╮\n" +
	"│ ID        │ TITLE         │ TAGS    │ PRI   │ PROPS         │\n" +
	"├───────────┼───────────────┼─────────┼───────┼───────────────┤\n" +
	"│ EXMPL-001 │ Fix the thing │ #launch │ !high │ @status:doing │\n" +
	"│ EXMPL-002 │ Ship it       │ #launch │       │               │\n" +
	"╰───────────┴───────────────┴─────────┴───────┴───────────────╯\n"

func TestListTable(t *testing.T) {
	env, stdout, stderr, dir := commandEnv(t, Terminal{IsTTY: true, NoColor: true})
	file := filepath.Join(dir, "tasks.md")
	mustWrite(t, dir, "tasks.md",
		"- [ ] EXMPL-001 Fix the thing\t\t#launch !high @status:doing\n"+
			"- [x] EXMPL-002 Ship it\t\t#launch\n")

	res := runList(env, []string{"--path", file, "--all"})
	if res.Code != 0 {
		t.Fatalf("runList().Code = %d, want 0 (stderr=%q)", res.Code, stderr.String())
	}
	if got := stdout.String(); got != wantSpecTable {
		t.Errorf("table =\n%s\nwant\n%s", got, wantSpecTable)
	}
}

func TestList(t *testing.T) {
	tests := []struct {
		name    string
		content string
		args    []string
		want    string
	}{
		{
			name:    "default",
			content: listVault,
			want: "[ ] LIST-1  Alpha  #one  !high\n" +
				"[ ] LIST-2  Beta  #one #two  !low\n" +
				"[ ] LIST-3  Gamma  #two\n",
		},
		{
			name:    "all",
			content: listVault,
			args:    []string{"--all"},
			want: "[ ] LIST-1  Alpha  #one  !high\n" +
				"[ ] LIST-2  Beta  #one #two  !low\n" +
				"[ ] LIST-3  Gamma  #two\n" +
				"[x] LIST-4  Delta  #one\n",
		},
		{
			name:    "blocked",
			content: listBlockedVault,
			args:    []string{"--blocked"},
			want: "[ ] BLK-2  Blocked by open  @blocked_by:BLK-3\n" +
				"[ ] BLK-3  Open  @blocked_by:BLK-404\n" +
				"[ ] BLK-4  Free\n",
		},
		{
			name:    "sort priority",
			content: listVault,
			args:    []string{"--sort=priority"},
			want: "[ ] LIST-1  Alpha  #one  !high\n" +
				"[ ] LIST-3  Gamma  #two\n" +
				"[ ] LIST-2  Beta  #one #two  !low\n",
		},
		{
			name:    "one tag flag",
			content: listVault,
			args:    []string{"--tag", "one"},
			want: "[ ] LIST-1  Alpha  #one  !high\n" +
				"[ ] LIST-2  Beta  #one #two  !low\n",
		},
		{
			name:    "two tags AND",
			content: listVault,
			args:    []string{"--tag", "one", "--tag", "two"},
			want:    "[ ] LIST-2  Beta  #one #two  !low\n",
		},
		{
			name:    "two positional tags AND",
			content: listVault,
			args:    []string{"#one", "#two"},
			want:    "[ ] LIST-2  Beta  #one #two  !low\n",
		},
		{
			name:    "priority OR",
			content: listVault,
			args:    []string{"--priority", "high", "--priority", "low"},
			want: "[ ] LIST-1  Alpha  #one  !high\n" +
				"[ ] LIST-2  Beta  #one #two  !low\n",
		},
		{
			name:    "positional tag plus tag flag",
			content: listVault,
			args:    []string{"#one", "--tag", "two"},
			want:    "[ ] LIST-2  Beta  #one #two  !low\n",
		},
		{
			name:    "positional priority plus priority flag OR",
			content: listVault,
			args:    []string{"!low", "--priority", "high"},
			want: "[ ] LIST-1  Alpha  #one  !high\n" +
				"[ ] LIST-2  Beta  #one #two  !low\n",
		},
		{
			name:    "unknown sort keeps file order",
			content: listVault,
			args:    []string{"--sort=bogus"},
			want: "[ ] LIST-1  Alpha  #one  !high\n" +
				"[ ] LIST-2  Beta  #one #two  !low\n" +
				"[ ] LIST-3  Gamma  #two\n",
		},
		{
			name:    "all blocked",
			content: listBlockedVault,
			args:    []string{"--all", "--blocked"},
			want: "[x] BLK-1  Done\n" +
				"[ ] BLK-2  Blocked by open  @blocked_by:BLK-3\n" +
				"[ ] BLK-3  Open  @blocked_by:BLK-404\n" +
				"[ ] BLK-4  Free\n",
		},
		{
			name:    "single priority OR",
			content: listVault,
			args:    []string{"--priority", "low"},
			want:    "[ ] LIST-2  Beta  #one #two  !low\n",
		},
		{
			name:    "json exact bytes",
			content: "- [ ] JSON-1 Simple @owner:alice #docs !high\n",
			args:    []string{"--json"},
			want: "[\n" +
				"  {\n" +
				"    \"id\": \"JSON-1\",\n" +
				"    \"title\": \"Simple\",\n" +
				"    \"status\": \"open\",\n" +
				"    \"priority\": \"high\",\n" +
				"    \"tags\": [\n" +
				"      \"docs\"\n" +
				"    ],\n" +
				"    \"properties\": {\n" +
				"      \"owner\": \"alice\"\n" +
				"    },\n" +
				"    \"file\": \"tasks.md\",\n" +
				"    \"line\": 1\n" +
				"  }\n" +
				"]\n",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			env, stdout, stderr, dir := commandEnv(t, Terminal{IsTTY: false})
			file := filepath.Join(dir, "tasks.md")
			mustWrite(t, dir, "tasks.md", tt.content)

			args := append([]string{"--path", file}, tt.args...)
			res := runList(env, args)
			if res.Code != 0 {
				t.Fatalf("runList(%v).Code = %d, want 0 (stderr=%q)", args, res.Code, stderr.String())
			}
			if got := stdout.String(); got != tt.want {
				t.Errorf("stdout =\n%q\nwant\n%q", got, tt.want)
			}
		})
	}

	t.Run("json key order and suppression of notes", func(t *testing.T) {
		env, stdout, stderr, dir := commandEnv(t, Terminal{IsTTY: false})
		file := filepath.Join(dir, "tasks.md")
		mustWrite(t, dir, "tasks.md", listBlockedVault)

		res := runList(env, []string{"--path", file, "--all", "--json"})
		if res.Code != 0 {
			t.Fatalf("runList(--json).Code = %d, want 0 (stderr=%q)", res.Code, stderr.String())
		}
		got := stdout.String()
		assertKeyOrder(t, got, `"id"`, `"title"`, `"status"`, `"priority"`, `"tags"`, `"properties"`, `"file"`, `"line"`)
		for _, want := range []string{`"id": "BLK-1"`, `"status": "done"`, `"priority": "medium"`, `"tags": []`, `"properties": {}`} {
			if !strings.Contains(got, want) {
				t.Errorf("JSON missing %q:\n%s", want, got)
			}
		}
		if strings.Contains(got, "Warning:") || strings.Contains(got, "have no ID") {
			t.Errorf("--json emitted a note: %s", got)
		}
		if strings.ContainsRune(got, '\x1b') {
			t.Errorf("--json emitted ANSI: %q", got)
		}
	})

	t.Run("hidden blocked note is aligned and counted", func(t *testing.T) {
		env, stdout, stderr, dir := commandEnv(t, Terminal{IsTTY: false})
		file := filepath.Join(dir, "tasks.md")
		mustWrite(t, dir, "tasks.md", listBlockedVault)

		res := runList(env, []string{"--path", file})
		if res.Code != 0 {
			t.Fatalf("runList().Code = %d, want 0 (stderr=%q)", res.Code, stderr.String())
		}
		raw1 := "- [ ] BLK-2 Blocked by open @blocked_by:BLK-3"
		raw2 := "- [ ] BLK-3 Open @blocked_by:BLK-404"
		widest := len(raw1)
		want := "[ ] BLK-4  Free\n" +
			"Warning: 2 blocked task(s) hidden (use `mdfu task list --blocked` to show them):\n" +
			raw1 + strings.Repeat(" ", widest-len(raw1)+2) + "tasks.md:2\n" +
			raw2 + strings.Repeat(" ", widest-len(raw2)+2) + "tasks.md:3\n"
		if got := stdout.String(); got != want {
			t.Errorf("stdout =\n%q\nwant\n%q", got, want)
		}
	})
}

func TestListNoANSIWhenPiped(t *testing.T) {
	fixtures, err := filepath.Glob(filepath.Join("..", "..", "testdata", "tasks", "*.md"))
	if err != nil {
		t.Fatalf("glob fixtures: %v", err)
	}
	if len(fixtures) == 0 {
		t.Fatal("no task fixtures found")
	}
	for _, fixture := range fixtures {
		abs, err := filepath.Abs(fixture)
		if err != nil {
			t.Fatal(err)
		}
		t.Run(filepath.Base(fixture), func(t *testing.T) {
			env, stdout, stderr, _ := commandEnv(t, Terminal{IsTTY: false})
			res := runList(env, []string{"--path", abs, "--all", "--blocked"})
			if res.Code != 0 {
				t.Fatalf("runList(%s).Code = %d, want 0 (stderr=%q)", abs, res.Code, stderr.String())
			}
			if strings.ContainsRune(stdout.String(), '\x1b') {
				t.Errorf("piped output contains an ESC byte: %q", stdout.String())
			}
		})
	}
}
