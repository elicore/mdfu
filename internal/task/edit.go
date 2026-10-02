package task

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
)

// FileEdit is a byte-preserving, line-anchored view of one task file. It holds
// the original bytes, the detected line ending, whether the file ended with a
// newline, and the line index; replacements are staged in memory and only hit
// disk on Commit.
type FileEdit struct {
	path            string
	original        []byte
	origLines       []string
	lines           []string
	eol             string
	trailingNewline bool
	dirty           bool
}

// Open reads path, detects its line ending per file, and splits it into lines
// while remembering the presence or absence of a trailing newline.
func Open(path string) (*FileEdit, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	fe := &FileEdit{path: path}
	fe.reload(data)
	return fe, nil
}

// reload resets the in-memory state from raw bytes.
func (fe *FileEdit) reload(data []byte) {
	fe.original = append([]byte(nil), data...)
	fe.eol = detectEOL(data)
	fe.lines, fe.trailingNewline = splitLines(data, fe.eol)
	fe.origLines = append([]string(nil), fe.lines...)
	fe.dirty = false
}

// ReplaceLines stages a replacement of the half-open line range [start, end)
// without writing. start == end inserts. Bounds are validated against the
// current staged lines.
func (fe *FileEdit) ReplaceLines(start, end int, lines []string) error {
	if start < 0 || end < start || end > len(fe.lines) {
		return fmt.Errorf("line range [%d,%d) out of bounds for %d lines", start, end, len(fe.lines))
	}
	staged := make([]string, 0, len(fe.lines)-(end-start)+len(lines))
	staged = append(staged, fe.lines[:start]...)
	staged = append(staged, lines...)
	staged = append(staged, fe.lines[end:]...)
	fe.lines = staged
	fe.dirty = !slices.Equal(fe.lines, fe.origLines)
	return nil
}

// Bytes returns the staged content, byte-exact to the original when nothing was
// staged.
func (fe *FileEdit) Bytes() []byte {
	if !fe.dirty {
		return append([]byte(nil), fe.original...)
	}
	content := strings.Join(fe.lines, fe.eol)
	if fe.trailingNewline {
		content += fe.eol
	}
	return []byte(content)
}

// Commit writes the staged content to a temp file in the same directory and
// renames it over path, preserving the original file mode. It is a no-op when
// nothing changed.
func (fe *FileEdit) Commit() error {
	if !fe.dirty {
		return nil
	}
	data := fe.Bytes()
	mode := os.FileMode(0o644)
	if info, err := os.Stat(fe.path); err == nil {
		mode = info.Mode().Perm()
	}
	if err := atomicWrite(fe.path, data, mode); err != nil {
		return err
	}
	fe.reload(data)
	return nil
}

// StaleError reports that a task did not sit at its recorded line when the file
// was re-read.
type StaleError struct {
	File string
	ID   string
}

// Error is exactly the E17 string from SPEC.md §J.
func (e *StaleError) Error() string {
	return fmt.Sprintf("file changed, task '%s' not at expected line", e.ID)
}

// Verify re-reads path from disk and confirms the task is still at the recorded
// 1-based line with an unchanged header, returning a *StaleError otherwise.
func (fe *FileEdit) Verify(taskID string, line int) error {
	stale := func() error { return &StaleError{File: fe.path, ID: taskID} }

	data, err := os.ReadFile(fe.path)
	if err != nil {
		return err
	}
	lines, _ := splitLines(data, detectEOL(data))
	if line < 1 || line > len(lines) {
		return stale()
	}
	parsed, ok := ParseHeader(lines[line-1])
	if !ok || parsed.ID != taskID {
		return stale()
	}
	if line <= len(fe.origLines) && lines[line-1] != fe.origLines[line-1] {
		return stale()
	}
	return nil
}

// VerifyLine re-reads path from disk and confirms line still holds exactly
// raw. Broadened checkbox items carry no ID, so they are matched by their raw
// header instead.
func (fe *FileEdit) VerifyLine(line int, raw string) error {
	data, err := os.ReadFile(fe.path)
	if err != nil {
		return err
	}
	lines, _ := splitLines(data, detectEOL(data))
	if line < 1 || line > len(lines) || lines[line-1] != strings.TrimSuffix(raw, "\r") {
		return &StaleError{File: fe.path, ID: raw}
	}
	return nil
}

// VerifyTask confirms t still sits at its recorded line, picking the check
// that matches how it was parsed: exact raw header for broadened items,
// ID-based for strict mdtask tasks.
func (fe *FileEdit) VerifyTask(t Task) error {
	if t.Broad {
		return fe.VerifyLine(t.Line, t.HeaderRaw)
	}
	return fe.Verify(t.ID, t.Line)
}

// ReplaceHeaderLine stages a replacement of the 1-based header line.
func (fe *FileEdit) ReplaceHeaderLine(line int, header string) error {
	return fe.ReplaceLines(line-1, line, []string{header})
}

// ReplaceBodyRange stages a replacement of the half-open line range
// [start, end) with bodyLines.
func (fe *FileEdit) ReplaceBodyRange(start, end int, bodyLines []string) error {
	return fe.ReplaceLines(start, end, bodyLines)
}

// FlipCheckbox stages the checkbox of the 1-based header line flipped between
// unchecked and checked, leaving the rest of the line byte-identical. It
// accepts both strict mdtask headers and broadened checkbox items.
func (fe *FileEdit) FlipCheckbox(line int) error {
	if line < 1 || line > len(fe.lines) {
		return fmt.Errorf("line %d out of bounds for %d lines", line, len(fe.lines))
	}
	flipped, ok := FlipCheckboxAny(fe.lines[line-1])
	if !ok {
		return fmt.Errorf("line %d is not a task header", line)
	}
	return fe.ReplaceLines(line-1, line, []string{flipped})
}

// AppendBlock appends block to the end of path, creating parent directories as
// needed, and preserves the existing line ending. When the file already has
// content and lacks a trailing newline one is inserted before the block; the
// result ends with a newline.
func AppendBlock(path, block string) error {
	block = strings.TrimRight(block, "\r\n")

	mode := os.FileMode(0o644)
	var existing []byte
	existErr := error(nil)
	if info, err := os.Stat(path); err == nil {
		mode = info.Mode().Perm()
	}
	existing, existErr = os.ReadFile(path)
	switch {
	case existErr == nil:
	case errors.Is(existErr, os.ErrNotExist):
		existing = nil
	default:
		return existErr
	}

	eol := detectEOL(existing)
	if len(existing) == 0 {
		eol = "\n"
	}

	var b strings.Builder
	if len(existing) > 0 {
		b.Write(existing)
		if !strings.HasSuffix(string(existing), eol) {
			b.WriteString(eol)
		}
	}
	if block != "" {
		lines := strings.Split(strings.ReplaceAll(block, "\r\n", "\n"), "\n")
		b.WriteString(strings.Join(lines, eol))
		b.WriteString(eol)
	}
	return atomicWrite(path, []byte(b.String()), mode)
}

// detectEOL reports the line ending used by data, preferring CRLF when present.
func detectEOL(data []byte) string {
	if bytes.Contains(data, []byte("\r\n")) {
		return "\r\n"
	}
	return "\n"
}

// splitLines splits data on eol without retaining the endings and reports
// whether data ended with eol.
func splitLines(data []byte, eol string) ([]string, bool) {
	s := string(data)
	trailing := false
	if strings.HasSuffix(s, eol) {
		trailing = true
		s = s[:len(s)-len(eol)]
	}
	if s == "" {
		return nil, trailing
	}
	return strings.Split(s, eol), trailing
}

// atomicWrite writes data to a temp file in path's directory and renames it
// over path, preserving mode. The temp file is removed on any failure.
func atomicWrite(path string, data []byte, mode os.FileMode) error {
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, ".mdfu-task-*")
	if err != nil {
		return err
	}
	name := tmp.Name()
	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		os.Remove(name)
		return err
	}
	if err := tmp.Chmod(mode); err != nil {
		tmp.Close()
		os.Remove(name)
		return err
	}
	if err := tmp.Close(); err != nil {
		os.Remove(name)
		return err
	}
	if err := os.Rename(name, path); err != nil {
		os.Remove(name)
		return err
	}
	return nil
}
