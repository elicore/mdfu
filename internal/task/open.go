package task

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"strings"
)

// runOpen implements `mdfu task open <id>`: resolve the task in scope, then
// hand its file and header line to $EDITOR. The child inherits the command's
// stdio through env and its exit code is propagated. Nothing is written to
// stdout by this command itself.
func runOpen(env Env, args []string) Result {
	scope, rest, err := resolveScope(env, args)
	if err != nil {
		return fail(env, err)
	}
	if len(rest) != 1 {
		errf(env, "open requires exactly one task ID")
		return Result{Code: 2}
	}

	tasks, _, err := scopeTaskSet(scope)
	if err != nil {
		return fail(env, err)
	}
	t, err := Resolve(rest[0], tasks)
	if err != nil {
		return fail(env, err)
	}

	editor := os.Getenv("EDITOR")
	if strings.TrimSpace(editor) == "" {
		errf(env, "$EDITOR is not set")
		return Result{Code: 1}
	}
	fields := strings.Fields(editor)
	argv := make([]string, 0, len(fields)+1)
	argv = append(argv, fields[1:]...)
	argv = append(argv, fmt.Sprintf("+%d", t.Line), t.File)

	cmd := exec.Command(fields[0], argv...)
	cmd.Stdin = env.Stdin
	cmd.Stdout = env.Stdout
	cmd.Stderr = env.Stderr
	if err := cmd.Run(); err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) {
			return Result{Code: exitErr.ExitCode()}
		}
		return fail(env, err)
	}
	return Result{Code: 0}
}

// scopeTaskSet loads the identified tasks and the seed/unidentified lines of a
// resolved scope. A --path naming a single file is read directly; otherwise the
// base directory is walked through LoadAll.
func scopeTaskSet(scope Scope) ([]Task, []Unidentified, error) {
	if scope.IsFile {
		return parseOneFileTasks(scope.File)
	}
	return LoadAll(scope.Base, scope.Config)
}

// parseOneFileTasks reads a single task file, honouring fence masking, and
// returns its identified tasks plus its seed and unidentified lines. File is
// recorded relative to the process working directory, matching LoadAll.
func parseOneFileTasks(path string) ([]Task, []Unidentified, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, nil, err
	}
	cwd, _ := os.Getwd()
	file := relToCwd(cwd, path)
	lines := strings.Split(string(data), "\n")
	mask := fenceMask(lines)

	var tasks []Task
	var unidentified []Unidentified
	for i, raw := range lines {
		if mask[i] {
			continue
		}
		parsed, ok := ParseHeader(raw)
		if !ok {
			continue
		}
		if parsed.ID == "" {
			unidentified = append(unidentified, Unidentified{
				File: file,
				Line: i + 1,
				Raw:  strings.TrimSuffix(raw, "\r"),
			})
			continue
		}
		if end, ok := blockRange(lines, i); ok {
			parsed.Body, parsed.BodyIndent = collectBody(lines, i, end)
		}
		parsed.File = file
		parsed.Line = i + 1
		tasks = append(tasks, parsed)
	}
	return tasks, unidentified, nil
}

func init() {
	register(Subcommand{Name: "open", Order: 30, Run: runOpen})
}
