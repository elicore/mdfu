package task

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"
)

// Env is the execution environment a subcommand runs against. Tests inject
// buffers and a fixed Cwd so the command layer never touches the real host.
type Env struct {
	Stdout   io.Writer
	Stderr   io.Writer
	Stdin    io.Reader
	Terminal Terminal
	Cwd      string
	Now      func() time.Time
}

// Result carries the process exit code a subcommand resolves to. See SPEC.md
// §I for the table: 0 success, 1 runtime/task/IO/config error, 2 usage error.
type Result struct {
	Code int
}

// Subcommand is one entry of the `mdfu task` registry.
type Subcommand struct {
	Name    string
	Aliases []string
	// Order is the canonical display order used by Subcommands(). Lower sorts
	// first.
	Order int
	Run   func(Env, []string) Result
}

// registry holds every registered subcommand. Each subcommand registers itself
// from an init() in its own file, so adding a command never edits this file.
var registry []Subcommand

// register appends a subcommand to the package registry. It is called from an
// init() in each subcommand's own file.
func register(sc Subcommand) {
	registry = append(registry, sc)
}

// Subcommands returns the registered subcommands sorted by Order. The returned
// slice is a copy; mutating it does not affect the registry.
func Subcommands() []Subcommand {
	out := make([]Subcommand, len(registry))
	copy(out, registry)
	sort.SliceStable(out, func(i, j int) bool { return out[i].Order < out[j].Order })
	return out
}

// lookupSubcommand finds a subcommand by exact name or alias.
func lookupSubcommand(name string) (Subcommand, bool) {
	for _, sc := range registry {
		if sc.Name == name {
			return sc, true
		}
		for _, alias := range sc.Aliases {
			if alias == name {
				return sc, true
			}
		}
	}
	return Subcommand{}, false
}

// taskIDRe is the router regex for an ID-shaped token (a full PREFIX-<n> or a
// bare numeric part).
var taskIDRe = regexp.MustCompile(`^(?:[A-Z][A-Z0-9]*-\d+|\d+)$`)

// Dispatch routes argv to a registered subcommand and returns its result.
//
//   - empty argv runs list with no arguments;
//   - argv[0] matching a subcommand name or alias runs that subcommand with the
//     remaining arguments;
//   - argv[0] shaped like a task ID runs view with all of argv;
//   - anything else writes E1 plus the help text to stderr and returns code 1.
//
// The unknown-command branch is reachable only through the explicit `mdfu task
// --` path (the root router implements D2 fall-through); it must still exist.
func Dispatch(env Env, argv []string) Result {
	if len(argv) == 0 {
		return runSubcommand(env, "list", nil)
	}
	if sc, ok := lookupSubcommand(argv[0]); ok {
		return sc.Run(env, argv[1:])
	}
	if taskIDRe.MatchString(argv[0]) {
		return runSubcommand(env, "view", argv)
	}
	errf(env, "unknown command '%s'", argv[0])
	fmt.Fprint(env.Stderr, helpText())
	return Result{Code: 1}
}

// runSubcommand resolves name and invokes it, returning code 1 when the name is
// somehow not registered.
func runSubcommand(env Env, name string, args []string) Result {
	sc, ok := lookupSubcommand(name)
	if !ok {
		errf(env, "unknown command '%s'", name)
		return Result{Code: 1}
	}
	return sc.Run(env, args)
}

// errf writes the D1-prefixed message plus a newline to env.Stderr.
func errf(env Env, format string, args ...any) {
	fmt.Fprintf(env.Stderr, "mdfu task: %s\n", fmt.Sprintf(format, args...))
}

// fail reports err on stderr and returns exit code 1. A *StaleError and any
// ordinary error are mapped identically.
func fail(env Env, err error) Result {
	if err != nil {
		errf(env, "%s", err.Error())
	}
	return Result{Code: 1}
}

// Scope is the resolved execution context shared by every subcommand. It is
// returned by resolveScope after the global --path flag and the task
// configuration are applied.
type Scope struct {
	// Base is the resolved base directory of the task scope (per SPEC.md §C
	// precedence). When --path named a single file this holds that file path and
	// IsFile is true.
	Base string
	// Config is the loaded task configuration; when no rc file exists it holds
	// the defaults (ArchivePath "_archive.md").
	Config Config
	// Cwd is the working directory the scope was resolved from; subcommands use
	// it to render cwd-relative paths.
	Cwd string
	// IsFile reports that --path named a single file, which is used as the whole
	// scope without a walk.
	IsFile bool
	// File is the single file named by --path when IsFile is true.
	File string
}

// resolveScope extracts the global --path flag from args, loads the task
// configuration starting at env.Cwd, resolves the base directory, and returns
// the remaining (non-global) args. A --path naming a missing path returns the
// bare E4 message; the caller prefixes it via fail.
func resolveScope(env Env, args []string) (Scope, []string, error) {
	pathFlag, rest := extractPathFlag(args)

	cfg, err := Load(env.Cwd)
	if err != nil {
		return Scope{}, nil, err
	}

	cfgPath, found := FindConfigPath(env.Cwd)
	cfgDir := ""
	if found {
		cfgDir = filepath.Dir(cfgPath)
	}

	base := ResolveBasePath(pathFlag, os.Getenv("MDTASK_PATH"), cfg.Path, cfgDir)
	if !found && base == "." {
		base = BaseDir(env.Cwd)
	}

	scope := Scope{Base: base, Config: cfg, Cwd: env.Cwd}

	if pathFlag != "" {
		info, statErr := os.Stat(pathFlag)
		if statErr != nil {
			return Scope{}, nil, fmt.Errorf("--path '%s' does not exist or is not a file/directory", pathFlag)
		}
		if !info.IsDir() {
			scope.IsFile = true
			scope.File = pathFlag
		}
	}

	return scope, rest, nil
}

// extractPathFlag removes the global --path flag (both `--path X` and
// `--path=X`) from args and returns its value plus the remaining args.
func extractPathFlag(args []string) (string, []string) {
	const prefix = "--path="
	pathFlag := ""
	rest := make([]string, 0, len(args))
	for i := 0; i < len(args); i++ {
		a := args[i]
		switch {
		case a == "--path":
			if i+1 < len(args) {
				pathFlag = args[i+1]
				i++
			}
		case strings.HasPrefix(a, prefix):
			pathFlag = a[len(prefix):]
		default:
			rest = append(rest, a)
		}
	}
	return pathFlag, rest
}

// parseFlags applies ReorderInterspersed to args and parses them with fs. It
// sets fs output to env.Stderr so the flag package's diagnostics reach the user
// stream. The returned *Result is non-nil when parsing should short-circuit:
// code 0 for -h/--help, code 2 for any parse error (SPEC.md §I, §K D4). Callers
// use it as:
//
//	rest, done := parseFlags(env, fs, args)
//	if done != nil {
//		return *done
//	}
func parseFlags(env Env, fs *flag.FlagSet, args []string) ([]string, *Result) {
	fs.SetOutput(env.Stderr)
	if err := fs.Parse(ReorderInterspersed(fs, args)); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil, &Result{Code: 0}
		}
		return nil, &Result{Code: 2}
	}
	return fs.Args(), nil
}

// helpText is the mdfu-branded usage block named by divergence D4. It is
// deterministic and ends with a newline.
func helpText() string {
	return "" +
		"Usage: mdfu task <command> [options] [args]\n" +
		"\n" +
		"Commands:\n" +
		"  list            List tasks\n" +
		"  view, show      Show a single task\n" +
		"  open            Open a task in $EDITOR\n" +
		"  move            Move a task block to another file\n" +
		"  set             Set metadata on tasks\n" +
		"  ids             Assign IDs to tasks without one\n" +
		"  archive         Archive done tasks\n" +
		"  validate        Validate the task scope\n" +
		"  install-skills  Install the mdfu task skills\n" +
		"\n" +
		"Global options:\n" +
		"  --path <dir|file>  Base directory or file for the task scope\n" +
		"\n" +
		"Run 'mdfu task <command> --help' for command-specific help.\n"
}
