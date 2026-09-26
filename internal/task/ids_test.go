package task

import (
	"bytes"
	"path/filepath"
	"strings"
	"testing"
)

func TestIDs(t *testing.T) {
	newVault := func(t *testing.T, files map[string]string) (Env, *bytes.Buffer, *bytes.Buffer, string) {
		t.Helper()
		env, stdout, stderr := testEnv(t)
		t.Setenv("MDTASK_PATH", "")
		t.Setenv("MDTASK_EXCLUDE_DIRS", "")
		root := t.TempDir()
		t.Chdir(root)
		env.Cwd = root
		for name, content := range files {
			mustWrite(t, root, name, content)
		}
		return env, stdout, stderr, root
	}

	t.Run("starts at global max plus one padded to three", func(t *testing.T) {
		env, stdout, stderr, root := newVault(t, map[string]string{
			"a.md": "- [ ] A-1 First\n- [ ] Do me\n",
		})

		res := Dispatch(env, []string{"ids"})
		if res.Code != 0 {
			t.Fatalf("code = %d, want 0 (stderr %q)", res.Code, stderr.String())
		}
		if want := "- [ ] A-002 Do me\n"; stdout.String() != want {
			t.Errorf("stdout = %q, want %q", stdout.String(), want)
		}
		if got, want := readTestFile(t, filepath.Join(root, "a.md")), "- [ ] A-1 First\n- [ ] A-002 Do me\n"; got != want {
			t.Errorf("file = %q, want %q", got, want)
		}
		if stderr.Len() != 0 {
			t.Errorf("stderr = %q, want empty", stderr.String())
		}
	})

	t.Run("per-file prefix is the most frequent existing prefix", func(t *testing.T) {
		env, stdout, _, root := newVault(t, map[string]string{
			"m.md": "- [ ] A-1 One\n- [ ] A-2 Two\n- [ ] B-3 Three\n- [ ] Do me\n",
		})

		res := Dispatch(env, []string{"ids"})
		if res.Code != 0 {
			t.Fatalf("code = %d, want 0", res.Code)
		}
		if want := "- [ ] A-004 Do me\n"; stdout.String() != want {
			t.Errorf("stdout = %q, want %q", stdout.String(), want)
		}
		if got, want := readTestFile(t, filepath.Join(root, "m.md")), "- [ ] A-1 One\n- [ ] A-2 Two\n- [ ] B-3 Three\n- [ ] A-004 Do me\n"; got != want {
			t.Errorf("file = %q, want %q", got, want)
		}
	})

	t.Run("seed line supplies the prefix when no IDs exist", func(t *testing.T) {
		env, stdout, _, root := newVault(t, map[string]string{
			"s.md": "- [ ] SEED- A seed\n- [ ] No ID\n",
		})

		res := Dispatch(env, []string{"ids"})
		if res.Code != 0 {
			t.Fatalf("code = %d, want 0", res.Code)
		}
		if want := "- [ ] SEED-001 A seed\n- [ ] SEED-002 No ID\n"; stdout.String() != want {
			t.Errorf("stdout = %q, want %q", stdout.String(), want)
		}
		if got, want := readTestFile(t, filepath.Join(root, "s.md")), "- [ ] SEED-001 A seed\n- [ ] SEED-002 No ID\n"; got != want {
			t.Errorf("file = %q, want %q", got, want)
		}
	})

	t.Run("prefix flag is uppercased before validation", func(t *testing.T) {
		env, stdout, _, root := newVault(t, map[string]string{
			"p.md": "- [ ] Do me\n",
		})

		res := Dispatch(env, []string{"ids", "--prefix", "prj"})
		if res.Code != 0 {
			t.Fatalf("code = %d, want 0", res.Code)
		}
		if want := "- [ ] PRJ-001 Do me\n"; stdout.String() != want {
			t.Errorf("stdout = %q, want %q", stdout.String(), want)
		}
		if got, want := readTestFile(t, filepath.Join(root, "p.md")), "- [ ] PRJ-001 Do me\n"; got != want {
			t.Errorf("file = %q, want %q", got, want)
		}
	})

	t.Run("invalid prefix flag fails", func(t *testing.T) {
		env, stdout, stderr, _ := newVault(t, map[string]string{
			"p.md": "- [ ] Do me\n",
		})

		res := Dispatch(env, []string{"ids", "--prefix", "1bad"})
		if res.Code != 1 {
			t.Fatalf("code = %d, want 1", res.Code)
		}
		if want := "mdfu task: invalid prefix '1BAD'\n"; stderr.String() != want {
			t.Errorf("stderr = %q, want %q", stderr.String(), want)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
	})

	t.Run("two files with an unresolved prefix write nothing", func(t *testing.T) {
		env, stdout, _, root := newVault(t, map[string]string{
			"a.md": "- [ ] A-1 Keep\n- [ ] Do me\n",
			"b.md": "- [ ] No source\n",
		})
		beforeA := readTestFile(t, filepath.Join(root, "a.md"))
		beforeB := readTestFile(t, filepath.Join(root, "b.md"))

		res := Dispatch(env, []string{"ids"})
		if res.Code != 1 {
			t.Fatalf("code = %d, want 1", res.Code)
		}
		if got := readTestFile(t, filepath.Join(root, "a.md")); got != beforeA {
			t.Errorf("a.md = %q, want unchanged %q", got, beforeA)
		}
		if got := readTestFile(t, filepath.Join(root, "b.md")); got != beforeB {
			t.Errorf("b.md = %q, want unchanged %q", got, beforeB)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
	})

	t.Run("duplicate numeric part warns and exits zero", func(t *testing.T) {
		env, stdout, stderr, root := newVault(t, map[string]string{
			"d.md": "- [ ] A-1 First\n- [ ] B-1 Second\n- [ ] Do me\n",
		})

		res := Dispatch(env, []string{"ids"})
		if res.Code != 0 {
			t.Fatalf("code = %d, want 0", res.Code)
		}
		if want := "- [ ] A-002 Do me\n"; stdout.String() != want {
			t.Errorf("stdout = %q, want %q", stdout.String(), want)
		}
		if want := "warning: duplicate numeric part 1 across prefixes: A-1, B-1\n"; stderr.String() != want {
			t.Errorf("stderr = %q, want %q", stderr.String(), want)
		}
		if got, want := readTestFile(t, filepath.Join(root, "d.md")), "- [ ] A-1 First\n- [ ] B-1 Second\n- [ ] A-002 Do me\n"; got != want {
			t.Errorf("file = %q, want %q", got, want)
		}
	})

	t.Run("injected stdin supplies the prompt answer", func(t *testing.T) {
		env, stdout, stderr, root := newVault(t, map[string]string{
			"b.md": "- [ ] No source\n",
		})
		env.Stdin = strings.NewReader("PRJ\n")

		res := Dispatch(env, []string{"ids"})
		if res.Code != 0 {
			t.Fatalf("code = %d, want 0", res.Code)
		}
		if want := "Enter prefix for b.md: "; stderr.String() != want {
			t.Errorf("stderr = %q, want %q", stderr.String(), want)
		}
		if want := "- [ ] PRJ-001 No source\n"; stdout.String() != want {
			t.Errorf("stdout = %q, want %q", stdout.String(), want)
		}
		if got, want := readTestFile(t, filepath.Join(root, "b.md")), "- [ ] PRJ-001 No source\n"; got != want {
			t.Errorf("file = %q, want %q", got, want)
		}
	})
}
