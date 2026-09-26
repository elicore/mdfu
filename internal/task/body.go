package task

import "strings"

// fenceMask reports, for every line, whether it lies inside a CommonMark
// fenced code block. The opening fence is a line with at most three leading
// spaces followed by three or more backticks or tildes; the closing fence is a
// line with at most three leading spaces and at least as many of the same
// character. An unclosed fence masks through end of file. The mask gates both
// header detection and body extraction.
func fenceMask(lines []string) []bool {
	mask := make([]bool, len(lines))
	var openChar byte
	var openLen int

	for i, raw := range lines {
		line := strings.TrimSuffix(raw, "\r")
		if openChar == 0 {
			if ch, n, ok := openingFence(line); ok {
				openChar = ch
				openLen = n
				mask[i] = true
			}
			continue
		}
		mask[i] = true
		if _, n, ok := closingFence(line, openChar); ok && n >= openLen {
			openChar = 0
			openLen = 0
		}
	}
	return mask
}

// openingFence recognizes an opening code fence and returns its character and
// run length.
func openingFence(line string) (byte, int, bool) {
	i := 0
	for i < len(line) && line[i] == ' ' {
		i++
	}
	if i > 3 {
		return 0, 0, false
	}
	rest := line[i:]
	switch {
	case strings.HasPrefix(rest, "```"):
		return '`', runLength(rest, '`'), true
	case strings.HasPrefix(rest, "~~~"):
		return '~', runLength(rest, '~'), true
	}
	return 0, 0, false
}

// closingFence recognizes a closing fence for the given character.
func closingFence(line string, ch byte) (byte, int, bool) {
	i := 0
	for i < len(line) && line[i] == ' ' {
		i++
	}
	if i > 3 {
		return 0, 0, false
	}
	rest := line[i:]
	if len(rest) == 0 || rest[0] != ch {
		return 0, 0, false
	}
	n := runLength(rest, ch)
	if n < 3 {
		return 0, 0, false
	}
	return ch, n, true
}

// runLength counts the leading run of ch in s.
func runLength(s string, ch byte) int {
	n := 0
	for n < len(s) && s[n] == ch {
		n++
	}
	return n
}

// blockRange returns the end of the task block that starts at the header line
// index start: the index one past its last body line. It reports ok == false
// when start is out of range, fence-masked, or not a header. A trailing run of
// blank lines is included only when an indented body line follows it.
func blockRange(lines []string, start int) (int, bool) {
	if start < 0 || start >= len(lines) {
		return 0, false
	}
	if fenceMask(lines)[start] {
		return 0, false
	}
	if _, ok := ParseHeader(lines[start]); !ok {
		return 0, false
	}

	lastIndented := -1
	for i := start + 1; i < len(lines); i++ {
		line := strings.TrimSuffix(lines[i], "\r")
		if blank(line) {
			continue
		}
		if line[0] == ' ' || line[0] == '\t' {
			lastIndented = i
			continue
		}
		break
	}
	if lastIndented < 0 {
		return start + 1, true
	}
	return lastIndented + 1, true
}

// collectBody dedents the body lines in lines[start+1:end] by the minimum
// indent of the non-empty lines and returns the dedented text and that indent.
func collectBody(lines []string, start, end int) (string, string) {
	if start < 0 || end > len(lines) || end <= start+1 {
		return "", ""
	}
	body := lines[start+1 : end]

	indent := ""
	found := false
	for _, raw := range body {
		line := strings.TrimSuffix(raw, "\r")
		if blank(line) {
			continue
		}
		ws := leadingWhitespace(line)
		if !found || len(ws) < len(indent) {
			indent = ws
			found = true
		}
	}
	if !found {
		return "", ""
	}

	out := make([]string, len(body))
	for i, raw := range body {
		line := strings.TrimSuffix(raw, "\r")
		if blank(line) {
			out[i] = ""
			continue
		}
		if strings.HasPrefix(line, indent) {
			out[i] = line[len(indent):]
			continue
		}
		out[i] = trimLeadingWhitespace(line, len(indent))
	}
	return strings.Join(out, "\n"), indent
}

// blank reports whether a line contains only spaces and tabs.
func blank(line string) bool {
	for i := 0; i < len(line); i++ {
		if line[i] != ' ' && line[i] != '\t' {
			return false
		}
	}
	return true
}

// leadingWhitespace returns the leading run of spaces and tabs in line.
func leadingWhitespace(line string) string {
	i := 0
	for i < len(line) && (line[i] == ' ' || line[i] == '\t') {
		i++
	}
	return line[:i]
}

// trimLeadingWhitespace removes at most n leading spaces or tabs.
func trimLeadingWhitespace(line string, n int) string {
	i := 0
	for i < len(line) && i < n && (line[i] == ' ' || line[i] == '\t') {
		i++
	}
	return line[i:]
}
