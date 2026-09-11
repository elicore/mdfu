package output

import (
	"encoding/json"
	"fmt"
	"strings"
)

// Result is the display unit for --filter mode. It is intentionally
// decoupled from internal/scan, internal/query and internal/search:
// callers map ranked documents to Results before formatting.
type Result struct {
	Path    string  `json:"path"`
	Score   float64 `json:"score"`
	Title   string  `json:"title"`
	DocType string  `json:"type"`
	Snippet string  `json:"snippet"`
}

// FormatPaths renders one path per line (fzf-compatible).
func FormatPaths(results []Result) string {
	var b strings.Builder
	for _, r := range results {
		b.WriteString(r.Path)
		b.WriteByte('\n')
	}
	return b.String()
}

// FormatJSON renders results as indented JSON.
func FormatJSON(results []Result) (string, error) {
	if results == nil {
		results = []Result{}
	}
	buf, err := json.MarshalIndent(results, "", "  ")
	if err != nil {
		return "", err
	}
	return string(buf), nil
}

// FormatVimgrep renders `path:1:1:title` lines for editor quickfix lists.
func FormatVimgrep(results []Result) string {
	var b strings.Builder
	for _, r := range results {
		fmt.Fprintf(&b, "%s:1:1:%s\n", r.Path, r.Title)
	}
	return b.String()
}

// Snippet returns a window of at most width runes from body containing the
// first case-insensitive occurrence of any of terms. The window is trimmed
// with "..." ellipsis markers where content was cut. If no term hits (or
// terms is empty), it returns the leading prefix trimmed with "...".
func Snippet(body string, terms []string, width int) string {
	if width <= 0 {
		width = 120
	}
	// Normalize whitespace so the snippet is a single line.
	s := strings.Join(strings.Fields(body), " ")
	if s == "" {
		return ""
	}
	r := []rune(s)
	if len(r) <= width {
		return s
	}

	lower := strings.ToLower(s)
	best := -1
	for _, t := range terms {
		t = strings.TrimSpace(t)
		if t == "" {
			continue
		}
		if idx := strings.Index(lower, strings.ToLower(t)); idx >= 0 && (best == -1 || idx < best) {
			best = idx
		}
	}

	if best == -1 {
		return string(r[:width]) + "..."
	}

	bestRune := len([]rune(lower[:best]))
	start := bestRune - width/4
	if start < 0 {
		start = 0
	}
	end := start + width
	if end > len(r) {
		end = len(r)
		start = end - width
	}
	out := string(r[start:end])
	if start > 0 {
		out = "..." + out
	}
	if end < len(r) {
		out += "..."
	}
	return out
}
