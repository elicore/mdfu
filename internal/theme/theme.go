// Package theme loads the optional mdfu TUI theme: flat scalar/enum keys from
// an XDG YAML file merged per key over builtin defaults that reproduce the
// stock look. The file is never required; absent or undiscoverable config
// yields the builtin defaults.
package theme

import (
	"fmt"
	"io"
	"os"
	"path/filepath"

	"github.com/charmbracelet/lipgloss"
	"gopkg.in/yaml.v3"
)

// Theme is the resolved set of styles and scalar settings consumed by the TUI.
// Styles are immutable lipgloss values; scalars stay readable so callers can
// emit raw SGR bytes or pick a glamour style.
type Theme struct {
	Cursor         lipgloss.Style
	Selected       lipgloss.Style
	Dim            lipgloss.Style
	Title          lipgloss.Style
	Filename       lipgloss.Style
	PreviewHeader  lipgloss.Style
	FrontmatterKey lipgloss.Style
	Pill           lipgloss.Style

	PillShape       string // "none" | "round"
	MarkdownStyle   string // "" | "dark" | "light"
	ShowFrontmatter bool
	BodyLines       int
	HighlightSGR    string // SGR parameters for match emphasis, e.g. "1;30;103"
	LinkSGR         string // SGR parameters for hyperlink labels
}

// fileConfig mirrors the YAML theme file. Pointer fields distinguish an unset
// key (nil, keep the builtin default) from an explicitly set value.
type fileConfig struct {
	MarkdownStyle       *string `yaml:"markdown_style"`
	ShowFrontmatter     *bool   `yaml:"show_frontmatter"`
	BodyLines           *int    `yaml:"body_lines"`
	CursorColor         *string `yaml:"cursor_color"`
	SelectedColor       *string `yaml:"selected_color"`
	TitleColor          *string `yaml:"title_color"`
	FilenameColor       *string `yaml:"filename_color"`
	PreviewHeaderColor  *string `yaml:"preview_header_color"`
	FrontmatterKeyColor *string `yaml:"frontmatter_key_color"`
	DimFaint            *bool   `yaml:"dim_faint"`
	PillForeground      *string `yaml:"pill_foreground"`
	PillBackground      *string `yaml:"pill_background"`
	PillShape           *string `yaml:"pill_shape"`
	PillPadding         *int    `yaml:"pill_padding"`
	HighlightSGR        *string `yaml:"highlight_sgr"`
	LinkSGR             *string `yaml:"link_sgr"`
}

// Builtin defaults. The colors reproduce the inline styles previously
// hardcoded in internal/tui; the SGR strings are the exact parameter bytes of
// the old hlStart/linkSGR constants (without the CSI wrapper).
const (
	defaultCursorColor         = "212"
	defaultSelectedColor       = "82"
	defaultTitleColor          = "212"
	defaultFilenameColor       = "244"
	defaultFrontmatterKeyColor = "245"
	defaultPillForeground      = "231"
	defaultPillBackground      = "62"
	defaultPillShape           = "round"
	defaultPillPadding         = 1
	defaultBodyLines           = 30
	defaultHighlightSGR        = "1;30;103"
	defaultLinkSGR             = "1;4;38;5;212"
)

// Default returns the builtin theme, which reproduces the stock TUI look when
// no config file exists.
func Default() Theme {
	return Theme{
		Cursor:         lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color(defaultCursorColor)),
		Selected:       lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color(defaultSelectedColor)),
		Dim:            lipgloss.NewStyle().Faint(true),
		Title:          lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color(defaultTitleColor)),
		Filename:       lipgloss.NewStyle().Foreground(lipgloss.Color(defaultFilenameColor)),
		PreviewHeader:  lipgloss.NewStyle().Bold(true).Underline(true),
		FrontmatterKey: lipgloss.NewStyle().Foreground(lipgloss.Color(defaultFrontmatterKeyColor)),
		Pill: lipgloss.NewStyle().
			Foreground(lipgloss.Color(defaultPillForeground)).
			Background(lipgloss.Color(defaultPillBackground)).
			Padding(0, defaultPillPadding),

		PillShape:       defaultPillShape,
		MarkdownStyle:   "",
		ShowFrontmatter: true,
		BodyLines:       defaultBodyLines,
		HighlightSGR:    defaultHighlightSGR,
		LinkSGR:         defaultLinkSGR,
	}
}

// LoadFile reads the YAML theme at path and merges it per key over Default.
// Unknown keys are ignored; invalid enum values keep the builtin default and
// numeric settings are clamped into their supported ranges.
func LoadFile(path string) (Theme, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return Theme{}, fmt.Errorf("theme: read %s: %w", path, err)
	}
	var fc fileConfig
	if err := yaml.Unmarshal(data, &fc); err != nil {
		return Theme{}, fmt.Errorf("theme: parse %s: %w", path, err)
	}
	return fc.apply(Default()), nil
}

// Discover reports the default theme path <os.UserConfigDir>/mdfu/config.yaml
// and whether that file exists.
func Discover() (string, bool) {
	dir, err := os.UserConfigDir()
	if err != nil {
		return "", false
	}
	path := filepath.Join(dir, "mdfu", "config.yaml")
	info, err := os.Stat(path)
	if err != nil || info.IsDir() {
		return "", false
	}
	return path, true
}

// Resolve picks the theme by precedence: explicit path > $MDFU_CONFIG >
// discovered XDG file > builtin defaults. An explicitly requested file that is
// missing, unreadable, or malformed is an error; a malformed discovered file
// downgrades to Default with exactly one warning line written to warn.
func Resolve(explicit string, warn io.Writer) (Theme, error) {
	path := explicit
	discovered := false
	if path == "" {
		path = os.Getenv("MDFU_CONFIG")
	}
	if path == "" {
		if p, ok := Discover(); ok {
			path, discovered = p, true
		}
	}
	if path == "" {
		return Default(), nil
	}
	th, err := LoadFile(path)
	if err == nil {
		return th, nil
	}
	if !discovered {
		return Theme{}, err
	}
	if warn == nil {
		warn = io.Discard
	}
	fmt.Fprintf(warn, "mdfu: ignoring theme file %s: %v\n", path, err)
	return Default(), nil
}

// apply merges every set key over t, keeping t's value for unset or invalid
// keys.
func (c fileConfig) apply(t Theme) Theme {
	if c.CursorColor != nil {
		t.Cursor = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color(*c.CursorColor))
	}
	if c.SelectedColor != nil {
		t.Selected = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color(*c.SelectedColor))
	}
	if c.TitleColor != nil {
		t.Title = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color(*c.TitleColor))
	}
	if c.FilenameColor != nil {
		t.Filename = lipgloss.NewStyle().Foreground(lipgloss.Color(*c.FilenameColor))
	}
	if c.PreviewHeaderColor != nil {
		t.PreviewHeader = lipgloss.NewStyle().Bold(true).Underline(true).
			Foreground(lipgloss.Color(*c.PreviewHeaderColor))
	}
	if c.FrontmatterKeyColor != nil {
		t.FrontmatterKey = lipgloss.NewStyle().Foreground(lipgloss.Color(*c.FrontmatterKeyColor))
	}
	if c.DimFaint != nil {
		t.Dim = lipgloss.NewStyle().Faint(*c.DimFaint)
	}
	if c.PillForeground != nil || c.PillBackground != nil || c.PillPadding != nil {
		fg, bg, pad := defaultPillForeground, defaultPillBackground, defaultPillPadding
		if c.PillForeground != nil {
			fg = *c.PillForeground
		}
		if c.PillBackground != nil {
			bg = *c.PillBackground
		}
		if c.PillPadding != nil {
			pad = min(max(*c.PillPadding, 0), 2)
		}
		t.Pill = lipgloss.NewStyle().
			Foreground(lipgloss.Color(fg)).
			Background(lipgloss.Color(bg)).
			Padding(0, pad)
	}
	if c.PillShape != nil {
		switch *c.PillShape {
		case "none", "round":
			t.PillShape = *c.PillShape
		}
	}
	if c.MarkdownStyle != nil {
		switch *c.MarkdownStyle {
		case "", "dark", "light":
			t.MarkdownStyle = *c.MarkdownStyle
		}
	}
	if c.ShowFrontmatter != nil {
		t.ShowFrontmatter = *c.ShowFrontmatter
	}
	if c.BodyLines != nil {
		t.BodyLines = min(max(*c.BodyLines, 1), 200)
	}
	if c.HighlightSGR != nil {
		t.HighlightSGR = *c.HighlightSGR
	}
	if c.LinkSGR != nil {
		t.LinkSGR = *c.LinkSGR
	}
	return t
}
