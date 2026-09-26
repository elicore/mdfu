package task

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestInstallSkills(t *testing.T) {
	t.Run("writes all three skills and overwrites an existing file", func(t *testing.T) {
		env, stdout, stderr, _ := commandEnv(t, Terminal{})
		target := t.TempDir()

		sentinel := filepath.Join(target, "UNRELATED.md")
		if err := os.WriteFile(sentinel, []byte("keep me\n"), 0o644); err != nil {
			t.Fatal(err)
		}
		stale := filepath.Join(target, "mdfu-task", "SKILL.md")
		if err := os.MkdirAll(filepath.Dir(stale), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(stale, []byte("STALE\n"), 0o644); err != nil {
			t.Fatal(err)
		}

		res := Dispatch(env, []string{"install-skills", target})
		if res.Code != 0 {
			t.Fatalf("install-skills code = %d, want 0 (stderr=%q)", res.Code, stderr.String())
		}

		lines := strings.Split(strings.TrimRight(stdout.String(), "\n"), "\n")
		if len(lines) != 3 {
			t.Fatalf("stdout = %q, want exactly three lines", stdout.String())
		}
		for i, name := range skillNames {
			if want := fmt.Sprintf("Installed %s into %s", name, target); lines[i] != want {
				t.Errorf("stdout line %d = %q, want %q", i, lines[i], want)
			}
			data, err := os.ReadFile(filepath.Join(target, name, "SKILL.md"))
			if err != nil {
				t.Fatalf("read installed %s: %v", name, err)
			}
			if len(data) == 0 {
				t.Errorf("installed %s is empty", name)
			}
		}

		overwritten, err := os.ReadFile(stale)
		if err != nil {
			t.Fatal(err)
		}
		if string(overwritten) == "STALE\n" || len(overwritten) == 0 {
			t.Errorf("existing SKILL.md was not overwritten: %q", overwritten)
		}

		kept, err := os.ReadFile(sentinel)
		if err != nil {
			t.Fatalf("sentinel removed: %v", err)
		}
		if string(kept) != "keep me\n" {
			t.Errorf("sentinel modified: %q", kept)
		}
	})

	t.Run("read-only target fails with cannot write to", func(t *testing.T) {
		if os.Geteuid() == 0 {
			t.Skip("running as root; permission bits are not enforced")
		}
		env, stdout, stderr, _ := commandEnv(t, Terminal{})
		target := t.TempDir()
		if err := os.Chmod(target, 0o555); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { os.Chmod(target, 0o755) })

		res := Dispatch(env, []string{"install-skills", target})
		if res.Code != 1 {
			t.Fatalf("install-skills code = %d, want 1", res.Code)
		}
		if !strings.Contains(stderr.String(), "cannot write to") {
			t.Errorf("stderr = %q, want cannot-write error", stderr.String())
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
	})

	t.Run("a missing directory argument is a usage error", func(t *testing.T) {
		env, stdout, _, _ := commandEnv(t, Terminal{})

		res := Dispatch(env, []string{"install-skills"})
		if res.Code != 2 {
			t.Fatalf("install-skills code = %d, want 2", res.Code)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
	})

	t.Run("assets name no foreign skill", func(t *testing.T) {
		for _, name := range skillNames {
			data, err := skillsFS.ReadFile("skills/" + name + "/SKILL.md")
			if err != nil {
				t.Fatalf("read embedded %s: %v", name, err)
			}
			if len(data) == 0 {
				t.Errorf("embedded %s is empty", name)
			}
			for _, foreign := range []string{"mdtask", "mdtask-add", "mdtask-do"} {
				if strings.Contains(string(data), foreign) {
					t.Errorf("asset %s contains the forbidden name %q", name, foreign)
				}
			}
		}
	})
}
