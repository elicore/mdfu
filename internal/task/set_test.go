package task

import (
	"bytes"
	"path/filepath"
	"testing"
)

func TestSet(t *testing.T) {
	setVault := func(t *testing.T, content string) (Env, *bytes.Buffer, *bytes.Buffer, string) {
		t.Helper()
		env, stdout, stderr := testEnv(t)
		t.Setenv("MDTASK_PATH", "")
		root := t.TempDir()
		t.Chdir(root)
		env.Cwd = root
		mustWrite(t, root, "tasks.md", content)
		return env, stdout, stderr, filepath.Join(root, "tasks.md")
	}

	t.Run("existing tag is a no-op", func(t *testing.T) {
		env, stdout, stderr, path := setVault(t, "- [ ] A-1 Task\t\t#tag\n")
		before := readTestFile(t, path)

		res := Dispatch(env, []string{"set", "A-1", "#tag"})
		if res.Code != 0 {
			t.Fatalf("code = %d, want 0", res.Code)
		}
		if got := readTestFile(t, path); got != before {
			t.Errorf("file = %q, want unchanged %q", got, before)
		}
		if stdout.String() != "" || stderr.String() != "" {
			t.Errorf("output = (%q, %q), want empty", stdout.String(), stderr.String())
		}
	})

	t.Run("new priority replaces the old one in place", func(t *testing.T) {
		env, _, _, path := setVault(t, "- [ ] A-1 Task\t\t!low #tag\n")

		res := Dispatch(env, []string{"set", "A-1", "!high"})
		if res.Code != 0 {
			t.Fatalf("code = %d, want 0", res.Code)
		}
		if got, want := readTestFile(t, path), "- [ ] A-1 Task\t\t!high #tag\n"; got != want {
			t.Errorf("file = %q, want %q", got, want)
		}
	})

	t.Run("tokens on a metadata-free line land after tabs", func(t *testing.T) {
		env, _, _, path := setVault(t, "- [ ] A-1 Task\n")

		res := Dispatch(env, []string{"set", "A-1", "#new"})
		if res.Code != 0 {
			t.Fatalf("code = %d, want 0", res.Code)
		}
		if got, want := readTestFile(t, path), "- [ ] A-1 Task\t\t#new\n"; got != want {
			t.Errorf("file = %q, want %q", got, want)
		}
	})

	t.Run("title containing a hash-number survives verbatim", func(t *testing.T) {
		env, _, _, path := setVault(t, "- [ ] A-1 Fix #123 now\n")

		res := Dispatch(env, []string{"set", "A-1", "#tag"})
		if res.Code != 0 {
			t.Fatalf("code = %d, want 0", res.Code)
		}
		if got, want := readTestFile(t, path), "- [ ] A-1 Fix #123 now\t\t#tag\n"; got != want {
			t.Errorf("file = %q, want %q", got, want)
		}
	})

	t.Run("comma separated ids in one file", func(t *testing.T) {
		env, _, _, path := setVault(t, "- [ ] A-1 One\n- [ ] A-2 Two\n")

		res := Dispatch(env, []string{"set", "A-1,A-2", "#tag"})
		if res.Code != 0 {
			t.Fatalf("code = %d, want 0", res.Code)
		}
		if got, want := readTestFile(t, path), "- [ ] A-1 One\t\t#tag\n- [ ] A-2 Two\t\t#tag\n"; got != want {
			t.Errorf("file = %q, want %q", got, want)
		}
	})

	t.Run("no task IDs provided", func(t *testing.T) {
		env, stdout, stderr := testEnv(t)
		t.Setenv("MDTASK_PATH", "")

		res := Dispatch(env, []string{"set", "#tag"})
		if res.Code != 1 {
			t.Fatalf("code = %d, want 1", res.Code)
		}
		if want := "mdfu task: no task IDs provided\n"; stderr.String() != want {
			t.Errorf("stderr = %q, want %q", stderr.String(), want)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
	})

	t.Run("no metadata tokens provided", func(t *testing.T) {
		env, stdout, stderr := testEnv(t)
		t.Setenv("MDTASK_PATH", "")

		res := Dispatch(env, []string{"set", "A-1"})
		if res.Code != 1 {
			t.Fatalf("code = %d, want 1", res.Code)
		}
		if want := "mdfu task: no metadata tokens provided\n"; stderr.String() != want {
			t.Errorf("stderr = %q, want %q", stderr.String(), want)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
	})

	t.Run("no args reports the missing IDs", func(t *testing.T) {
		env, stdout, stderr := testEnv(t)
		t.Setenv("MDTASK_PATH", "")

		res := Dispatch(env, []string{"set"})
		if res.Code != 1 {
			t.Fatalf("code = %d, want 1", res.Code)
		}
		if want := "mdfu task: no task IDs provided\n"; stderr.String() != want {
			t.Errorf("stderr = %q, want %q", stderr.String(), want)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
	})
}
