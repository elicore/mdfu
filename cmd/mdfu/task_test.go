package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/elicore/mdfu/internal/theme"
)

const routerVault = "- [ ] EXMPL-001 Fix the thing\t\t#launch !high @status:doing\n" +
	"- [x] EXMPL-002 Ship it\t\t#launch\n"

func writeVaultFile(t *testing.T, dir, name, content string) {
	t.Helper()
	if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o644); err != nil {
		t.Fatalf("write %s: %v", name, err)
	}
}

func TestRouter(t *testing.T) {
	origTerminal, origStdin := isTerminal, stdin
	isTerminal = func() bool { return false }
	stdin = strings.NewReader("")
	defer func() {
		isTerminal = origTerminal
		stdin = origStdin
	}()

	isolateThemeEnv(t)
	t.Setenv("NO_COLOR", "")
	t.Setenv("EDITOR", "")

	tests := []struct {
		name     string
		files    map[string]string
		chdir    bool
		args     []string
		wantCode int
		check    func(t *testing.T, dir, out, errOut string)
	}{
		{
			name:  "task alone lists",
			files: map[string]string{"tasks.md": routerVault},
			chdir: true,
			args:  []string{"task"},
			check: func(t *testing.T, _, out, _ string) {
				if !strings.Contains(out, "EXMPL-001") {
					t.Errorf("task alone output = %q, want a task row", out)
				}
			},
		},
		{
			name:  "task list",
			files: map[string]string{"tasks.md": routerVault},
			args:  []string{"task", "list", "--path", "{dir}"},
			check: func(t *testing.T, _, out, _ string) {
				if !strings.Contains(out, "EXMPL-001") || strings.Contains(out, "EXMPL-002") {
					t.Errorf("task list output = %q, want only open EXMPL-001", out)
				}
			},
		},
		{
			name:  "task list --all",
			files: map[string]string{"tasks.md": routerVault},
			args:  []string{"task", "list", "--all", "--path", "{dir}"},
			check: func(t *testing.T, _, out, _ string) {
				if !strings.Contains(out, "EXMPL-001") || !strings.Contains(out, "EXMPL-002") {
					t.Errorf("task list --all output = %q, want both tasks", out)
				}
			},
		},
		{
			name:  "task list --json",
			files: map[string]string{"tasks.md": routerVault},
			args:  []string{"task", "list", "--json", "--path", "{dir}"},
			check: func(t *testing.T, _, out, _ string) {
				if !strings.HasPrefix(strings.TrimSpace(out), "[") || !strings.Contains(out, `"EXMPL-001"`) {
					t.Errorf("task list --json output = %q, want a JSON array", out)
				}
			},
		},
		{
			name:  "task view",
			files: map[string]string{"tasks.md": routerVault},
			args:  []string{"task", "view", "EXMPL-001", "--path", "{dir}"},
			check: func(t *testing.T, _, out, _ string) {
				if !strings.Contains(out, "EXMPL-001") {
					t.Errorf("task view output = %q, want the task header", out)
				}
			},
		},
		{
			name:  "task show alias",
			files: map[string]string{"tasks.md": routerVault},
			args:  []string{"task", "show", "EXMPL-001", "--path", "{dir}"},
			check: func(t *testing.T, _, out, _ string) {
				if !strings.Contains(out, "EXMPL-001") {
					t.Errorf("task show output = %q, want the task header", out)
				}
			},
		},
		{
			name:     "task open",
			files:    map[string]string{"tasks.md": routerVault},
			args:     []string{"task", "open", "EXMPL-001", "--path", "{dir}"},
			wantCode: 1,
			check: func(t *testing.T, _, _, errOut string) {
				if !strings.Contains(errOut, "$EDITOR is not set") {
					t.Errorf("task open stderr = %q, want the E8 message", errOut)
				}
			},
		},
		{
			name:  "task move",
			files: map[string]string{"tasks.md": routerVault},
			args:  []string{"task", "move", "EXMPL-001", "{dir}/moved.md", "--path", "{dir}"},
			check: func(t *testing.T, dir, _, _ string) {
				data, err := os.ReadFile(filepath.Join(dir, "moved.md"))
				if err != nil {
					t.Fatalf("read moved.md: %v", err)
				}
				if !strings.Contains(string(data), "EXMPL-001") {
					t.Errorf("moved.md = %q, want the moved task", data)
				}
			},
		},
		{
			name:  "task set",
			files: map[string]string{"tasks.md": routerVault},
			args:  []string{"task", "set", "EXMPL-001", "#t", "--path", "{dir}"},
			check: func(t *testing.T, dir, _, _ string) {
				data, err := os.ReadFile(filepath.Join(dir, "tasks.md"))
				if err != nil {
					t.Fatalf("read tasks.md: %v", err)
				}
				if !strings.Contains(string(data), "#t") {
					t.Errorf("tasks.md = %q, want the new tag", data)
				}
			},
		},
		{
			name:  "task ids --prefix",
			files: map[string]string{"ids.md": "- [ ] Plain task without an id\n"},
			args:  []string{"task", "ids", "--prefix", "PRJ", "--path", "{dir}"},
			check: func(t *testing.T, dir, _, _ string) {
				data, err := os.ReadFile(filepath.Join(dir, "ids.md"))
				if err != nil {
					t.Fatalf("read ids.md: %v", err)
				}
				if !strings.Contains(string(data), "PRJ-001") {
					t.Errorf("ids.md = %q, want an assigned PRJ-001", data)
				}
			},
		},
		{
			name:  "task archive",
			files: map[string]string{"tasks.md": routerVault},
			args:  []string{"task", "archive", "--path", "{dir}"},
			check: func(t *testing.T, dir, _, _ string) {
				data, err := os.ReadFile(filepath.Join(dir, "_archive.md"))
				if err != nil {
					t.Fatalf("read _archive.md: %v", err)
				}
				if !strings.Contains(string(data), "EXMPL-002") {
					t.Errorf("_archive.md = %q, want the done task", data)
				}
			},
		},
		{
			name:  "task validate",
			files: map[string]string{"tasks.md": routerVault},
			args:  []string{"task", "validate", "--path", "{dir}"},
			check: func(t *testing.T, _, out, errOut string) {
				if out != "" || errOut != "" {
					t.Errorf("task validate wrote out=%q err=%q, want silence", out, errOut)
				}
			},
		},
		{
			name:  "task install-skills",
			chdir: true,
			args:  []string{"task", "install-skills", "skillsout"},
			check: func(t *testing.T, dir, _, _ string) {
				for _, name := range []string{"mdfu-task", "mdfu-task-add", "mdfu-task-do"} {
					path := filepath.Join(dir, "skillsout", name, "SKILL.md")
					if _, err := os.Stat(path); err != nil {
						t.Errorf("install-skills missing %s: %v", path, err)
					}
				}
			},
		},
		{
			name:  "task ID routes to view",
			files: map[string]string{"tasks.md": routerVault},
			args:  []string{"task", "EXMPL-001", "--path", "{dir}"},
			check: func(t *testing.T, _, out, _ string) {
				if !strings.Contains(out, "EXMPL-001") {
					t.Errorf("task EXMPL-001 output = %q, want the task header", out)
				}
			},
		},
		{
			name:  "bare numeric ID routes to view",
			files: map[string]string{"tasks.md": routerVault},
			args:  []string{"task", "1", "--path", "{dir}"},
			check: func(t *testing.T, _, out, _ string) {
				if !strings.Contains(out, "EXMPL-001") {
					t.Errorf("task 1 output = %q, want EXMPL-001", out)
				}
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			for name, content := range tt.files {
				writeVaultFile(t, dir, name, content)
			}
			if tt.chdir {
				t.Chdir(dir)
			}
			args := make([]string, len(tt.args))
			for i, a := range tt.args {
				args[i] = strings.ReplaceAll(a, "{dir}", dir)
			}

			var out, errOut bytes.Buffer
			code := run(args, &out, &errOut)
			if code != tt.wantCode {
				t.Fatalf("run(%v) = %d, want %d; stderr=%q", args, code, tt.wantCode, errOut.String())
			}
			tt.check(t, dir, out.String(), errOut.String())
		})
	}

	t.Run("task retention falls through to filter mode", func(t *testing.T) {
		root := buildTestVault(t)
		writeVaultFile(t, root, "retention.md", "# Retention\n\nCohort retention and task tracking notes.\n")
		t.Chdir(root)

		var out, errOut bytes.Buffer
		code := run([]string{"task", "retention"}, &out, &errOut)
		if code != 0 {
			t.Fatalf("run(task retention) = %d, want the filter-mode 0; stderr=%q", code, errOut.String())
		}
		if !strings.Contains(out.String(), "retention.md") {
			t.Errorf("output = %q, want the retention.md note path", out.String())
		}
		if strings.Contains(out.String(), "EXMPL-") {
			t.Errorf("output = %q, want a note path, not a task ID", out.String())
		}
		if strings.Contains(errOut.String(), "unknown command") {
			t.Errorf("stderr = %q, want the search fall-through, not the unknown-command error", errOut.String())
		}
	})

	t.Run("task task searches for task", func(t *testing.T) {
		root := buildTestVault(t)
		writeVaultFile(t, root, "retention.md", "# Retention\n\nCohort retention and task tracking notes.\n")
		t.Chdir(root)

		var out, errOut bytes.Buffer
		code := run([]string{"task", "task"}, &out, &errOut)
		if code != 0 {
			t.Fatalf("run(task task) = %d, want the filter-mode 0; stderr=%q", code, errOut.String())
		}
		if !strings.Contains(out.String(), ".md") {
			t.Errorf("output = %q, want a note path", out.String())
		}
	})

	t.Run("double dash bypasses the router", func(t *testing.T) {
		root := buildTestVault(t)
		writeVaultFile(t, root, "retention.md", "# Retention\n\nCohort retention and task tracking notes.\n")
		t.Chdir(root)

		var out, errOut bytes.Buffer
		code := run([]string{"--", "task"}, &out, &errOut)
		if code != 0 {
			t.Fatalf("run(-- task) = %d, want the filter-mode 0; stderr=%q", code, errOut.String())
		}
		if !strings.Contains(out.String(), ".md") {
			t.Errorf("output = %q, want a note path", out.String())
		}
	})

	t.Run("explicit dispatch marker reaches unknown command", func(t *testing.T) {
		var out, errOut bytes.Buffer
		code := run([]string{"task", "--", "bogus"}, &out, &errOut)
		if code != 1 {
			t.Fatalf("run(task -- bogus) = %d, want 1; stderr=%q", code, errOut.String())
		}
		if out.String() != "" {
			t.Errorf("stdout = %q, want empty", out.String())
		}
		const wantPrefix = "mdfu task: unknown command 'bogus'\n"
		if !strings.HasPrefix(errOut.String(), wantPrefix) {
			t.Errorf("stderr = %q, want prefix %q", errOut.String(), wantPrefix)
		}
		if len(errOut.String()) <= len(wantPrefix) {
			t.Errorf("stderr = %q, want non-empty help after the unknown-command line", errOut.String())
		}
	})

	t.Run("explicit dispatch marker with no args lists", func(t *testing.T) {
		var out, errOut bytes.Buffer
		code := run([]string{"task", "--"}, &out, &errOut)
		if code != 0 {
			t.Fatalf("run(task --) = %d, want 0 (list); stderr=%q", code, errOut.String())
		}
	})

	t.Run("explicit dispatch marker forwards a known subcommand", func(t *testing.T) {
		dir := t.TempDir()
		writeVaultFile(t, dir, "tasks.md", routerVault)

		var out, errOut bytes.Buffer
		code := run([]string{"task", "--", "list", "--path", dir}, &out, &errOut)
		if code != 0 {
			t.Fatalf("run(task -- list --path %s) = %d, want 0; stderr=%q", dir, code, errOut.String())
		}
		if !strings.Contains(out.String(), "EXMPL-001") {
			t.Errorf("output = %q, want the listed task", out.String())
		}
	})

	t.Run("config default before task prints the theme YAML", func(t *testing.T) {
		var out, errOut bytes.Buffer
		code := run([]string{"--config", "default", "task", "list"}, &out, &errOut)
		if code != 0 {
			t.Fatalf("run(--config default task list) = %d, want 0; stderr=%q", code, errOut.String())
		}
		if out.String() != theme.DefaultYAML() {
			t.Errorf("--config default output diverged from theme.DefaultYAML()")
		}
	})

	t.Run("root flag before task subcommand is E2", func(t *testing.T) {
		dir := t.TempDir()
		var out, errOut bytes.Buffer
		code := run([]string{"--root", dir, "task", "list"}, &out, &errOut)
		if code != 2 {
			t.Fatalf("run(--root <v> task list) = %d, want 2; stderr=%q", code, errOut.String())
		}
		const want = "mdfu task: --root must come after the task subcommand\n"
		if errOut.String() != want {
			t.Errorf("stderr = %q, want %q", errOut.String(), want)
		}
		if out.String() != "" {
			t.Errorf("stdout = %q, want empty", out.String())
		}
	})

	t.Run("pre-existing behaviour is byte-identical", func(t *testing.T) {
		root := buildTestVault(t)

		t.Run("bare filter mode", func(t *testing.T) {
			var runOut, runErr bytes.Buffer
			code := run([]string{"--root", root, "onboarding"}, &runOut, &runErr)
			var directOut, directErr bytes.Buffer
			directCode := runFilter(&directOut, &directErr, root, false, true, "onboarding", false, defaultFilterLimit, "paths")
			if code != directCode || runOut.String() != directOut.String() || runErr.String() != directErr.String() {
				t.Errorf("bare filter diverged: run=(%d,%q,%q) direct=(%d,%q,%q)",
					code, runOut.String(), runErr.String(), directCode, directOut.String(), directErr.String())
			}
		})

		t.Run("version", func(t *testing.T) {
			var out, errOut bytes.Buffer
			code := run([]string{"--version"}, &out, &errOut)
			if code != 0 || out.String() != "mdfu "+version+"\n" {
				t.Errorf("--version = (%d, %q), want (0, %q)", code, out.String(), "mdfu "+version+"\n")
			}
		})

		t.Run("config default", func(t *testing.T) {
			var out, errOut bytes.Buffer
			code := run([]string{"--config", "default"}, &out, &errOut)
			if code != 0 || out.String() != theme.DefaultYAML() {
				t.Errorf("--config default = (%d, %q), want (0, DefaultYAML)", code, out.String())
			}
		})

		t.Run("format json", func(t *testing.T) {
			var runOut, runErr bytes.Buffer
			code := run([]string{"--root", root, "--format", "json", "onboarding"}, &runOut, &runErr)
			var directOut, directErr bytes.Buffer
			directCode := runFilter(&directOut, &directErr, root, false, true, "onboarding", false, defaultFilterLimit, "json")
			if code != directCode || runOut.String() != directOut.String() || runErr.String() != directErr.String() {
				t.Errorf("--format json diverged: run=(%d,%q,%q) direct=(%d,%q,%q)",
					code, runOut.String(), runErr.String(), directCode, directOut.String(), directErr.String())
			}
		})

		t.Run("format bogus", func(t *testing.T) {
			var out, errOut bytes.Buffer
			code := run([]string{"--root", root, "--format", "bogus", "onboarding"}, &out, &errOut)
			const want = "invalid --format \"bogus\": must be paths|json|vimgrep\n"
			if code != 2 || errOut.String() != want {
				t.Errorf("--format bogus = (%d, %q), want (2, %q)", code, errOut.String(), want)
			}
		})
	})
}

func TestTasksNonTTYParity(t *testing.T) {
	origTerminal, origStdin := isTerminal, stdin
	isTerminal = func() bool { return false }
	stdin = strings.NewReader("")
	defer func() {
		isTerminal = origTerminal
		stdin = origStdin
	}()

	isolateThemeEnv(t)
	t.Setenv("NO_COLOR", "")

	dir := t.TempDir()
	writeVaultFile(t, dir, "tasks.md", routerVault)

	for _, extra := range [][]string{nil, {"--all"}} {
		t.Run(strings.Join(append([]string{"tasks"}, extra...), " "), func(t *testing.T) {
			tasksArgs := append([]string{"tasks", "--path", dir}, extra...)
			listArgs := append([]string{"task", "list", "--path", dir}, extra...)

			var tasksOut, tasksErr bytes.Buffer
			tasksCode := run(tasksArgs, &tasksOut, &tasksErr)
			var listOut, listErr bytes.Buffer
			listCode := run(listArgs, &listOut, &listErr)

			if tasksCode != listCode {
				t.Fatalf("tasks code = %d, task list code = %d", tasksCode, listCode)
			}
			if tasksOut.String() != listOut.String() {
				t.Errorf("tasks stdout diverged:\ntasks=%q\nlist =%q", tasksOut.String(), listOut.String())
			}
			if tasksErr.String() != listErr.String() {
				t.Errorf("tasks stderr diverged:\ntasks=%q\nlist =%q", tasksErr.String(), listErr.String())
			}
			if strings.Contains(tasksOut.String(), "\x1b[?1049h") || strings.Contains(tasksErr.String(), "\x1b[?1049h") {
				t.Errorf("tasks emitted an alt-screen escape without a TTY")
			}
		})
	}

	t.Run("tasks --all is accepted", func(t *testing.T) {
		var out, errOut bytes.Buffer
		code := run([]string{"tasks", "--all", "--path", dir}, &out, &errOut)
		if code != 0 {
			t.Fatalf("tasks --all = %d, want 0; stderr=%q", code, errOut.String())
		}
		if !strings.Contains(out.String(), "EXMPL-002") {
			t.Errorf("tasks --all output = %q, want the done task", out.String())
		}
	})

	t.Run("tasks bogus is E3", func(t *testing.T) {
		var out, errOut bytes.Buffer
		code := run([]string{"tasks", "bogus", "--path", dir}, &out, &errOut)
		if code != 2 {
			t.Fatalf("tasks bogus = %d, want 2; stderr=%q", code, errOut.String())
		}
		const want = "mdfu tasks: unexpected argument 'bogus'\n"
		if errOut.String() != want {
			t.Errorf("stderr = %q, want %q", errOut.String(), want)
		}
		if out.String() != "" {
			t.Errorf("stdout = %q, want empty", out.String())
		}
	})
}
