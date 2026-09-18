package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/elicore/mdfu/internal/search"
	"github.com/elicore/mdfu/internal/tui"
)

// version is set at build time via -ldflags "-X main.version=...".
var version = "dev"

func main() {
	root := flag.String("root", ".", "root directory to scan")
	hidden := flag.Bool("hidden", false, "include hidden files and directories")
	noIgnore := flag.Bool("no-ignore", false, "disable gitignore respect")
	limit := flag.Int("limit", 50, "max number of results")
	filter := flag.String("filter", "", "non-interactive filter query")
	format := flag.String("format", "paths", "output format: paths|json|vimgrep")
	archived := flag.Bool("archived", false, "include archived documents")
	noHyperlinks := flag.Bool("no-hyperlinks", false, "render markdown links as label and URL instead of OSC 8 terminal hyperlinks")
	showVersion := flag.Bool("version", false, "print version and exit")
	flag.Parse()

	if *showVersion {
		fmt.Println("mdfu", version)
		return
	}

	if *format != "paths" && *format != "json" && *format != "vimgrep" {
		fmt.Fprintf(os.Stderr, "invalid --format %q: must be paths|json|vimgrep\n", *format)
		os.Exit(2)
	}

	if *filter != "" {
		os.Exit(runFilter(*root, *hidden, !*noIgnore, *filter, *archived, *limit, *format))
	}
	os.Exit(runTUI(*root, *hidden, !*noIgnore, *archived, *limit, *noHyperlinks))
}

// runTUI loads all documents the same way as --filter mode, then launches
// the interactive picker with a live re-parse/re-rank FilterFunc. Selected
// paths are printed one per line on success.
func runTUI(root string, includeHidden bool, respectGitignore bool, includeArchived bool, limit int, noHyperlinks bool) int {
	docs, err := loadDocuments(root, includeHidden, respectGitignore)
	if err != nil {
		fmt.Fprintln(os.Stderr, "mdfu:", err)
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
	})
	if err != nil {
		fmt.Fprintln(os.Stderr, "mdfu:", err)
		return 1
	}
	for _, it := range selected {
		if it.Doc != nil {
			fmt.Println(it.Doc.Path)
		}
	}
	return 0
}
