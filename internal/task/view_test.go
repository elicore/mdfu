package task

import (
	"path/filepath"
	"strings"
	"testing"
)

// runViewFile writes content to a fresh tasks.md and runs runView scoped to it.
func runViewFile(t *testing.T, term Terminal, content string, args ...string) (Result, string, string) {
	t.Helper()
	env, stdout, stderr, dir := commandEnv(t, term)
	mustWrite(t, dir, "tasks.md", content)
	file := filepath.Join(dir, "tasks.md")
	full := append([]string{"--path", file}, args...)
	res := runView(env, full)
	return res, stdout.String(), stderr.String()
}

const viewVault = "- [ ] VIEW-1 First task\n" +
	"  body line one\n" +
	"  body line two\n" +
	"- [x] VIEW-2 Done dependency\n" +
	"- [ ] VIEW-3 Needs dep @blocked_by:VIEW-2\n" +
	"- [ ] VIEW-4 No body\n"

func TestView(t *testing.T) {
	t.Run("human body", func(t *testing.T) {
		res, stdout, stderr := runViewFile(t, Terminal{IsTTY: false}, viewVault, "VIEW-1")
		if res.Code != 0 {
			t.Fatalf("Code = %d, want 0 (stderr=%q)", res.Code, stderr)
		}
		want := "- [ ] VIEW-1 First task\n      body line one\n      body line two\n"
		if stdout != want {
			t.Errorf("stdout = %q, want %q", stdout, want)
		}
	})

	t.Run("human no body", func(t *testing.T) {
		res, stdout, _ := runViewFile(t, Terminal{IsTTY: false}, viewVault, "VIEW-4")
		if res.Code != 0 {
			t.Fatalf("Code = %d, want 0", res.Code)
		}
		if want := "- [ ] VIEW-4 No body\n"; stdout != want {
			t.Errorf("stdout = %q, want %q", stdout, want)
		}
	})

	t.Run("resolved blocker stays in the verbatim header", func(t *testing.T) {
		res, stdout, _ := runViewFile(t, Terminal{IsTTY: false}, viewVault, "VIEW-3")
		if res.Code != 0 {
			t.Fatalf("Code = %d, want 0", res.Code)
		}
		if want := "- [ ] VIEW-3 Needs dep @blocked_by:VIEW-2\n"; stdout != want {
			t.Errorf("stdout = %q, want %q", stdout, want)
		}
	})

	t.Run("bare number resolves", func(t *testing.T) {
		res, stdout, _ := runViewFile(t, Terminal{IsTTY: false}, viewVault, "1")
		if res.Code != 0 {
			t.Fatalf("Code = %d, want 0", res.Code)
		}
		if want := "- [ ] VIEW-1 First task\n      body line one\n      body line two\n"; stdout != want {
			t.Errorf("stdout = %q, want %q", stdout, want)
		}
	})

	t.Run("json key order and body last", func(t *testing.T) {
		res, stdout, stderr := runViewFile(t, Terminal{IsTTY: false}, viewVault, "VIEW-1", "--json")
		if res.Code != 0 {
			t.Fatalf("Code = %d, want 0 (stderr=%q)", res.Code, stderr)
		}
		assertKeyOrder(t, stdout, `"id"`, `"title"`, `"status"`, `"priority"`, `"tags"`, `"properties"`, `"file"`, `"line"`, `"body"`)
		for _, want := range []string{
			`"id": "VIEW-1"`,
			`"priority": "medium"`,
			`"tags": []`,
			`"properties": {}`,
			`"body": "body line one\nbody line two"`,
		} {
			if !strings.Contains(stdout, want) {
				t.Errorf("JSON missing %q:\n%s", want, stdout)
			}
		}
		if strings.Contains(stdout, "Warning:") {
			t.Errorf("view --json emitted a note: %s", stdout)
		}
	})

	t.Run("json keeps a resolved blocker", func(t *testing.T) {
		res, stdout, _ := runViewFile(t, Terminal{IsTTY: false}, viewVault, "VIEW-3", "--json")
		if res.Code != 0 {
			t.Fatalf("Code = %d, want 0", res.Code)
		}
		if !strings.Contains(stdout, `"blocked_by": "VIEW-2"`) {
			t.Errorf("view --json hid a resolved blocker:\n%s", stdout)
		}
	})

	t.Run("invalid ID propagates E9", func(t *testing.T) {
		res, stdout, stderr := runViewFile(t, Terminal{IsTTY: false}, viewVault, "BOGUS")
		if res.Code != 1 {
			t.Fatalf("Code = %d, want 1", res.Code)
		}
		if stdout != "" {
			t.Errorf("stdout = %q, want empty", stdout)
		}
		if want := "mdfu task: invalid task ID 'BOGUS'\n"; stderr != want {
			t.Errorf("stderr = %q, want %q", stderr, want)
		}
	})

	t.Run("missing task propagates E10", func(t *testing.T) {
		res, stdout, stderr := runViewFile(t, Terminal{IsTTY: false}, viewVault, "NOPE-001")
		if res.Code != 1 {
			t.Fatalf("Code = %d, want 1", res.Code)
		}
		if stdout != "" {
			t.Errorf("stdout = %q, want empty", stdout)
		}
		if want := "mdfu task: task 'NOPE-001' not found\n"; stderr != want {
			t.Errorf("stderr = %q, want %q", stderr, want)
		}
	})

	t.Run("duplicate task propagates E12", func(t *testing.T) {
		vault := "- [ ] DUP-1 One\n- [ ] DUP-1 Two\n"
		res, stdout, stderr := runViewFile(t, Terminal{IsTTY: false}, vault, "DUP-1")
		if res.Code != 1 {
			t.Fatalf("Code = %d, want 1", res.Code)
		}
		if stdout != "" {
			t.Errorf("stdout = %q, want empty", stdout)
		}
		if want := "mdfu task: task 'DUP-1' appears multiple times; expected exactly one match\n"; stderr != want {
			t.Errorf("stderr = %q, want %q", stderr, want)
		}
	})

	t.Run("show alias routes identically", func(t *testing.T) {
		env, stdout, stderr, dir := commandEnv(t, Terminal{IsTTY: false})
		mustWrite(t, dir, "tasks.md", viewVault)
		file := filepath.Join(dir, "tasks.md")
		res := Dispatch(env, []string{"show", "VIEW-4", "--path", file})
		if res.Code != 0 {
			t.Fatalf("Dispatch(show).Code = %d, want 0 (stderr=%q)", res.Code, stderr.String())
		}
		if want := "- [ ] VIEW-4 No body\n"; stdout.String() != want {
			t.Errorf("stdout = %q, want %q", stdout.String(), want)
		}
	})
}

func TestViewHuman(t *testing.T) {
	const vault = "- [ ] HUMAN-1 Title here\n" +
		"  first body line\n" +
		"  second body line\n" +
		"\n" +
		"  after blank\n" +
		"- [ ] HUMAN-2 No body\n"

	const wantBody = "- [ ] HUMAN-1 Title here\n" +
		"      first body line\n" +
		"      second body line\n" +
		"\n" +
		"      after blank\n"

	t.Run("piped output byte-exact", func(t *testing.T) {
		res, stdout, stderr := runViewFile(t, Terminal{IsTTY: false}, vault, "HUMAN-1")
		if res.Code != 0 {
			t.Fatalf("Code = %d, want 0 (stderr=%q)", res.Code, stderr)
		}
		if stdout != wantBody {
			t.Errorf("stdout =\n%q\nwant\n%q", stdout, wantBody)
		}
	})

	t.Run("no-body task has no indented lines", func(t *testing.T) {
		res, stdout, _ := runViewFile(t, Terminal{IsTTY: false}, vault, "HUMAN-2")
		if res.Code != 0 {
			t.Fatalf("Code = %d, want 0", res.Code)
		}
		if want := "- [ ] HUMAN-2 No body\n"; stdout != want {
			t.Errorf("stdout = %q, want %q", stdout, want)
		}
	})

	t.Run("TTY prepends the gray location line", func(t *testing.T) {
		res, stdout, stderr := runViewFile(t, Terminal{IsTTY: true}, vault, "HUMAN-1")
		if res.Code != 0 {
			t.Fatalf("Code = %d, want 0 (stderr=%q)", res.Code, stderr)
		}
		want := "\x1b[38;5;245mtasks.md:1\x1b[0m\n" + wantBody
		if stdout != want {
			t.Errorf("stdout =\n%q\nwant\n%q", stdout, want)
		}
	})

	t.Run("TTY no-color prints a plain location line", func(t *testing.T) {
		res, stdout, _ := runViewFile(t, Terminal{IsTTY: true, NoColor: true}, vault, "HUMAN-2")
		if res.Code != 0 {
			t.Fatalf("Code = %d, want 0", res.Code)
		}
		want := "tasks.md:6\n- [ ] HUMAN-2 No body\n"
		if stdout != want {
			t.Errorf("stdout = %q, want %q", stdout, want)
		}
	})
}

func TestViewAmbiguous(t *testing.T) {
	const vault = "- [ ] ALPHA-1 A\n- [ ] BETA-1 B\n"
	res, stdout, stderr := runViewFile(t, Terminal{IsTTY: false}, vault, "1")
	if res.Code != 1 {
		t.Fatalf("Code = %d, want 1", res.Code)
	}
	if stdout != "" {
		t.Errorf("stdout = %q, want empty", stdout)
	}
	if want := "mdfu task: ambiguous numeric ID '1' matches: ALPHA-1, BETA-1\n"; stderr != want {
		t.Errorf("stderr = %q, want %q", stderr, want)
	}
}
