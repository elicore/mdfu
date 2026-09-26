package task

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestFileEditRoundTrip(t *testing.T) {
	tests := []struct {
		name    string
		content []byte
		line    int
		header  string
		want    []byte
	}{
		{
			name:    "crlf file",
			content: []byte("- [ ] A-1 Old\r\n\t\t#x\r\n"),
			line:    1,
			header:  "- [x] A-1 New",
			want:    []byte("- [x] A-1 New\r\n\t\t#x\r\n"),
		},
		{
			name:    "no trailing newline",
			content: []byte("- [ ] A-1 Title"),
			line:    1,
			header:  "- [x] A-1 Title",
			want:    []byte("- [x] A-1 Title"),
		},
		{
			name:    "eof header without body",
			content: []byte("- [ ] A-1 Title\n"),
			line:    1,
			header:  "- [x] A-1 Title",
			want:    []byte("- [x] A-1 Title\n"),
		},
		{
			name:    "invalid utf-8 bytes",
			content: []byte("- [ ] A-1 \xff\xfe\n"),
			line:    1,
			header:  "- [x] A-1 \xff\xfe",
			want:    []byte("- [x] A-1 \xff\xfe\n"),
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "tasks.md")
			if err := os.WriteFile(path, tt.content, 0o640); err != nil {
				t.Fatal(err)
			}

			fe, err := Open(path)
			if err != nil {
				t.Fatalf("Open error = %v", err)
			}
			if !bytes.Equal(fe.Bytes(), tt.content) {
				t.Fatalf("Bytes() = %q, want byte-exact %q", fe.Bytes(), tt.content)
			}
			if err := fe.ReplaceHeaderLine(tt.line, tt.header); err != nil {
				t.Fatalf("ReplaceHeaderLine error = %v", err)
			}
			if err := fe.Commit(); err != nil {
				t.Fatalf("Commit error = %v", err)
			}

			got, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.Equal(got, tt.want) {
				t.Errorf("file = %q, want %q", got, tt.want)
			}
			info, err := os.Stat(path)
			if err != nil {
				t.Fatal(err)
			}
			if info.Mode().Perm() != 0o640 {
				t.Errorf("mode = %o, want 640", info.Mode().Perm())
			}
		})
	}
}

func TestFileEditStale(t *testing.T) {
	path := filepath.Join(t.TempDir(), "tasks.md")
	if err := os.WriteFile(path, []byte("- [ ] A-1 Title\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	fe, err := Open(path)
	if err != nil {
		t.Fatalf("Open error = %v", err)
	}

	if err := fe.Verify("A-1", 1); err != nil {
		t.Fatalf("Verify before mutation = %v, want nil", err)
	}

	if err := os.WriteFile(path, []byte("# changed\n- [ ] A-1 Title\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	err = fe.Verify("A-1", 1)
	if err == nil {
		t.Fatal("Verify after mutation = nil, want StaleError")
	}
	if want := "file changed, task 'A-1' not at expected line"; err.Error() != want {
		t.Errorf("Verify error = %q, want %q", err.Error(), want)
	}
	var stale *StaleError
	if !errors.As(err, &stale) {
		t.Errorf("Verify error type = %T, want *StaleError", err)
	}
}

func TestAppendBlock(t *testing.T) {
	t.Run("creates parents and preserves crlf", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "nested", "archive.md")
		if err := AppendBlock(path, "- [x] A-9 Done\r\n\tbody\r\n"); err != nil {
			t.Fatalf("AppendBlock error = %v", err)
		}
		got, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		if want := "- [x] A-9 Done\n\tbody\n"; string(got) != want {
			t.Errorf("new file = %q, want %q", got, want)
		}

		if err := AppendBlock(path, "- [x] A-10 Also\n"); err != nil {
			t.Fatalf("AppendBlock error = %v", err)
		}
		if err := AppendBlock(path, "- [x] A-11 Last"); err != nil {
			t.Fatalf("AppendBlock error = %v", err)
		}
		got, err = os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		if want := "- [x] A-9 Done\n\tbody\n- [x] A-10 Also\n- [x] A-11 Last\n"; string(got) != want {
			t.Errorf("appended file = %q, want %q", got, want)
		}
	})
}
