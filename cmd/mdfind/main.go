package main

import (
	"flag"
	"fmt"
	"os"
)

func main() {
	root := flag.String("root", ".", "root directory to scan")
	hidden := flag.Bool("hidden", false, "include hidden files and directories")
	noIgnore := flag.Bool("no-ignore", false, "disable gitignore respect")
	limit := flag.Int("limit", 50, "max number of results")
	filter := flag.String("filter", "", "non-interactive filter query")
	format := flag.String("format", "paths", "output format: paths|json|vimgrep")
	flag.Parse()

	if *format != "paths" && *format != "json" && *format != "vimgrep" {
		fmt.Fprintf(os.Stderr, "invalid --format %q: must be paths|json|vimgrep\n", *format)
		os.Exit(2)
	}

	// TODO: wire scan+parse+query+search+TUI; root/hidden/noIgnore/limit
	// are parsed here and will be threaded through once tracks integrate.
	_, _, _ = *root, *hidden, *noIgnore
	_ = *limit

	if *filter != "" {
		os.Exit(runFilterStub(*filter, *format))
	}
	fmt.Println("tui mode stub")
}
