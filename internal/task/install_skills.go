package task

import (
	"embed"
	"errors"
	"flag"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
)

// skillNames lists the embedded skill directories in install order.
var skillNames = []string{"mdfu-task", "mdfu-task-add", "mdfu-task-do"}

//go:embed skills
var skillsFS embed.FS

// runInstallSkills implements `mdfu task install-skills <dir>` per SPEC.md §F.9.
//
// This subcommand is documented divergence D3: it is present for CLI-surface
// completeness, not for byte parity with the original tool. It writes the three
// embedded skill documents to <dir>/<name>/SKILL.md, creating directories as
// needed and overwriting any existing file, then prints one line per skill in
// the form `Installed <name> into <dir>` to stdout. Files are written, never
// symlinked, so an embedded asset needs no on-disk source path and Windows
// needs no symlink privilege. A missing <dir> argument is a usage error (exit
// 2); a write failure is `cannot write to '<path>': permission denied` (exit 1).
// No pre-existing file other than the three SKILL.md paths is touched.
func runInstallSkills(env Env, args []string) Result {
	flags := flag.NewFlagSet("install-skills", flag.ContinueOnError)
	rest, done := parseFlags(env, flags, args)
	if done != nil {
		return *done
	}
	if len(rest) == 0 {
		errf(env, "missing directory argument")
		return Result{Code: 2}
	}
	if len(rest) > 1 {
		errf(env, "unexpected argument '%s'", rest[1])
		return Result{Code: 2}
	}
	dir := rest[0]

	for _, name := range skillNames {
		data, err := skillsFS.ReadFile("skills/" + name + "/SKILL.md")
		if err != nil {
			return fail(env, err)
		}
		target := filepath.Join(dir, name, "SKILL.md")
		if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
			return fail(env, skillWriteError(target, err))
		}
		if err := os.WriteFile(target, data, 0o644); err != nil {
			return fail(env, skillWriteError(target, err))
		}
		fmt.Fprintf(env.Stdout, "Installed %s into %s\n", name, dir)
	}
	return Result{Code: 0}
}

// skillWriteError maps a filesystem failure to the SPEC §F.9 message, keeping
// the exact permission-denied spelling.
func skillWriteError(path string, err error) error {
	if errors.Is(err, fs.ErrPermission) {
		return fmt.Errorf("cannot write to '%s': permission denied", path)
	}
	return fmt.Errorf("cannot write to '%s': %v", path, err)
}

func init() {
	register(Subcommand{Name: "install-skills", Order: 90, Run: runInstallSkills})
}
