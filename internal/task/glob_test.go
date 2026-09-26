package task

import "testing"

func TestGlobMatch(t *testing.T) {
	tests := []struct {
		name    string
		pattern string
		target  string
		want    bool
	}{
		{"doublestar matches top level", "**/*.md", "a.md", true},
		{"doublestar matches one deep", "**/*.md", "x/a.md", true},
		{"doublestar matches many deep", "**/*.md", "x/y/z.md", true},
		{"doublestar prefix then literal", "src/**/*.md", "src/a.md", true},
		{"doublestar prefix nested", "src/**/*.md", "src/x/y/a.md", true},
		{"doublestar prefix requires the prefix", "src/**/*.md", "a.md", false},

		{"asterisk does not cross a separator", "*.md", "a.md", true},
		{"asterisk top level only", "*.md", "x/a.md", false},
		{"asterisk within a directory", "a/*.md", "a/b.md", true},
		{"asterisk does not span directories", "a/*.md", "a/b/c.md", false},
		{"asterisk matches empty run", "*.md", ".md", true},

		{"literal question mark", "a?.md", "a?.md", true},
		{"literal question mark is not a wildcard", "a?.md", "ab.md", false},
		{"literal bracket expression", "[ab].md", "[ab].md", true},
		{"bracket expression is not a class", "[ab].md", "a.md", false},

		{"trailing doublestar requires a segment", "archive/**", "archive", false},
		{"trailing doublestar matches a file", "archive/**", "archive/a.md", true},
		{"trailing doublestar matches nested", "archive/**", "archive/a/b.md", true},

		{"bare doublestar matches one segment", "**", "a", true},
		{"bare doublestar matches many segments", "**", "a/b/c", true},

		{"exact match", "a.md", "a.md", true},
		{"exact mismatch", "a.md", "b.md", false},
		{"exact does not match nested", "a.md", "x/a.md", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := Match(tt.pattern, tt.target); got != tt.want {
				t.Errorf("Match(%q, %q) = %v, want %v", tt.pattern, tt.target, got, tt.want)
			}
		})
	}
}
