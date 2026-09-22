package main

import (
	"bytes"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

// isolateThemeEnv points XDG discovery at an empty temp dir and clears
// $MDFU_CONFIG so the host environment cannot leak into loadTheme tests.
func isolateThemeEnv(t *testing.T) {
	t.Helper()
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("MDFU_CONFIG", "")
}

func TestLoadThemeDefaults(t *testing.T) {
	isolateThemeEnv(t)

	th, err := loadTheme("", io.Discard)
	if err != nil {
		t.Fatalf("loadTheme(\"\"): %v", err)
	}
	if th.BodyLines != 30 {
		t.Errorf("BodyLines = %d, want 30", th.BodyLines)
	}
	if !th.ShowFrontmatter {
		t.Errorf("ShowFrontmatter = false, want true")
	}
}

func TestLoadThemeExplicitMissing(t *testing.T) {
	isolateThemeEnv(t)

	_, err := loadTheme("/nonexistent/config.yaml", io.Discard)
	if err == nil {
		t.Fatal("loadTheme of a missing explicit path should return an error")
	}
}

func TestRunPositionalQuery(t *testing.T) {
	isolateThemeEnv(t)
	root := buildTestVault(t)

	var stdout, stderr bytes.Buffer
	code := run([]string{"--root", root, "onboarding"}, &stdout, &stderr)
	if code != 0 {
		t.Fatalf("run() = %d, want 0; stderr=%q", code, stderr.String())
	}
	if !strings.Contains(stdout.String(), "metric.md") {
		t.Errorf("positional query output = %q, want metric.md", stdout.String())
	}
}

func TestRunPositionalQueryJoinsWords(t *testing.T) {
	isolateThemeEnv(t)
	root := buildTestVault(t)

	var stdout, stderr bytes.Buffer
	code := run([]string{"--root", root, "kumquat", "zebra"}, &stdout, &stderr)
	if code != 0 {
		t.Fatalf("run() = %d, want 0; stderr=%q", code, stderr.String())
	}
	if !strings.Contains(stdout.String(), "bad.md") {
		t.Errorf("joined query output = %q, want bad.md", stdout.String())
	}
}

func TestRunConfigDefaultPrintsConfig(t *testing.T) {
	isolateThemeEnv(t)

	var stdout, stderr bytes.Buffer
	code := run([]string{"--config", "default"}, &stdout, &stderr)
	if code != 0 {
		t.Fatalf("run() = %d, want 0; stderr=%q", code, stderr.String())
	}
	out := stdout.String()
	if !strings.Contains(out, "pill_shape: round") || !strings.Contains(out, "body_lines: 30") {
		t.Errorf("--config default output missing defaults:\n%s", out)
	}
	if strings.Contains(out, "\npreview_header_color:") {
		t.Errorf("--config default should leave preview_header_color commented out:\n%s", out)
	}
}

func TestFilterLimitDefaults(t *testing.T) {
	tests := []struct {
		name     string
		explicit int
		set      bool
		want     int
	}{
		{"unset falls back to filter default", 50, false, 20},
		{"explicit override wins", 5, true, 5},
		{"explicit zero means unlimited", 0, true, 0},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := filterLimit(tt.explicit, tt.set); got != tt.want {
				t.Errorf("filterLimit(%d, %t) = %d, want %d", tt.explicit, tt.set, got, tt.want)
			}
		})
	}
}

func TestRunFilterCapsResultsAt20ByDefault(t *testing.T) {
	isolateThemeEnv(t)
	root := t.TempDir()
	for i := 0; i < 25; i++ {
		path := filepath.Join(root, fmt.Sprintf("note-%02d.md", i))
		if err := os.WriteFile(path, []byte("# Note\n\nbanana smoothie recipe\n"), 0o644); err != nil {
			t.Fatalf("write %s: %v", path, err)
		}
	}

	var stdout, stderr bytes.Buffer
	if code := run([]string{"--root", root, "banana"}, &stdout, &stderr); code != 0 {
		t.Fatalf("run() = %d, want 0; stderr=%q", code, stderr.String())
	}
	if lines := countLines(stdout.String()); lines != 20 {
		t.Errorf("default filter results = %d, want 20", lines)
	}

	stdout.Reset()
	if code := run([]string{"--root", root, "--limit", "0", "banana"}, &stdout, &stderr); code != 0 {
		t.Fatalf("run(--limit 0) = %d, want 0; stderr=%q", code, stderr.String())
	}
	if lines := countLines(stdout.String()); lines != 25 {
		t.Errorf("--limit 0 results = %d, want 25", lines)
	}

	stdout.Reset()
	if code := run([]string{"--root", root, "banana", "--limit", "3"}, &stdout, &stderr); code != 0 {
		t.Fatalf("run(trailing --limit) = %d, want 0; stderr=%q", code, stderr.String())
	}
	if lines := countLines(stdout.String()); lines != 3 {
		t.Errorf("trailing --limit 3 results = %d, want 3", lines)
	}
}

func TestSplitFlags(t *testing.T) {
	fs := flag.NewFlagSet("mdfu", flag.ContinueOnError)
	fs.Int("limit", 50, "")
	fs.Bool("hidden", false, "")
	fs.String("root", ".", "")

	tests := []struct {
		name string
		args []string
		want []string
	}{
		{"flags first", []string{"--limit", "3", "query"}, []string{"--limit", "3", "--", "query"}},
		{"trailing flags move ahead of query", []string{"query", "words", "--limit", "3", "--hidden"}, []string{"--limit", "3", "--hidden", "--", "query", "words"}},
		{"equals form keeps its value", []string{"q", "--limit=4"}, []string{"--limit=4", "--", "q"}},
		{"double dash keeps dashes in query", []string{"--hidden", "--", "--limit", "query"}, []string{"--hidden", "--", "--limit", "query"}},
		{"no positional adds no separator", []string{"--limit", "2"}, []string{"--limit", "2"}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := splitFlags(fs, tt.args); !reflect.DeepEqual(got, tt.want) {
				t.Errorf("splitFlags(%v) = %v, want %v", tt.args, got, tt.want)
			}
		})
	}
}

func countLines(s string) int {
	s = strings.TrimSpace(s)
	if s == "" {
		return 0
	}
	return strings.Count(s, "\n") + 1
}
