package task

import (
	"reflect"
	"testing"
)

func TestFenceMask(t *testing.T) {
	tests := []struct {
		name   string
		lines  []string
		want   []bool
		hidden int
	}{
		{
			name:   "backtick fence",
			lines:  []string{"# Title", "```", "- [ ] A-1 Not a task", "```", "- [ ] A-2 Real"},
			want:   []bool{false, true, true, true, false},
			hidden: 1,
		},
		{
			name:   "tilde fence",
			lines:  []string{"~~~", "text", "~~~", "- [ ] A-1 Real"},
			want:   []bool{true, true, true, false},
			hidden: 0,
		},
		{
			name:   "unclosed fence masks to EOF",
			lines:  []string{"```", "- [ ] A-1 Hidden", "- [ ] A-2 Hidden"},
			want:   []bool{true, true, true},
			hidden: 2,
		},
		{
			name:   "up to three leading spaces opens",
			lines:  []string{"   ```", "x", "   ```"},
			want:   []bool{true, true, true},
			hidden: 0,
		},
		{
			name:   "four leading spaces is not a fence",
			lines:  []string{"    ```", "x"},
			want:   []bool{false, false},
			hidden: 0,
		},
		{
			name:   "closing run must be at least as long",
			lines:  []string{"````", "x", "```", "y", "````"},
			want:   []bool{true, true, true, true, true},
			hidden: 0,
		},
		{
			name:   "shorter opposite fence does not close",
			lines:  []string{"```", "~~~", "```"},
			want:   []bool{true, true, true},
			hidden: 0,
		},
		{
			name:   "info string on opening fence",
			lines:  []string{"```text", "- [ ] A-1", "```", "after"},
			want:   []bool{true, true, true, false},
			hidden: 1,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := fenceMask(tt.lines)
			if !reflect.DeepEqual(got, tt.want) {
				t.Errorf("fenceMask(%q) = %v, want %v", tt.lines, got, tt.want)
			}
			if hidden := maskedHeaderCount(tt.lines, got); hidden != tt.hidden {
				t.Errorf("masked header-shaped lines = %d, want %d", hidden, tt.hidden)
			}
		})
	}
}

func maskedHeaderCount(lines []string, mask []bool) int {
	count := 0
	for i, line := range lines {
		if !mask[i] {
			continue
		}
		if _, ok := ParseHeader(line); ok {
			count++
		}
	}
	return count
}

func TestCollectBody(t *testing.T) {
	tests := []struct {
		name       string
		lines      []string
		start      int
		wantEnd    int
		wantOK     bool
		wantBody   string
		wantIndent string
	}{
		{
			name:       "two line body",
			lines:      []string{"- [ ] A-1 Title", "  one", "  two", "- [ ] A-2 Next"},
			start:      0,
			wantEnd:    3,
			wantOK:     true,
			wantBody:   "one\ntwo",
			wantIndent: "  ",
		},
		{
			name:       "blank line inside body",
			lines:      []string{"- [ ] A-1 Title", "  one", "", "  two", "- [ ] A-2 Next"},
			start:      0,
			wantEnd:    4,
			wantOK:     true,
			wantBody:   "one\n\ntwo",
			wantIndent: "  ",
		},
		{
			name:       "trailing blank before non-indented is not body",
			lines:      []string{"- [ ] A-1 Title", "  one", "", "- [ ] A-2 Next"},
			start:      0,
			wantEnd:    2,
			wantOK:     true,
			wantBody:   "one",
			wantIndent: "  ",
		},
		{
			name:       "no body",
			lines:      []string{"- [ ] A-1 Title", "- [ ] A-2 Next"},
			start:      0,
			wantEnd:    1,
			wantOK:     true,
			wantBody:   "",
			wantIndent: "",
		},
		{
			name:       "untab deep indent dedents to minimum",
			lines:      []string{"- [ ] A-1 Title", "    four", "  two", "- [ ] A-2 Next"},
			start:      0,
			wantEnd:    3,
			wantOK:     true,
			wantBody:   "  four\ntwo",
			wantIndent: "  ",
		},
		{
			name:   "out of range",
			lines:  []string{"- [ ] A-1 Title"},
			start:  5,
			wantOK: false,
		},
		{
			name:   "not a header",
			lines:  []string{"plain", "  body"},
			start:  0,
			wantOK: false,
		},
		{
			name:   "fence masked start",
			lines:  []string{"```", "- [ ] A-1 Title", "  body", "```"},
			start:  1,
			wantOK: false,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			end, ok := blockRange(tt.lines, tt.start)
			if ok != tt.wantOK {
				t.Fatalf("blockRange ok = %v, want %v", ok, tt.wantOK)
			}
			if !ok {
				return
			}
			if end != tt.wantEnd {
				t.Fatalf("blockRange end = %d, want %d", end, tt.wantEnd)
			}
			body, indent := collectBody(tt.lines, tt.start, end)
			if body != tt.wantBody {
				t.Errorf("collectBody body = %q, want %q", body, tt.wantBody)
			}
			if indent != tt.wantIndent {
				t.Errorf("collectBody indent = %q, want %q", indent, tt.wantIndent)
			}
		})
	}
}
