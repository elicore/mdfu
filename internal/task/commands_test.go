package task

import (
	"bytes"
	"errors"
	"flag"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

func testEnv(t *testing.T) (Env, *bytes.Buffer, *bytes.Buffer) {
	t.Helper()
	var stdout, stderr bytes.Buffer
	env := Env{
		Stdout:   &stdout,
		Stderr:   &stderr,
		Stdin:    strings.NewReader(""),
		Terminal: Terminal{},
		Cwd:      t.TempDir(),
		Now:      func() time.Time { return time.Unix(0, 0) },
	}
	return env, &stdout, &stderr
}

type subcommandSpy struct {
	name string
	args []string
}

func spySubcommands() (*subcommandSpy, func()) {
	spy := &subcommandSpy{}
	type saved struct {
		index int
		run   func(Env, []string) Result
	}
	var prev []saved
	for i := range registry {
		prev = append(prev, saved{index: i, run: registry[i].Run})
		idx := i
		registry[i].Run = func(_ Env, args []string) Result {
			spy.name = registry[idx].Name
			spy.args = append([]string(nil), args...)
			return Result{Code: 0}
		}
	}
	return spy, func() {
		for _, s := range prev {
			registry[s.index].Run = s.run
		}
	}
}

func TestDispatch(t *testing.T) {
	env, stdout, stderr := testEnv(t)
	spy, restore := spySubcommands()
	defer restore()

	tests := []struct {
		name     string
		argv     []string
		wantName string
		wantArgs []string
	}{
		{"empty argv lists", nil, "list", nil},
		{"list", []string{"list"}, "list", nil},
		{"view", []string{"view", "EXMPL-1"}, "view", []string{"EXMPL-1"}},
		{"show alias", []string{"show", "EXMPL-1"}, "view", []string{"EXMPL-1"}},
		{"open", []string{"open", "EXMPL-1"}, "open", []string{"EXMPL-1"}},
		{"move", []string{"move", "EXMPL-1", "f.md"}, "move", []string{"EXMPL-1", "f.md"}},
		{"set", []string{"set", "EXMPL-1", "#t"}, "set", []string{"EXMPL-1", "#t"}},
		{"ids", []string{"ids", "--prefix", "PRJ"}, "ids", []string{"--prefix", "PRJ"}},
		{"archive", []string{"archive"}, "archive", nil},
		{"validate", []string{"validate"}, "validate", nil},
		{"install-skills", []string{"install-skills", "d"}, "install-skills", []string{"d"}},
		{"numeric ID routes to view", []string{"42"}, "view", []string{"42"}},
		{"full ID routes to view", []string{"EXMPL-42"}, "view", []string{"EXMPL-42"}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			res := Dispatch(env, tt.argv)
			if res.Code != 0 {
				t.Fatalf("Dispatch(%v).Code = %d, want 0", tt.argv, res.Code)
			}
			if spy.name != tt.wantName {
				t.Errorf("routed to %q, want %q", spy.name, tt.wantName)
			}
			if !reflect.DeepEqual(spy.args, tt.wantArgs) {
				t.Errorf("args = %v, want %v", spy.args, tt.wantArgs)
			}
		})
	}

	t.Run("unknown command", func(t *testing.T) {
		stdout.Reset()
		stderr.Reset()
		res := Dispatch(env, []string{"bogus"})
		if res.Code != 1 {
			t.Fatalf("Dispatch(bogus).Code = %d, want 1", res.Code)
		}
		const wantPrefix = "mdfu task: unknown command 'bogus'\n"
		got := stderr.String()
		if !strings.HasPrefix(got, wantPrefix) {
			t.Errorf("stderr = %q, want prefix %q", got, wantPrefix)
		}
		if len(got) <= len(wantPrefix) {
			t.Errorf("help text missing after unknown-command line: %q", got)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
	})
}

func TestErrf(t *testing.T) {
	env, stdout, stderr := testEnv(t)
	errf(env, "boom %d", 7)

	if got, want := stderr.String(), "mdfu task: boom 7\n"; got != want {
		t.Errorf("stderr = %q, want %q", got, want)
	}
	if stdout.Len() != 0 {
		t.Errorf("stdout = %q, want empty", stdout.String())
	}
	if strings.Contains(stderr.String(), "mdtask:") {
		t.Errorf("stderr %q must not contain the foreign prefix %q", stderr.String(), "mdtask:")
	}
}

func TestExitCodeTable(t *testing.T) {
	probe := func(args []string) func(*testing.T, Env) Result {
		return func(_ *testing.T, env Env) Result {
			fs := flag.NewFlagSet("probe", flag.ContinueOnError)
			fs.String("tag", "", "")
			_, done := parseFlags(env, fs, args)
			if done != nil {
				return *done
			}
			return Result{Code: 0}
		}
	}

	tests := []struct {
		name string
		run  func(*testing.T, Env) Result
		want int
	}{
		{"success", probe([]string{"--tag", "x"}), 0},
		{"unknown flag", probe([]string{"--bogus"}), 2},
		{"flag missing value", probe([]string{"--tag"}), 2},
		{"--path missing", func(_ *testing.T, env Env) Result {
			_, _, err := resolveScope(env, []string{"--path", filepath.Join(env.Cwd, "nope")})
			if err != nil {
				return fail(env, err)
			}
			return Result{Code: 0}
		}, 1},
		{"unknown subcommand", func(_ *testing.T, env Env) Result {
			return Dispatch(env, []string{"bogus"})
		}, 1},
		{"stale error", func(_ *testing.T, env Env) Result {
			return fail(env, &StaleError{File: "f.md", ID: "EXMPL-1"})
		}, 1},
		{"ordinary error", func(_ *testing.T, env Env) Result {
			return fail(env, errors.New("boom"))
		}, 1},
		{"config error", func(t *testing.T, env Env) Result {
			if err := os.WriteFile(filepath.Join(env.Cwd, ".mdtaskrc"), []byte("{"), 0o644); err != nil {
				t.Fatal(err)
			}
			_, _, err := resolveScope(env, nil)
			if err != nil {
				return fail(env, err)
			}
			return Result{Code: 0}
		}, 1},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			env, stdout, _ := testEnv(t)
			t.Setenv("MDTASK_PATH", "")
			if got := tt.run(t, env); got.Code != tt.want {
				t.Errorf("code = %d, want %d", got.Code, tt.want)
			}
			if tt.want != 0 && stdout.Len() != 0 {
				t.Errorf("failure path wrote to stdout: %q", stdout.String())
			}
		})
	}
}

func TestSubcommands(t *testing.T) {
	want := []string{"list", "view", "open", "move", "set", "ids", "archive", "validate", "install-skills"}
	got := Subcommands()
	if len(got) != len(want) {
		t.Fatalf("Subcommands() returned %d entries, want %d", len(got), len(want))
	}
	for i, sc := range got {
		if sc.Name != want[i] {
			t.Errorf("Subcommands()[%d].Name = %q, want %q", i, sc.Name, want[i])
		}
	}
	if len(got[1].Aliases) != 1 || got[1].Aliases[0] != "show" {
		t.Errorf("view aliases = %v, want [show]", got[1].Aliases)
	}
	got[0].Name = "mutated"
	if Subcommands()[0].Name != "list" {
		t.Errorf("Subcommands() must return a copy of the registry")
	}
}

func TestResolveScope(t *testing.T) {
	env, _, _ := testEnv(t)
	t.Setenv("MDTASK_PATH", "")
	dir := t.TempDir()
	env.Cwd = dir
	file := filepath.Join(dir, "tasks.md")
	if err := os.WriteFile(file, []byte("- [ ] A-1 T\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	scope, rest, err := resolveScope(env, []string{"--path", file, "extra"})
	if err != nil {
		t.Fatalf("resolveScope(--path file) error = %v", err)
	}
	if !scope.IsFile || scope.File != file {
		t.Errorf("scope IsFile=%v File=%q, want true %q", scope.IsFile, scope.File, file)
	}
	if !reflect.DeepEqual(rest, []string{"extra"}) {
		t.Errorf("rest = %v, want [extra]", rest)
	}

	scope, rest, err = resolveScope(env, []string{"--path=" + dir})
	if err != nil {
		t.Fatalf("resolveScope(--path=dir) error = %v", err)
	}
	if scope.IsFile || scope.Base != dir {
		t.Errorf("scope IsFile=%v Base=%q, want false %q", scope.IsFile, scope.Base, dir)
	}
	if len(rest) != 0 {
		t.Errorf("rest = %v, want empty", rest)
	}
}
