package main

import (
	"flag"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/elicore/mdfu/internal/search"
	"github.com/elicore/mdfu/internal/theme"
	"github.com/elicore/mdfu/internal/tui"
)

// version is set at build time via -ldflags "-X main.version=...".
var version = "dev"

// defaultConfigValue is the reserved --config value that prints the builtin
// default configuration instead of loading a theme file.
const defaultConfigValue = "default"

// Result-cap defaults. Filter mode caps lower than the interactive picker
// because its output is meant to be piped into other tools.
const (
	defaultTUILimit    = 50
	defaultFilterLimit = 20
)

func main() {
	os.Exit(run(os.Args[1:], os.Stdout, os.Stderr))
}

// run parses args and dispatches to one of three modes: print the default
// config (--config default), non-interactive filter (a positional query), or
// the interactive TUI (no positional query). It returns a process exit code.
func run(args []string, stdout, stderr io.Writer) int {
	fs := flag.NewFlagSet("mdfu", flag.ContinueOnError)
	fs.SetOutput(stderr)

	root := fs.String("root", ".", "root directory to scan")
	hidden := fs.Bool("hidden", false, "include hidden files and directories")
	noIgnore := fs.Bool("no-ignore", false, "disable gitignore respect")
	limit := fs.Int("limit", defaultTUILimit, "max number of results (filter mode defaults to 20; 0 or negative = unlimited)")
	format := fs.String("format", "paths", "output format: paths|json|vimgrep")
	archived := fs.Bool("archived", false, "include archived documents")
	noHyperlinks := fs.Bool("no-hyperlinks", false, "render markdown links as label and URL instead of OSC 8 terminal hyperlinks")
	config := fs.String("config", "", "path to a TUI theme YAML, or \"default\" to print the builtin default config (default: $XDG_CONFIG_HOME/mdfu/config.yaml)")
	showVersion := fs.Bool("version", false, "print version and exit")

	if err := fs.Parse(splitFlags(fs, args)); err != nil {
		if err == flag.ErrHelp {
			return 0
		}
		return 2
	}

	if *showVersion {
		fmt.Fprintln(stdout, "mdfu", version)
		return 0
	}

	if *format != "paths" && *format != "json" && *format != "vimgrep" {
		fmt.Fprintf(stderr, "invalid --format %q: must be paths|json|vimgrep\n", *format)
		return 2
	}

	if *config == defaultConfigValue {
		fmt.Fprint(stdout, theme.DefaultYAML())
		return 0
	}

	limitSet := false
	fs.Visit(func(f *flag.Flag) {
		if f.Name == "limit" {
			limitSet = true
		}
	})

	query := strings.TrimSpace(strings.Join(fs.Args(), " "))
	if query != "" {
		return runFilter(stdout, stderr, *root, *hidden, !*noIgnore, query, *archived, filterLimit(*limit, limitSet), *format)
	}

	th, err := loadTheme(*config, stderr)
	if err != nil {
		fmt.Fprintln(stderr, "mdfu:", err)
		return 2
	}
	return runTUI(stdout, stderr, *root, *hidden, !*noIgnore, *archived, *limit, *noHyperlinks, th)
}

// splitFlags reorders args so flag tokens precede the positional query, giving
// GNU-style interspersed flags. Without this, Go's flag package stops at the
// first positional argument, so `mdfu <query> --limit 5` would fold the flag
// into the query and silently return nothing. A `--` separator is always
// inserted before the positional section so a query token that starts with a
// dash stays a query token.
func splitFlags(fs *flag.FlagSet, args []string) []string {
	var flags, positional []string
	for i := 0; i < len(args); i++ {
		a := args[i]
		switch {
		case a == "--":
			positional = append(positional, args[i+1:]...)
			return joinSections(flags, positional)
		case a == "" || a == "-" || a[0] != '-':
			positional = append(positional, a)
			continue
		}
		flags = append(flags, a)
		name := strings.TrimLeft(a, "-")
		if eq := strings.IndexByte(name, '='); eq >= 0 {
			name = name[:eq]
		}
		if f := fs.Lookup(name); f != nil && !isBoolFlag(f) && !strings.Contains(a, "=") && i+1 < len(args) {
			i++
			flags = append(flags, args[i])
		}
	}
	return joinSections(flags, positional)
}

func joinSections(flags, positional []string) []string {
	if len(positional) == 0 {
		return flags
	}
	return append(append(flags, "--"), positional...)
}

func isBoolFlag(f *flag.Flag) bool {
	bf, ok := f.Value.(interface{ IsBoolFlag() bool })
	return ok && bf.IsBoolFlag()
}

// filterLimit resolves the effective result cap for filter mode: an explicit
// --limit wins, otherwise the filter-mode default applies. Zero or negative
// means unlimited.
func filterLimit(explicit int, set bool) int {
	if set {
		return explicit
	}
	return defaultFilterLimit
}

func loadTheme(explicit string, warn io.Writer) (*theme.Theme, error) {
	th, err := theme.Resolve(explicit, warn)
	if err != nil {
		return nil, err
	}
	return &th, nil
}

// runTUI loads all documents the same way as filter mode, then launches the
// interactive picker with a live re-parse/re-rank FilterFunc. Selected paths
// are printed one per line on success.
func runTUI(stdout, stderr io.Writer, root string, includeHidden bool, respectGitignore bool, includeArchived bool, limit int, noHyperlinks bool, th *theme.Theme) int {
	docs, err := loadDocuments(root, includeHidden, respectGitignore)
	if err != nil {
		fmt.Fprintln(stderr, "mdfu:", err)
		return 1
	}
	if !includeArchived {
		docs = search.FilterArchived(docs, false)
	}
	items := make([]tui.Item, 0, len(docs))
	for _, d := range docs {
		items = append(items, tui.Item{Doc: d})
	}
	tui.SetFilter(buildFilterFunc())
	selected, err := tui.Run(items, tui.Config{
		Limit:        limit,
		ShowArchived: includeArchived,
		Preview:      true,
		NoHyperlinks: noHyperlinks,
		Theme:        th,
	})
	if err != nil {
		fmt.Fprintln(stderr, "mdfu:", err)
		return 1
	}
	for _, it := range selected {
		if it.Doc != nil {
			fmt.Fprintln(stdout, it.Doc.Path)
		}
	}
	return 0
}
