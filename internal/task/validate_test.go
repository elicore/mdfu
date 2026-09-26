package task

import (
	"bytes"
	"testing"
)

// validateSetup creates an Env over a fresh temp vault and makes it the working
// directory so Task.File values are short, cwd-relative names.
func validateSetup(t *testing.T) (Env, *bytes.Buffer, *bytes.Buffer, string) {
	t.Helper()
	env, stdout, stderr, dir := commandEnv(t, Terminal{})
	t.Chdir(dir)
	t.Setenv("MDTASK_PATH", "")
	t.Setenv("MDTASK_EXCLUDE_DIRS", "")
	return env, stdout, stderr, dir
}

func TestValidate(t *testing.T) {
	t.Run("duplicate ID yields exit 1 and one error line", func(t *testing.T) {
		env, stdout, stderr, dir := validateSetup(t)
		mustWrite(t, dir, "a.md", "- [ ] A-1 One\n- [x] A-1 Two\n")

		res := Dispatch(env, []string{"validate"})
		if res.Code != 1 {
			t.Fatalf("validate code = %d, want 1", res.Code)
		}
		if want := "error: duplicate ID 'A-1' in a.md:1, a.md:2\n"; stderr.String() != want {
			t.Errorf("stderr = %q, want %q", stderr.String(), want)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
	})

	warnings := []struct {
		name    string
		content string
		want    string
	}{
		{
			name:    "duplicate numeric part",
			content: "- [ ] A-1 One\n- [ ] B-1 Two\n",
			want:    "warning: duplicate numeric part 1 across prefixes: A-1, B-1\n",
		},
		{
			name:    "empty tag",
			content: "- [ ] A-1 Title\t\t#\n",
			want:    "warning: empty tag in a.md:1\n",
		},
		{
			name:    "malformed metadata",
			content: "- [ ] A-1 Title\t\t@key\n",
			want:    "warning: malformed metadata '@key' in a.md:1\n",
		},
		{
			name:    "unknown priority",
			content: "- [ ] A-1 Title\t\t!urgent\n",
			want:    "warning: unknown priority '!urgent' in a.md:1\n",
		},
	}
	for _, tt := range warnings {
		t.Run(tt.name, func(t *testing.T) {
			env, stdout, stderr, dir := validateSetup(t)
			mustWrite(t, dir, "a.md", tt.content)

			res := Dispatch(env, []string{"validate"})
			if res.Code != 0 {
				t.Fatalf("validate code = %d, want 0 (stderr=%q)", res.Code, stderr.String())
			}
			if stderr.String() != tt.want {
				t.Errorf("stderr = %q, want %q", stderr.String(), tt.want)
			}
			if stdout.Len() != 0 {
				t.Errorf("stdout = %q, want empty", stdout.String())
			}
		})
	}

	t.Run("excluded prefixes are not reported", func(t *testing.T) {
		env, stdout, stderr, dir := validateSetup(t)
		mustWrite(t, dir, ".mdfurc", `{"excludePrefixes":["ARCH"]}`)
		mustWrite(t, dir, "a.md",
			"- [ ] ARCH-1 One\n- [x] ARCH-1 Two\n- [ ] EXMPL-1 One\n- [x] EXMPL-1 Two\n")

		res := Dispatch(env, []string{"validate"})
		if res.Code != 1 {
			t.Fatalf("validate code = %d, want 1", res.Code)
		}
		if want := "error: duplicate ID 'EXMPL-1' in a.md:3, a.md:4\n"; stderr.String() != want {
			t.Errorf("stderr = %q, want %q", stderr.String(), want)
		}
		if got := stderr.String(); len(got) > 0 && bytes.Contains([]byte(got), []byte("ARCH")) {
			t.Errorf("stderr = %q, must not mention the excluded prefix", got)
		}
		if stdout.Len() != 0 {
			t.Errorf("stdout = %q, want empty", stdout.String())
		}
	})
}
