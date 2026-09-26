package task

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func readTestFile(t *testing.T, path string) string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return string(data)
}

func TestMove(t *testing.T) {
	t.Run("relocates block and keeps source separator", func(t *testing.T) {
		env, stdout, stderr := testEnv(t)
		t.Setenv("MDTASK_PATH", "")
		root := t.TempDir()
		t.Chdir(root)
		env.Cwd = root

		mustWrite(t, root, "a.md", "# Notes\n\n- [ ] A-1 First\n\n- [ ] A-2 Second\n  second body\n")
		mustWrite(t, root, "b.md", "# Target\n")

		res := Dispatch(env, []string{"move", "A-2", "b.md"})
		if res.Code != 0 {
			t.Fatalf("Dispatch(move).Code = %d, want 0 (stderr %q)", res.Code, stderr.String())
		}
		if got, want := readTestFile(t, filepath.Join(root, "b.md")), "# Target\n- [ ] A-2 Second\n  second body\n"; got != want {
			t.Errorf("target = %q, want %q", got, want)
		}
		if got, want := readTestFile(t, filepath.Join(root, "a.md")), "# Notes\n\n- [ ] A-1 First\n\n"; got != want {
			t.Errorf("source = %q, want %q", got, want)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
		if stderr.Len() != 0 {
			t.Errorf("stderr = %q, want empty", stderr.String())
		}
	})

	t.Run("does not delete the trailing blank separator", func(t *testing.T) {
		env, _, _ := testEnv(t)
		t.Setenv("MDTASK_PATH", "")
		root := t.TempDir()
		t.Chdir(root)
		env.Cwd = root

		mustWrite(t, root, "c.md", "- [ ] A-1 First\n- [ ] A-2 Second\n\ntail\n")
		mustWrite(t, root, "d.md", "")

		res := Dispatch(env, []string{"move", "A-2", "d.md"})
		if res.Code != 0 {
			t.Fatalf("Dispatch(move).Code = %d, want 0", res.Code)
		}
		if got, want := readTestFile(t, filepath.Join(root, "c.md")), "- [ ] A-1 First\n\ntail\n"; got != want {
			t.Errorf("source = %q, want %q", got, want)
		}
		if got, want := readTestFile(t, filepath.Join(root, "d.md")), "- [ ] A-2 Second\n"; got != want {
			t.Errorf("target = %q, want %q", got, want)
		}
	})

	t.Run("creates parent directories", func(t *testing.T) {
		env, _, _ := testEnv(t)
		t.Setenv("MDTASK_PATH", "")
		root := t.TempDir()
		t.Chdir(root)
		env.Cwd = root

		mustWrite(t, root, "e.md", "- [ ] A-1 First\n")
		res := Dispatch(env, []string{"move", "A-1", "nested/deep/g.md"})
		if res.Code != 0 {
			t.Fatalf("Dispatch(move).Code = %d, want 0", res.Code)
		}
		if got, want := readTestFile(t, filepath.Join(root, "nested", "deep", "g.md")), "- [ ] A-1 First\n"; got != want {
			t.Errorf("target = %q, want %q", got, want)
		}
	})

	t.Run("same realpath is a silent no-op", func(t *testing.T) {
		env, stdout, stderr := testEnv(t)
		t.Setenv("MDTASK_PATH", "")
		root := t.TempDir()
		t.Chdir(root)
		env.Cwd = root

		mustWrite(t, root, "f.md", "- [ ] A-1 First\n")
		before := readTestFile(t, filepath.Join(root, "f.md"))

		res := Dispatch(env, []string{"move", "A-1", "f.md"})
		if res.Code != 0 {
			t.Fatalf("Dispatch(move).Code = %d, want 0", res.Code)
		}
		if got := readTestFile(t, filepath.Join(root, "f.md")); got != before {
			t.Errorf("file changed: %q, want %q", got, before)
		}
		if stdout.Len() != 0 || stderr.Len() != 0 {
			t.Errorf("output = (%q, %q), want empty", stdout.String(), stderr.String())
		}
	})

	t.Run("target that is a directory", func(t *testing.T) {
		env, stdout, stderr := testEnv(t)
		t.Setenv("MDTASK_PATH", "")
		root := t.TempDir()
		t.Chdir(root)
		env.Cwd = root

		mustWrite(t, root, "h.md", "- [ ] A-1 First\n")
		if err := os.Mkdir(filepath.Join(root, "adir"), 0o755); err != nil {
			t.Fatal(err)
		}

		res := Dispatch(env, []string{"move", "A-1", "adir"})
		if res.Code != 1 {
			t.Fatalf("Dispatch(move).Code = %d, want 1", res.Code)
		}
		if want := "mdfu task: 'adir' is a directory\n"; stderr.String() != want {
			t.Errorf("stderr = %q, want %q", stderr.String(), want)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
	})

	t.Run("unwritable source", func(t *testing.T) {
		if os.Geteuid() == 0 {
			t.Skip("running as root; permission bits are not enforced")
		}
		env, _, stderr := testEnv(t)
		t.Setenv("MDTASK_PATH", "")
		root := t.TempDir()
		t.Chdir(root)
		env.Cwd = root

		src := filepath.Join(root, "ro.md")
		mustWrite(t, root, "ro.md", "- [ ] A-1 First\n")
		if err := os.Chmod(src, 0o444); err != nil {
			t.Fatal(err)
		}

		res := Dispatch(env, []string{"move", "A-1", "out.md"})
		if res.Code != 1 {
			t.Fatalf("Dispatch(move).Code = %d, want 1", res.Code)
		}
		if !strings.Contains(stderr.String(), "cannot write to 'ro.md': permission denied") {
			t.Errorf("stderr = %q, want a cannot-write error", stderr.String())
		}
		if _, err := os.Stat(filepath.Join(root, "out.md")); !os.IsNotExist(err) {
			t.Errorf("target was created despite unwritable source")
		}
	})
}

func TestMoveStale(t *testing.T) {
	env, stdout, stderr := testEnv(t)
	t.Setenv("MDTASK_PATH", "")
	root := t.TempDir()
	t.Chdir(root)
	env.Cwd = root

	mustWrite(t, root, "a.md", "- [ ] A-1 Title\n")
	mustWrite(t, root, "b.md", "# Target\n")

	scope, _, err := resolveScope(env, []string{"--path", "."})
	if err != nil {
		t.Fatalf("resolveScope error = %v", err)
	}
	tasks, _, err := scopeTaskSet(scope)
	if err != nil {
		t.Fatalf("scopeTaskSet error = %v", err)
	}
	task, err := Resolve("A-1", tasks)
	if err != nil {
		t.Fatalf("Resolve error = %v", err)
	}

	if err := os.WriteFile(filepath.Join(root, "a.md"), []byte("# changed\n- [ ] A-1 Title\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	res := moveResolvedTask(env, task, "b.md")
	if res.Code != 1 {
		t.Fatalf("moveResolvedTask.Code = %d, want 1", res.Code)
	}
	if want := "mdfu task: file changed, task 'A-1' not at expected line\n"; stderr.String() != want {
		t.Errorf("stderr = %q, want %q", stderr.String(), want)
	}
	if got, want := readTestFile(t, filepath.Join(root, "b.md")), "# Target\n"; got != want {
		t.Errorf("target = %q, want untouched %q", got, want)
	}
	if stdout.Len() != 0 {
		t.Errorf("stdout = %q, want empty", stdout.String())
	}
}
