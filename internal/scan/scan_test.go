package scan

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestWalkMarkdown(t *testing.T) {
	root := t.TempDir()

	files := []string{
		"a.md",
		"b.txt",
		filepath.Join(".hidden", "c.md"),
		filepath.Join("sub", "d.md"),
		filepath.Join(".git", "e.md"),
	}
	for _, f := range files {
		p := filepath.Join(root, f)
		if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, []byte("# test\n"), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	tests := []struct {
		name          string
		includeHidden bool
		want          []string
	}{
		{
			name:          "excludes hidden and .git",
			includeHidden: false,
			want: []string{
				filepath.Join(root, "a.md"),
				filepath.Join(root, "sub", "d.md"),
			},
		},
		{
			name:          "includes hidden but still excludes .git",
			includeHidden: true,
			want: []string{
				filepath.Join(root, ".hidden", "c.md"),
				filepath.Join(root, "a.md"),
				filepath.Join(root, "sub", "d.md"),
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := WalkMarkdown(Options{Root: root, IncludeHidden: tt.includeHidden})
			if err != nil {
				t.Fatalf("WalkMarkdown() error = %v", err)
			}
			if !reflect.DeepEqual(got, tt.want) {
				t.Errorf("WalkMarkdown() = %v, want %v", got, tt.want)
			}
		})
	}
}
