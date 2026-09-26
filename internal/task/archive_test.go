package task

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// archiveSetup creates an Env over a fresh temp vault, makes it the working
// directory, and clears the environment that affects scope resolution.
func archiveSetup(t *testing.T) (Env, *bytes.Buffer, *bytes.Buffer, string) {
	t.Helper()
	env, stdout, stderr, dir := commandEnv(t, Terminal{})
	t.Chdir(dir)
	t.Setenv("MDTASK_PATH", "")
	t.Setenv("MDTASK_EXCLUDE_DIRS", "")
	return env, stdout, stderr, dir
}

// readTaskFile reads a file and fails the test on error.
func readTaskFile(t *testing.T, path string) string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return string(data)
}

func TestArchive(t *testing.T) {
	t.Run("no ids archives every done task", func(t *testing.T) {
		env, stdout, stderr, dir := archiveSetup(t)
		mustWrite(t, dir, "a.md",
			"- [x] A-1 Done one\n  body of one\n- [ ] A-2 Open two\n- [x] A-3 Done three\n")

		res := Dispatch(env, []string{"archive"})
		if res.Code != 0 {
			t.Fatalf("archive code = %d, want 0 (stderr=%q)", res.Code, stderr.String())
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
		if stderr.Len() != 0 {
			t.Errorf("stderr = %q, want empty", stderr.String())
		}

		if got, want := readTaskFile(t, filepath.Join(dir, "a.md")), "- [ ] A-2 Open two\n"; got != want {
			t.Errorf("source = %q, want %q", got, want)
		}
		wantArchive := "- [x] A-1 Done one\n  body of one\n\n- [x] A-3 Done three\n"
		if got := readTaskFile(t, filepath.Join(dir, "_archive.md")); got != wantArchive {
			t.Errorf("archive = %q, want %q", got, wantArchive)
		}
	})

	t.Run("both files are correct after a successful archive", func(t *testing.T) {
		env, _, stderr, dir := archiveSetup(t)
		mustWrite(t, dir, "a.md", "- [x] A-1 Done A\n- [ ] A-2 Open A\n")
		mustWrite(t, dir, "b/b.md", "- [x] B-1 Done B\n- [ ] B-2 Open B\n")

		res := Dispatch(env, []string{"archive"})
		if res.Code != 0 {
			t.Fatalf("archive code = %d, want 0 (stderr=%q)", res.Code, stderr.String())
		}
		if got, want := readTaskFile(t, filepath.Join(dir, "a.md")), "- [ ] A-2 Open A\n"; got != want {
			t.Errorf("a.md = %q, want %q", got, want)
		}
		if got, want := readTaskFile(t, filepath.Join(dir, "b/b.md")), "- [ ] B-2 Open B\n"; got != want {
			t.Errorf("b/b.md = %q, want %q", got, want)
		}
		wantArchive := "- [x] A-1 Done A\n\n- [x] B-1 Done B\n"
		if got := readTaskFile(t, filepath.Join(dir, "_archive.md")); got != wantArchive {
			t.Errorf("archive = %q, want %q", got, wantArchive)
		}
	})

	t.Run("a named open task is not done", func(t *testing.T) {
		env, stdout, stderr, dir := archiveSetup(t)
		mustWrite(t, dir, "a.md", "- [ ] A-1 Open\n- [x] A-2 Done\n")

		res := Dispatch(env, []string{"archive", "A-1"})
		if res.Code != 1 {
			t.Fatalf("archive code = %d, want 1", res.Code)
		}
		if !strings.Contains(stderr.String(), "task 'A-1' is not done") {
			t.Errorf("stderr = %q, want %q", stderr.String(), "task 'A-1' is not done")
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
		if got, want := readTaskFile(t, filepath.Join(dir, "a.md")), "- [ ] A-1 Open\n- [x] A-2 Done\n"; got != want {
			t.Errorf("source mutated: %q, want %q", got, want)
		}
		if _, err := os.Stat(filepath.Join(dir, "_archive.md")); !os.IsNotExist(err) {
			t.Errorf("archive created on failure: err=%v", err)
		}
	})

	t.Run("an escaping archive path errors", func(t *testing.T) {
		env, stdout, stderr, dir := archiveSetup(t)
		mustWrite(t, dir, ".mdfurc", `{"archivePath":"../escaped-archive.md"}`)
		mustWrite(t, dir, "a.md", "- [x] A-1 Done\n")

		res := Dispatch(env, []string{"archive"})
		if res.Code != 1 {
			t.Fatalf("archive code = %d, want 1 (stderr=%q)", res.Code, stderr.String())
		}
		if !strings.Contains(stderr.String(), "is outside the base directory") {
			t.Errorf("stderr = %q, want outside-the-base error", stderr.String())
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
		if got, want := readTaskFile(t, filepath.Join(dir, "a.md")), "- [x] A-1 Done\n"; got != want {
			t.Errorf("source mutated: %q, want %q", got, want)
		}
		if _, err := os.Stat(filepath.Join(dir, "..", "escaped-archive.md")); !os.IsNotExist(err) {
			t.Errorf("escaped archive created: err=%v", err)
		}
	})

	t.Run("an empty scope is a silent exit 0", func(t *testing.T) {
		env, stdout, stderr, dir := archiveSetup(t)

		res := Dispatch(env, []string{"archive"})
		if res.Code != 0 {
			t.Fatalf("archive code = %d, want 0", res.Code)
		}
		if stdout.Len() != 0 || stderr.Len() != 0 {
			t.Errorf("stdout=%q stderr=%q, want both empty", stdout.String(), stderr.String())
		}
		if _, err := os.Stat(filepath.Join(dir, "_archive.md")); !os.IsNotExist(err) {
			t.Errorf("archive created for an empty scope: err=%v", err)
		}
	})

	t.Run("archive is written before any source is rewritten", func(t *testing.T) {
		if os.Geteuid() == 0 {
			t.Skip("running as root; permission bits are not enforced")
		}
		env, _, stderr, dir := archiveSetup(t)
		mustWrite(t, dir, "src/tasks.md", "- [x] A-1 Done\n")
		srcDir := filepath.Join(dir, "src")
		if err := os.Chmod(srcDir, 0o555); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { os.Chmod(srcDir, 0o755) })

		res := Dispatch(env, []string{"archive"})
		if res.Code != 1 {
			t.Fatalf("archive code = %d, want 1 (stderr=%q)", res.Code, stderr.String())
		}
		if !strings.Contains(stderr.String(), "cannot write to") {
			t.Errorf("stderr = %q, want cannot-write error", stderr.String())
		}
		archive := readTaskFile(t, filepath.Join(dir, "_archive.md"))
		if !strings.Contains(archive, "- [x] A-1 Done") {
			t.Errorf("archive = %q, want the block written before the failed source rewrite", archive)
		}
		source := readTaskFile(t, filepath.Join(srcDir, "tasks.md"))
		if !strings.Contains(source, "- [x] A-1 Done") {
			t.Errorf("source = %q, want it left untouched after the failed rewrite", source)
		}
	})
}
