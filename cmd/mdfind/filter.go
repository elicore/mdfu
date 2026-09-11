package main

import (
	"fmt"
	"os"

	"github.com/anomalyco/mdfind/internal/output"
)

// TODO(track-D): integrate scan+query+search. Future wiring for --filter mode:
//  1. docs, err := scan.WalkMarkdown(root)       // collect *.md paths
//  2. for each path: doc, err := parse.ParseFile(path) // → *model.Document
//  3. q, err := query.Parse(query)               // parse the filter string
//  4. ranked := search.Rank(docs, q)             // hard-filter + fuzzy score
//  5. map ranked docs to []output.Result (with output.Snippet for context)
//  6. format via output.FormatPaths / FormatJSON / FormatVimgrep
// This stub keeps the package compilable standalone until tracks A+B+C land.

// runFilterStub is a placeholder for --filter mode wired for later integration.
// It builds 2 fake results, formats them via the output package, prints to
// stdout and returns a process exit code (0 on success).
func runFilterStub(query, format string) int {
	results := []output.Result{
		{
			Path:    "notes/example.md",
			Score:   100,
			Title:   "Example Note",
			DocType: "Note",
			Snippet: output.Snippet("An example note used as placeholder output for the filter stub.", []string{query}, 80),
		},
		{
			Path:    "notes/other.md",
			Score:   50,
			Title:   "Other Note",
			DocType: "Note",
			Snippet: output.Snippet("Another placeholder document shown until scan+query+search are wired.", []string{query}, 80),
		},
	}

	var (
		out string
		err error
	)
	switch format {
	case "json":
		out, err = output.FormatJSON(results)
		if err != nil {
			fmt.Fprintln(os.Stderr, "format json:", err)
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
