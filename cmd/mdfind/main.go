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
	format := flag.String("format", "paths", "output format: paths|json")
	flag.Parse()

	if *format != "paths" && *format != "json" {
		fmt.Fprintf(os.Stderr, "invalid --format %q: must be paths|json\n", *format)
		os.Exit(2)
	}

	// Suppress unused warnings until Track D wires real logic.
	_, _, _ = *root, *hidden, *noIgnore
	_ = *limit

	if *filter != "" {
		fmt.Printf("filter mode stub: %s\n", *filter)
		return
	}
	fmt.Println("tui mode stub")
}
