package output

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestFormatPaths(t *testing.T) {
	results := []Result{
		{Path: "a.md", Title: "A"},
		{Path: "sub/b.md", Title: "B"},
	}
	want := "a.md\nsub/b.md\n"
	if got := FormatPaths(results); got != want {
		t.Errorf("FormatPaths = %q, want %q", got, want)
	}
	if got := FormatPaths(nil); got != "" {
		t.Errorf("FormatPaths(nil) = %q, want empty", got)
	}
}

func TestFormatJSON(t *testing.T) {
	results := []Result{
		{Path: "a.md", Score: 1.5, Title: "A", DocType: "Note", Snippet: "hello"},
		{Path: "b.md", Score: 0.5, Title: "B", DocType: "Log", Snippet: "world"},
	}
	out, err := FormatJSON(results)
	if err != nil {
		t.Fatalf("FormatJSON error: %v", err)
	}
	var back []Result
	if err := json.Unmarshal([]byte(out), &back); err != nil {
		t.Fatalf("FormatJSON output is not valid JSON: %v\n%s", err, out)
	}
	if len(back) != 2 || back[0].Path != "a.md" || back[1].Title != "B" {
		t.Errorf("JSON round-trip mismatch: %+v", back)
	}
	if !strings.Contains(out, "\n") {
		t.Errorf("expected indented JSON, got: %q", out)
	}

	empty, err := FormatJSON(nil)
	if err != nil {
		t.Fatalf("FormatJSON(nil) error: %v", err)
	}
	if strings.TrimSpace(empty) != "[]" {
		t.Errorf("FormatJSON(nil) = %q, want []", empty)
	}
}

func TestFormatVimgrep(t *testing.T) {
	results := []Result{
		{Path: "a.md", Title: "Hello A"},
		{Path: "sub/b.md", Title: "Hello B"},
	}
	want := "a.md:1:1:Hello A\nsub/b.md:1:1:Hello B\n"
	if got := FormatVimgrep(results); got != want {
		t.Errorf("FormatVimgrep = %q, want %q", got, want)
	}
	if got := FormatVimgrep(nil); got != "" {
		t.Errorf("FormatVimgrep(nil) = %q, want empty", got)
	}
}

func TestSnippetHit(t *testing.T) {
	body := "This is a long document body that mentions Golang somewhere in the middle of all this text for testing."
	got := Snippet(body, []string{"golang"}, 30)
	if !strings.Contains(strings.ToLower(got), "golang") {
		t.Errorf("Snippet hit should contain the term, got %q", got)
	}
	if n := len([]rune(got)); n > 30+2*3 { // width + two ellipsis markers
		t.Errorf("Snippet too long (%d runes): %q", n, got)
	}
}

func TestSnippetMissReturnsPrefix(t *testing.T) {
	body := "abcdefghijklmnopqrstuvwxyz0123456789"
	got := Snippet(body, []string{"zzz"}, 10)
	want := "abcdefghij..."
	if got != want {
		t.Errorf("Snippet miss = %q, want %q", got, want)
	}
}

func TestSnippetEmptyTerms(t *testing.T) {
	body := "abcdefghijklmnopqrstuvwxyz0123456789"
	got := Snippet(body, nil, 10)
	want := "abcdefghij..."
	if got != want {
		t.Errorf("Snippet empty terms = %q, want %q", got, want)
	}
}

func TestSnippetShortBody(t *testing.T) {
	if got := Snippet("hi", []string{"hi"}, 10); got != "hi" {
		t.Errorf("Snippet short body = %q, want %q", got, "hi")
	}
	if got := Snippet("", []string{"hi"}, 10); got != "" {
		t.Errorf("Snippet empty body = %q, want empty", got)
	}
}
