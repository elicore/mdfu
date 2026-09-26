package task

import (
	"bytes"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"
)

// mustWrite writes content to dir/name, creating parent directories.
func mustWrite(t *testing.T, dir, name, content string) {
	t.Helper()
	path := filepath.Join(dir, name)
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("mkdir %s: %v", filepath.Dir(path), err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("write %s: %v", path, err)
	}
}

// sortedPaths returns the paths joined under base and sorted like Walk.
func sortedPaths(base string, rels ...string) []string {
	out := make([]string, 0, len(rels))
	for _, rel := range rels {
		out = append(out, filepath.Join(base, filepath.FromSlash(rel)))
	}
	sort.Strings(out)
	return out
}

func TestWalk(t *testing.T) {
	t.Run("includes hidden and excludes git and node_modules", func(t *testing.T) {
		base := t.TempDir()
		mustWrite(t, base, "a.md", "x\n")
		mustWrite(t, base, "A.MD", "x\n")
		mustWrite(t, base, "README.txt", "x\n")
		mustWrite(t, base, ".hidden/dir/h.md", "x\n")
		mustWrite(t, base, ".git/g.md", "x\n")
		mustWrite(t, base, "node_modules/n.md", "x\n")
		mustWrite(t, base, "sub/c.md", "x\n")
		mustWrite(t, base, "sub/deep/d.md", "x\n")
		mustWrite(t, base, "keep/k.md", "x\n")

		got, err := Walk(base, DiscoverOptions{FollowSymlinks: true})
		if err != nil {
			t.Fatalf("Walk error = %v", err)
		}
		want := sortedPaths(base,
			".hidden/dir/h.md", "A.MD", "a.md", "keep/k.md", "sub/c.md", "sub/deep/d.md")
		if !reflect.DeepEqual(got, want) {
			t.Errorf("Walk = %v\nwant %v", got, want)
		}
	})

	t.Run("excludes configured directories", func(t *testing.T) {
		base := t.TempDir()
		mustWrite(t, base, "a.md", "x\n")
		mustWrite(t, base, "skipme/s.md", "x\n")
		mustWrite(t, base, "keep/k.md", "x\n")

		got, err := Walk(base, DiscoverOptions{FollowSymlinks: true, ExcludeDirs: []string{"skipme", "keep"}})
		if err != nil {
			t.Fatalf("Walk error = %v", err)
		}
		want := sortedPaths(base, "a.md")
		if !reflect.DeepEqual(got, want) {
			t.Errorf("Walk = %v, want %v", got, want)
		}
	})

	t.Run("does not follow symlinks when disabled", func(t *testing.T) {
		base := t.TempDir()
		real := filepath.Join(base, "real")
		if err := os.MkdirAll(real, 0o755); err != nil {
			t.Fatal(err)
		}
		mustWrite(t, base, "real/r.md", "x\n")
		if err := os.Symlink(real, filepath.Join(base, "link")); err != nil {
			t.Fatal(err)
		}

		got, err := Walk(base, DiscoverOptions{FollowSymlinks: false})
		if err != nil {
			t.Fatalf("Walk error = %v", err)
		}
		want := sortedPaths(base, "real/r.md")
		if !reflect.DeepEqual(got, want) {
			t.Errorf("Walk = %v, want %v", got, want)
		}
	})

	t.Run("follows symlinks without duplicating realpaths", func(t *testing.T) {
		base := t.TempDir()
		mustWrite(t, base, "real/r.md", "x\n")
		if err := os.Symlink(filepath.Join(base, "real"), filepath.Join(base, "alias")); err != nil {
			t.Fatal(err)
		}

		got, err := Walk(base, DiscoverOptions{FollowSymlinks: true})
		if err != nil {
			t.Fatalf("Walk error = %v", err)
		}
		// The alias spelling is encountered first (ReadDir is lexical), so the
		// duplicate realpath is deduplicated keeping alias/r.md.
		want := []string{filepath.Join(base, "alias", "r.md")}
		if !reflect.DeepEqual(got, want) {
			t.Errorf("Walk = %v, want %v", got, want)
		}
	})

	t.Run("symlink cycle completes under a five second guard", func(t *testing.T) {
		base := t.TempDir()
		cycle := filepath.Join(base, "cycle")
		if err := os.MkdirAll(cycle, 0o755); err != nil {
			t.Fatal(err)
		}
		mustWrite(t, base, "cycle/x.md", "x\n")
		if err := os.Symlink(cycle, filepath.Join(cycle, "link")); err != nil {
			t.Fatal(err)
		}

		done := make(chan []string, 1)
		go func() {
			got, _ := Walk(base, DiscoverOptions{FollowSymlinks: true})
			done <- got
		}()
		select {
		case got := <-done:
			want := sortedPaths(base, "cycle/x.md")
			if !reflect.DeepEqual(got, want) {
				t.Errorf("Walk = %v, want %v", got, want)
			}
		case <-time.After(5 * time.Second):
			t.Fatal("Walk did not complete within 5s (symlink cycle?)")
		}
	})

	t.Run("unreadable directory warns once and continues", func(t *testing.T) {
		if os.Geteuid() == 0 {
			t.Skip("running as root; permission bits are not enforced")
		}
		base := t.TempDir()
		mustWrite(t, base, "a.md", "x\n")
		locked := filepath.Join(base, "locked")
		if err := os.MkdirAll(locked, 0o755); err != nil {
			t.Fatal(err)
		}
		mustWrite(t, base, "locked/secret.md", "x\n")
		if err := os.Chmod(locked, 0); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { os.Chmod(locked, 0o755) })

		var warn bytes.Buffer
		got, err := Walk(base, DiscoverOptions{FollowSymlinks: true, Warn: &warn})
		if err != nil {
			t.Fatalf("Walk error = %v", err)
		}
		if want := sortedPaths(base, "a.md"); !reflect.DeepEqual(got, want) {
			t.Errorf("Walk = %v, want %v", got, want)
		}
		lines := strings.Split(strings.TrimRight(warn.String(), "\n"), "\n")
		if len(lines) != 1 {
			t.Fatalf("warning lines = %q, want exactly one", warn.String())
		}
		if !strings.HasPrefix(lines[0], "mdfu task: skipping "+locked+": ") {
			t.Errorf("warning = %q, want prefix %q", lines[0], "mdfu task: skipping "+locked+": ")
		}
	})
}

func TestFilter(t *testing.T) {
	paths := []string{"a.md", "x/a.md", "archive/z.md"}

	tests := []struct {
		name string
		inc  []string
		exc  []string
		want []string
	}{
		{"empty include keeps all", nil, nil, []string{"a.md", "x/a.md", "archive/z.md"}},
		{"doublestar include keeps all", []string{"**/*.md"}, nil, []string{"a.md", "x/a.md", "archive/z.md"}},
		{"bare include keeps top level only", []string{"*.md"}, nil, []string{"a.md"}},
		{"exclude drops archive", []string{"**/*.md"}, []string{"archive/**"}, []string{"a.md", "x/a.md"}},
		{"no matches", []string{"nope/*.md"}, nil, []string{}},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := Filter(paths, ".", tt.inc, tt.exc)
			if len(got) == 0 && len(tt.want) == 0 {
				return
			}
			if !reflect.DeepEqual(got, tt.want) {
				t.Errorf("Filter = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestLoadAll(t *testing.T) {
	t.Run("parses identified tasks and excludes the archive", func(t *testing.T) {
		root := t.TempDir()
		t.Chdir(root)
		mustWrite(t, root, "a.md", "- [ ] A-1 First\n  body line\n- [ ] Second\n- [ ] SEED- Seed\n")
		mustWrite(t, root, "_archive.md", "- [x] ARCH-9 Archived\n")
		mustWrite(t, root, "nested/n.md", "- [ ] N-1 Nested\n")

		tasks, unidentified, err := LoadAll(".", Config{ArchivePath: "_archive.md"})
		if err != nil {
			t.Fatalf("LoadAll error = %v", err)
		}
		if len(tasks) != 2 {
			t.Fatalf("tasks = %+v, want 2 identified tasks", tasks)
		}
		first := tasks[0]
		if first.ID != "A-1" || first.File != "a.md" || first.Line != 1 {
			t.Errorf("first = %+v, want A-1 a.md:1", first)
		}
		if first.Body != "body line" || first.BodyIndent != "  " {
			t.Errorf("first body = (%q, %q), want (%q, %q)", first.Body, first.BodyIndent, "body line", "  ")
		}
		if tasks[1].ID != "N-1" || tasks[1].File != "nested/n.md" || tasks[1].Line != 1 {
			t.Errorf("second = %+v, want N-1 nested/n.md:1", tasks[1])
		}
		for _, task := range tasks {
			if task.ID == "ARCH-9" {
				t.Errorf("archive task leaked into scope: %+v", task)
			}
		}

		if len(unidentified) != 2 {
			t.Fatalf("unidentified = %+v, want 2 entries", unidentified)
		}
		if unidentified[0].Raw != "- [ ] Second" || unidentified[0].Line != 3 || unidentified[0].File != "a.md" {
			t.Errorf("unidentified[0] = %+v", unidentified[0])
		}
		if unidentified[1].Raw != "- [ ] SEED- Seed" {
			t.Errorf("unidentified[1] = %+v", unidentified[1])
		}
	})

	t.Run("honours MDTASK_EXCLUDE_DIRS", func(t *testing.T) {
		root := t.TempDir()
		t.Chdir(root)
		t.Setenv("MDTASK_EXCLUDE_DIRS", "skipme:also")
		mustWrite(t, root, "a.md", "- [ ] A-1 Keep\n")
		mustWrite(t, root, "skipme/s.md", "- [ ] S-1 Skip\n")
		mustWrite(t, root, "also/t.md", "- [ ] T-1 Skip\n")

		tasks, _, err := LoadAll(".", Config{ArchivePath: "_archive.md"})
		if err != nil {
			t.Fatalf("LoadAll error = %v", err)
		}
		if len(tasks) != 1 || tasks[0].ID != "A-1" {
			t.Errorf("tasks = %+v, want only A-1", tasks)
		}
	})
}
