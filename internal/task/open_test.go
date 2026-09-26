package task

import (
	"os"
	"path/filepath"
	"testing"
)

func TestOpen(t *testing.T) {
	t.Run("passes line and file and propagates exit code", func(t *testing.T) {
		env, stdout, stderr := testEnv(t)
		t.Setenv("MDTASK_PATH", "")
		root := t.TempDir()
		t.Chdir(root)
		env.Cwd = root

		mustWrite(t, root, "tasks.md", "# Notes\n\n- [ ] A-1 First\n")
		argvFile := filepath.Join(root, "argv.txt")
		script := filepath.Join(root, "editor.sh")
		body := "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '" + argvFile + "'\nexit 3\n"
		if err := os.WriteFile(script, []byte(body), 0o755); err != nil {
			t.Fatal(err)
		}
		t.Setenv("EDITOR", script)

		res := Dispatch(env, []string{"open", "A-1"})
		if res.Code != 3 {
			t.Fatalf("Dispatch(open).Code = %d, want 3", res.Code)
		}
		got, err := os.ReadFile(argvFile)
		if err != nil {
			t.Fatalf("reading argv file: %v", err)
		}
		if want := "+3\ntasks.md\n"; string(got) != want {
			t.Errorf("child argv = %q, want %q", got, want)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
		if stderr.Len() != 0 {
			t.Errorf("stderr = %q, want empty", stderr.String())
		}
	})

	t.Run("editor arguments precede line and file", func(t *testing.T) {
		env, _, _ := testEnv(t)
		t.Setenv("MDTASK_PATH", "")
		root := t.TempDir()
		t.Chdir(root)
		env.Cwd = root

		mustWrite(t, root, "tasks.md", "- [ ] A-1 First\n")
		argvFile := filepath.Join(root, "argv.txt")
		script := filepath.Join(root, "editor.sh")
		body := "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '" + argvFile + "'\n"
		if err := os.WriteFile(script, []byte(body), 0o755); err != nil {
			t.Fatal(err)
		}
		t.Setenv("EDITOR", script+" --wait")

		res := Dispatch(env, []string{"open", "A-1"})
		if res.Code != 0 {
			t.Fatalf("Dispatch(open).Code = %d, want 0", res.Code)
		}
		got, err := os.ReadFile(argvFile)
		if err != nil {
			t.Fatalf("reading argv file: %v", err)
		}
		if want := "--wait\n+1\ntasks.md\n"; string(got) != want {
			t.Errorf("child argv = %q, want %q", got, want)
		}
	})

	t.Run("unset editor", func(t *testing.T) {
		env, stdout, stderr := testEnv(t)
		t.Setenv("MDTASK_PATH", "")
		root := t.TempDir()
		t.Chdir(root)
		env.Cwd = root

		mustWrite(t, root, "tasks.md", "- [ ] A-1 First\n")
		t.Setenv("EDITOR", "")

		res := Dispatch(env, []string{"open", "A-1"})
		if res.Code != 1 {
			t.Fatalf("Dispatch(open).Code = %d, want 1", res.Code)
		}
		if want := "mdfu task: $EDITOR is not set\n"; stderr.String() != want {
			t.Errorf("stderr = %q, want %q", stderr.String(), want)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
	})
}
