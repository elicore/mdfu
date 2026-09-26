package task

import (
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"strings"
	"testing"
)

func writeTaskFile(t *testing.T, dir, name, content string) string {
	t.Helper()
	path := filepath.Join(dir, name)
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("write %s: %v", path, err)
	}
	return path
}

func TestFindConfigPath(t *testing.T) {
	t.Run("prefers mdtaskrc at the same level", func(t *testing.T) {
		root := t.TempDir()
		sub := filepath.Join(root, "a", "b")
		if err := os.MkdirAll(sub, 0o755); err != nil {
			t.Fatal(err)
		}
		writeTaskFile(t, root, ".mdtaskrc", "{}")
		writeTaskFile(t, root, ".mdfurc", "{}")
		got, ok := FindConfigPath(sub)
		if !ok || got != filepath.Join(root, ".mdtaskrc") {
			t.Errorf("FindConfigPath = (%q, %v), want %q", got, ok, filepath.Join(root, ".mdtaskrc"))
		}
	})

	t.Run("falls back to mdfurc", func(t *testing.T) {
		root := t.TempDir()
		writeTaskFile(t, root, ".mdfurc", "{}")
		got, ok := FindConfigPath(root)
		if !ok || got != filepath.Join(root, ".mdfurc") {
			t.Errorf("FindConfigPath = (%q, %v), want %q", got, ok, filepath.Join(root, ".mdfurc"))
		}
	})

	t.Run("nearer mdfurc beats farther mdtaskrc", func(t *testing.T) {
		root := t.TempDir()
		sub := filepath.Join(root, "sub")
		if err := os.MkdirAll(sub, 0o755); err != nil {
			t.Fatal(err)
		}
		writeTaskFile(t, root, ".mdtaskrc", "{}")
		writeTaskFile(t, sub, ".mdfurc", "{}")
		got, ok := FindConfigPath(sub)
		if !ok || got != filepath.Join(sub, ".mdfurc") {
			t.Errorf("FindConfigPath = (%q, %v), want %q", got, ok, filepath.Join(sub, ".mdfurc"))
		}
	})

	t.Run("nearer mdtaskrc wins", func(t *testing.T) {
		root := t.TempDir()
		sub := filepath.Join(root, "sub")
		if err := os.MkdirAll(sub, 0o755); err != nil {
			t.Fatal(err)
		}
		writeTaskFile(t, root, ".mdfurc", "{}")
		writeTaskFile(t, sub, ".mdtaskrc", "{}")
		got, ok := FindConfigPath(sub)
		if !ok || got != filepath.Join(sub, ".mdtaskrc") {
			t.Errorf("FindConfigPath = (%q, %v), want %q", got, ok, filepath.Join(sub, ".mdtaskrc"))
		}
	})

	t.Run("none found", func(t *testing.T) {
		root := t.TempDir()
		if _, ok := FindConfigPath(root); ok {
			t.Errorf("FindConfigPath(%q) found a config, want none", root)
		}
	})
}

func TestLoadConfig(t *testing.T) {
	t.Run("full schema", func(t *testing.T) {
		dir := t.TempDir()
		writeTaskFile(t, dir, ".mdtaskrc", `{
  "path": "notes",
  "files": {"include": ["**/*.md"], "exclude": ["archive/**"]},
  "excludePrefixes": ["ARCH"],
  "archivePath": "_arch.md"
}`)
		cfg, err := Load(dir)
		if err != nil {
			t.Fatalf("Load error = %v", err)
		}
		want := Config{
			Path:            "notes",
			Include:         []string{"**/*.md"},
			Exclude:         []string{"archive/**"},
			ExcludePrefixes: []string{"ARCH"},
			ArchivePath:     "_arch.md",
		}
		if !reflect.DeepEqual(cfg, want) {
			t.Errorf("Load = %+v, want %+v", cfg, want)
		}
	})

	t.Run("defaults when no config", func(t *testing.T) {
		cfg, err := Load(t.TempDir())
		if err != nil {
			t.Fatalf("Load error = %v", err)
		}
		if cfg.ArchivePath != "_archive.md" {
			t.Errorf("ArchivePath = %q, want _archive.md", cfg.ArchivePath)
		}
	})

	t.Run("malformed json", func(t *testing.T) {
		dir := t.TempDir()
		writeTaskFile(t, dir, ".mdtaskrc", "{")
		_, err := Load(dir)
		if err == nil {
			t.Fatal("Load error = nil, want error")
		}
		if ok, _ := regexp.MatchString(`^Invalid JSON in .*: `, err.Error()); !ok {
			t.Errorf("Load error = %q, want ^Invalid JSON in .*: ", err.Error())
		}
	})

	t.Run("defensive typing", func(t *testing.T) {
		dir := t.TempDir()
		writeTaskFile(t, dir, ".mdtaskrc", `{
  "path": 42,
  "files": "nope",
  "excludePrefixes": ["ARCH", 7, true],
  "archivePath": 9,
  "unknownKey": {"x": 1}
}`)
		cfg, err := Load(dir)
		if err != nil {
			t.Fatalf("Load error = %v", err)
		}
		if cfg.Path != "" {
			t.Errorf("Path = %q, want empty", cfg.Path)
		}
		if len(cfg.Include) != 0 || len(cfg.Exclude) != 0 {
			t.Errorf("Include/Exclude = %v/%v, want empty (non-object files ignored)", cfg.Include, cfg.Exclude)
		}
		if !reflect.DeepEqual(cfg.ExcludePrefixes, []string{"ARCH"}) {
			t.Errorf("ExcludePrefixes = %v, want [ARCH]", cfg.ExcludePrefixes)
		}
		if cfg.ArchivePath != "_archive.md" {
			t.Errorf("ArchivePath = %q, want default", cfg.ArchivePath)
		}
	})

	t.Run("mdtaskrc wins over mdfurc", func(t *testing.T) {
		dir := t.TempDir()
		writeTaskFile(t, dir, ".mdtaskrc", `{"path":"from-task"}`)
		writeTaskFile(t, dir, ".mdfurc", `{"path":"from-mdfu"}`)
		cfg, err := Load(dir)
		if err != nil {
			t.Fatalf("Load error = %v", err)
		}
		if cfg.Path != "from-task" {
			t.Errorf("Path = %q, want from-task", cfg.Path)
		}
	})

	t.Run("mdfurc used when no mdtaskrc", func(t *testing.T) {
		dir := t.TempDir()
		writeTaskFile(t, dir, ".mdfurc", `{"path":"from-mdfu"}`)
		cfg, err := Load(dir)
		if err != nil {
			t.Fatalf("Load error = %v", err)
		}
		if cfg.Path != "from-mdfu" {
			t.Errorf("Path = %q, want from-mdfu", cfg.Path)
		}
	})

	t.Run("directory rc path is fatal", func(t *testing.T) {
		dir := t.TempDir()
		if err := os.Mkdir(filepath.Join(dir, ".mdtaskrc"), 0o755); err != nil {
			t.Fatal(err)
		}
		_, err := Load(dir)
		if err == nil {
			t.Fatal("Load error = nil, want error")
		}
		if !strings.HasPrefix(err.Error(), "cannot read ") {
			t.Errorf("Load error = %q, want cannot read prefix", err.Error())
		}
	})
}

func TestResolveBasePath(t *testing.T) {
	tests := []struct {
		name     string
		flagPath string
		envPath  string
		cfgPath  string
		cfgDir   string
		want     string
	}{
		{"flag wins", "/flag", "/env", "cfg", "/cfgdir", "/flag"},
		{"env beats config", "", "/env", "cfg", "/cfgdir", "/env"},
		{"config relative to cfg dir", "", "", "notes", "/cfgdir", "/cfgdir/notes"},
		{"config absolute", "", "", "/abs/notes", "/cfgdir", "/abs/notes"},
		{"fallback to dot", "", "", "", "/cfgdir", "."},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := ResolveBasePath(tt.flagPath, tt.envPath, tt.cfgPath, tt.cfgDir); got != tt.want {
				t.Errorf("ResolveBasePath = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestBaseDir(t *testing.T) {
	root := t.TempDir()
	if err := os.Mkdir(filepath.Join(root, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}
	deep := filepath.Join(root, "a", "b")
	if err := os.MkdirAll(deep, 0o755); err != nil {
		t.Fatal(err)
	}
	if got := BaseDir(deep); got != root {
		t.Errorf("BaseDir = %q, want %q", got, root)
	}

	plain := t.TempDir()
	if got := BaseDir(plain); got != plain {
		t.Errorf("BaseDir without .git = %q, want %q", got, plain)
	}
}
