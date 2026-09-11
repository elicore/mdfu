package main

import (
	"fmt"
	"os"

	"github.com/anomalyco/mdfind/internal/output"
)

// runFilter executes --filter mode: it runs the shared
// scan→parse→query→rank→output pipeline, prints the results to stdout in the
// requested format, and returns a process exit code (0 with results, 1 when
// empty or on error).
func runFilter(root string, includeHidden bool, respectGitignore bool, queryStr string, includeArchived bool, limit int, format string) int {
	results, err := runQueryWithOptions(root, includeHidden, respectGitignore, queryStr, includeArchived, limit)
	if err != nil {
		fmt.Fprintln(os.Stderr, "mdfind:", err)
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
			fmt.Fprintln(os.Stderr, "format json:", ferr)
			return 1
		}
	case "vimgrep":
		out = output.FormatVimgrep(results)
	default:
		out = output.FormatPaths(results)
	}
	fmt.Print(out)
	return 0
}
