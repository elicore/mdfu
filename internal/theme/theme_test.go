package theme

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/charmbracelet/lipgloss"
)

func writeConfig(t *testing.T, dir, content string) string {
	t.Helper()
	path := filepath.Join(dir, "config.yaml")
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("write config: %v", err)
	}
	return path
}

func TestDefault(t *testing.T) {
	th := Default()

	styles := []struct {
		name      string
		style     lipgloss.Style
		wantFG    lipgloss.TerminalColor
		wantBold  bool
		wantFaint bool
		wantUnder bool
	}{
		{"Cursor", th.Cursor, lipgloss.Color("212"), true, false, false},
		{"Selected", th.Selected, lipgloss.Color("82"), true, false, false},
		{"Dim", th.Dim, lipgloss.NoColor{}, false, true, false},
		{"Title", th.Title, lipgloss.Color("212"), true, false, false},
		{"Filename", th.Filename, lipgloss.Color("244"), false, false, false},
		{"PreviewHeader", th.PreviewHeader, lipgloss.NoColor{}, true, false, true},
		{"FrontmatterKey", th.FrontmatterKey, lipgloss.Color("245"), false, false, false},
		{"Pill", th.Pill, lipgloss.Color("231"), false, false, false},
	}
	for _, sc := range styles {
		if got := sc.style.GetForeground(); got != sc.wantFG {
			t.Errorf("%s foreground = %v, want %v", sc.name, got, sc.wantFG)
		}
		if got := sc.style.GetBold(); got != sc.wantBold {
			t.Errorf("%s bold = %v, want %v", sc.name, got, sc.wantBold)
		}
		if got := sc.style.GetFaint(); got != sc.wantFaint {
			t.Errorf("%s faint = %v, want %v", sc.name, got, sc.wantFaint)
		}
		if got := sc.style.GetUnderline(); got != sc.wantUnder {
			t.Errorf("%s underline = %v, want %v", sc.name, got, sc.wantUnder)
		}
	}
	if got := th.Pill.GetBackground(); got != lipgloss.Color("62") {
		t.Errorf("Pill background = %v, want 62", got)
	}
	if got := th.Pill.GetPaddingRight(); got != 1 {
		t.Errorf("Pill padding right = %d, want 1", got)
	}

	scalars := []struct {
		name string
		got  any
		want any
	}{
		{"PillShape", th.PillShape, "round"},
		{"MarkdownStyle", th.MarkdownStyle, ""},
		{"ShowFrontmatter", th.ShowFrontmatter, true},
		{"BodyLines", th.BodyLines, 30},
		{"HighlightSGR", th.HighlightSGR, "1;30;103"},
		{"LinkSGR", th.LinkSGR, "1;4;38;5;212"},
	}
	for _, sc := range scalars {
		if sc.got != sc.want {
			t.Errorf("%s = %v, want %v", sc.name, sc.got, sc.want)
		}
	}
}

func TestLoadFile(t *testing.T) {
	tests := []struct {
		name  string
		yaml  string
		check func(t *testing.T, th Theme)
	}{
		{
			name: "partial override merges, unset keys keep defaults",
			yaml: "cursor_color: \"160\"\nbody_lines: 12\n",
			check: func(t *testing.T, th Theme) {
				if got := th.Cursor.GetForeground(); got != lipgloss.Color("160") {
					t.Errorf("Cursor foreground = %v, want 160", got)
				}
				if !th.Cursor.GetBold() {
					t.Errorf("Cursor bold = false, want true")
				}
				if th.BodyLines != 12 {
					t.Errorf("BodyLines = %d, want 12", th.BodyLines)
				}
				if got := th.Selected.GetForeground(); got != lipgloss.Color("82") {
					t.Errorf("Selected foreground = %v, want default 82", got)
				}
				if got := th.Title.GetForeground(); got != lipgloss.Color("212") {
					t.Errorf("Title foreground = %v, want default 212", got)
				}
				if !th.ShowFrontmatter || th.HighlightSGR != "1;30;103" || th.LinkSGR != "1;4;38;5;212" {
					t.Errorf("unset scalars changed: %+v", th)
				}
			},
		},
		{
			name: "unknown keys are ignored",
			yaml: "nonsense: true\nnested: {a: 1}\n",
			check: func(t *testing.T, th Theme) {
				if th.BodyLines != 30 || th.PillShape != "round" {
					t.Errorf("defaults changed by unknown keys: %+v", th)
				}
			},
		},
		{
			name: "invalid markdown_style keeps default",
			yaml: "markdown_style: solarized\n",
			check: func(t *testing.T, th Theme) {
				if th.MarkdownStyle != "" {
					t.Errorf("MarkdownStyle = %q, want default \"\"", th.MarkdownStyle)
				}
			},
		},
		{
			name: "valid markdown_style applies",
			yaml: "markdown_style: light\n",
			check: func(t *testing.T, th Theme) {
				if th.MarkdownStyle != "light" {
					t.Errorf("MarkdownStyle = %q, want light", th.MarkdownStyle)
				}
			},
		},
		{
			name: "invalid pill_shape keeps default",
			yaml: "pill_shape: square\n",
			check: func(t *testing.T, th Theme) {
				if th.PillShape != "round" {
					t.Errorf("PillShape = %q, want default round", th.PillShape)
				}
			},
		},
		{
			name: "pill_shape none applies",
			yaml: "pill_shape: none\n",
			check: func(t *testing.T, th Theme) {
				if th.PillShape != "none" {
					t.Errorf("PillShape = %q, want none", th.PillShape)
				}
			},
		},
		{
			name: "body_lines clamped to lower bound",
			yaml: "body_lines: 0\n",
			check: func(t *testing.T, th Theme) {
				if th.BodyLines != 1 {
					t.Errorf("BodyLines = %d, want 1", th.BodyLines)
				}
			},
		},
		{
			name: "body_lines clamped to upper bound",
			yaml: "body_lines: 500\n",
			check: func(t *testing.T, th Theme) {
				if th.BodyLines != 200 {
					t.Errorf("BodyLines = %d, want 200", th.BodyLines)
				}
			},
		},
		{
			name: "pill_padding clamped",
			yaml: "pill_padding: 9\n",
			check: func(t *testing.T, th Theme) {
				if got := th.Pill.GetPaddingRight(); got != 2 {
					t.Errorf("Pill padding right = %d, want 2", got)
				}
			},
		},
		{
			name: "colors, flags, and SGR strings override",
			yaml: "title_color: \"#ff0000\"\ndim_faint: false\nshow_frontmatter: false\n" +
				"preview_header_color: \"99\"\npill_foreground: \"16\"\npill_background: \"220\"\n" +
				"highlight_sgr: \"7\"\nlink_sgr: \"4\"\n",
			check: func(t *testing.T, th Theme) {
				if got := th.Title.GetForeground(); got != lipgloss.Color("#ff0000") {
					t.Errorf("Title foreground = %v, want #ff0000", got)
				}
				if th.Dim.GetFaint() {
					t.Errorf("Dim faint = true, want false")
				}
				if th.ShowFrontmatter {
					t.Errorf("ShowFrontmatter = true, want false")
				}
				if got := th.PreviewHeader.GetForeground(); got != lipgloss.Color("99") {
					t.Errorf("PreviewHeader foreground = %v, want 99", got)
				}
				if !th.PreviewHeader.GetBold() || !th.PreviewHeader.GetUnderline() {
					t.Errorf("PreviewHeader lost bold/underline")
				}
				if got := th.Pill.GetForeground(); got != lipgloss.Color("16") {
					t.Errorf("Pill foreground = %v, want 16", got)
				}
				if got := th.Pill.GetBackground(); got != lipgloss.Color("220") {
					t.Errorf("Pill background = %v, want 220", got)
				}
				if th.HighlightSGR != "7" || th.LinkSGR != "4" {
					t.Errorf("SGR = %q/%q, want 7/4", th.HighlightSGR, th.LinkSGR)
				}
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			path := writeConfig(t, t.TempDir(), tt.yaml)
			th, err := LoadFile(path)
			if err != nil {
				t.Fatalf("LoadFile: %v", err)
			}
			tt.check(t, th)
		})
	}
}

func TestLoadFileMalformed(t *testing.T) {
	path := writeConfig(t, t.TempDir(), "markdown_style: [unclosed\n")
	if _, err := LoadFile(path); err == nil {
		t.Errorf("LoadFile(malformed) = nil error, want non-nil")
	}
	if _, err := LoadFile(filepath.Join(t.TempDir(), "missing.yaml")); err == nil {
		t.Errorf("LoadFile(missing) = nil error, want non-nil")
	}
}

func TestDiscover(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", dir)

	if _, ok := Discover(); ok {
		t.Fatalf("Discover() = true, want false for empty config dir")
	}
	cfgDir := filepath.Join(dir, "mdfu")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatalf("mkdir: %v", err)
	}
	want := writeConfig(t, cfgDir, "body_lines: 10\n")
	got, ok := Discover()
	if !ok || got != want {
		t.Errorf("Discover() = %q, %v, want %q, true", got, ok, want)
	}
}

func TestResolve(t *testing.T) {
	t.Run("explicit missing path returns error", func(t *testing.T) {
		_, err := Resolve(filepath.Join(t.TempDir(), "missing.yaml"), &bytes.Buffer{})
		if err == nil {
			t.Errorf("Resolve(missing explicit) = nil error, want non-nil")
		}
	})

	t.Run("explicit malformed file returns error", func(t *testing.T) {
		path := writeConfig(t, t.TempDir(), "body_lines: [unclosed\n")
		if _, err := Resolve(path, &bytes.Buffer{}); err == nil {
			t.Errorf("Resolve(malformed explicit) = nil error, want non-nil")
		}
	})

	t.Run("no file returns Default with no warning", func(t *testing.T) {
		t.Setenv("XDG_CONFIG_HOME", t.TempDir())
		t.Setenv("MDFU_CONFIG", "")
		var warn bytes.Buffer
		th, err := Resolve("", &warn)
		if err != nil {
			t.Fatalf("Resolve() = %v, want nil", err)
		}
		if th.BodyLines != 30 || th.PillShape != "round" {
			t.Errorf("Resolve() = %+v, want Default", th)
		}
		if warn.Len() != 0 {
			t.Errorf("warning = %q, want empty", warn.String())
		}
	})

	t.Run("malformed discovered file warns once and returns Default", func(t *testing.T) {
		dir := t.TempDir()
		t.Setenv("XDG_CONFIG_HOME", dir)
		t.Setenv("MDFU_CONFIG", "")
		if err := os.MkdirAll(filepath.Join(dir, "mdfu"), 0o755); err != nil {
			t.Fatalf("mkdir: %v", err)
		}
		writeConfig(t, filepath.Join(dir, "mdfu"), "markdown_style: [unclosed\n")
		var warn bytes.Buffer
		th, err := Resolve("", &warn)
		if err != nil {
			t.Fatalf("Resolve() = %v, want nil", err)
		}
		if th.BodyLines != 30 || th.MarkdownStyle != "" {
			t.Errorf("Resolve() = %+v, want Default", th)
		}
		if lines := strings.Count(warn.String(), "\n"); lines != 1 {
			t.Errorf("warning lines = %d (%q), want exactly 1", lines, warn.String())
		}
	})

	t.Run("MDFU_CONFIG beats Discover", func(t *testing.T) {
		dir := t.TempDir()
		t.Setenv("XDG_CONFIG_HOME", dir)
		if err := os.MkdirAll(filepath.Join(dir, "mdfu"), 0o755); err != nil {
			t.Fatalf("mkdir: %v", err)
		}
		writeConfig(t, filepath.Join(dir, "mdfu"), "body_lines: 11\n")
		envPath := writeConfig(t, t.TempDir(), "body_lines: 22\n")
		t.Setenv("MDFU_CONFIG", envPath)
		th, err := Resolve("", &bytes.Buffer{})
		if err != nil {
			t.Fatalf("Resolve() = %v, want nil", err)
		}
		if th.BodyLines != 22 {
			t.Errorf("BodyLines = %d, want 22 from MDFU_CONFIG", th.BodyLines)
		}
	})

	t.Run("explicit beats MDFU_CONFIG", func(t *testing.T) {
		t.Setenv("XDG_CONFIG_HOME", t.TempDir())
		envPath := writeConfig(t, t.TempDir(), "body_lines: 22\n")
		t.Setenv("MDFU_CONFIG", envPath)
		explicit := writeConfig(t, t.TempDir(), "body_lines: 33\n")
		th, err := Resolve(explicit, &bytes.Buffer{})
		if err != nil {
			t.Fatalf("Resolve() = %v, want nil", err)
		}
		if th.BodyLines != 33 {
			t.Errorf("BodyLines = %d, want 33 from explicit path", th.BodyLines)
		}
	})
}
