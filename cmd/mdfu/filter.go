package main

import (
	"fmt"
	"io"

	"github.com/elicore/mdfu/internal/output"
)

// runFilter executes filter mode: it runs the shared
// scan→parse→query→rank→output pipeline, prints the results to stdout in the
// requested format, and returns a process exit code (0 with results, 1 when
// empty or on error).
func runFilter(stdout, stderr io.Writer, root string, includeHidden bool, respectGitignore bool, queryStr string, includeArchived bool, limit int, format string) int {
	results, err := runQueryWithOptions(root, includeHidden, respectGitignore, queryStr, includeArchived, limit)
	if err != nil {
		fmt.Fprintln(stderr, "mdfu:", err)
		return 1
	}
	if len(results) == 0 {
		return 1
	}

	var (
		out  string
		ferr error
	)
	switch format {
	case "json":
		out, ferr = output.FormatJSON(results)
		if ferr != nil {
			fmt.Fprintln(stderr, "format json:", ferr)
			return 1
		}
	case "vimgrep":
		out = output.FormatVimgrep(results)
	default:
		out = output.FormatPaths(results)
	}
	fmt.Fprint(stdout, out)
	return 0
}
