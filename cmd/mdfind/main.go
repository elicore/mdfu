package main

import (
	"flag"
	"fmt"
	"os"
)

// NOTE(track-A): this file is owned by Track A (scaffold+scan). Minimal
// placeholder so `go build ./...` stays green until the real cmd skeleton
// with full flag parsing lands — Track A will replace it.

func main() {
	query := flag.String("filter", "", "non-interactive filter query")
	format := flag.String("format", "paths", "output format: paths|json|vimgrep")
	flag.Parse()
	if *query == "" {
		fmt.Fprintln(os.Stderr, "usage: mdfind --filter QUERY [--format paths|json|vimgrep]")
		os.Exit(2)
	}
	os.Exit(runFilterStub(*query, *format))
}
