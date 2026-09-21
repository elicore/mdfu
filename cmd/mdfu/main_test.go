package main

import (
	"io"
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
