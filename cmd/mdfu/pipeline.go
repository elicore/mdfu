package main

import (
	"strings"

	"github.com/anomalyco/mdfu/internal/model"
	"github.com/anomalyco/mdfu/internal/output"
	"github.com/anomalyco/mdfu/internal/parse"
	"github.com/anomalyco/mdfu/internal/query"
	"github.com/anomalyco/mdfu/internal/scan"
	"github.com/anomalyco/mdfu/internal/search"
	"github.com/anomalyco/mdfu/internal/tui"
)

// loadDocuments walks root for markdown files and parses each one.
// Documents are kept even when frontmatter parsing fails (ParseError is set
// on the document); only files that fail I/O are skipped.
func loadDocuments(root string, includeHidden bool, respectGitignore bool) ([]*model.Document, error) {
	paths, err := scan.WalkMarkdown(scan.Options{
		Root:             root,
		IncludeHidden:    includeHidden,
		RespectGitignore: respectGitignore,
		Limit:            0, // never truncate discovery; --limit applies to final results
	})
	if err != nil {
		return nil, err
	}
	docs := make([]*model.Document, 0, len(paths))
	for _, p := range paths {
		doc, err := parse.ParseFile(p)
		if err != nil {
			continue // I/O error: skip
		}
		if doc == nil {
			continue
		}
		docs = append(docs, doc)
	}
	return docs, nil
}

// runQueryWithOptions is the shared scan→parse→query→rank→results pipeline
// used by both --filter and TUI modes.
func runQueryWithOptions(root string, includeHidden bool, respectGitignore bool, queryStr string, includeArchived bool, limit int) ([]output.Result, error) {
	docs, err := loadDocuments(root, includeHidden, respectGitignore)
	if err != nil {
		return nil, err
	}
	q, err := query.Parse(queryStr)
	if err != nil {
		return nil, err
	}
	if !includeArchived {
		docs = search.FilterArchived(docs, false)
	}
	ranked := search.Rank(docs, q)
	results := make([]output.Result, 0, len(ranked))
	for _, d := range ranked {
		results = append(results, output.Result{
			Path:    d.Path,
			Score:   0, // Rank returns ordered docs; order is the score
			Title:   d.Title,
			DocType: d.DocType,
			Snippet: output.Snippet(d.Body, q.Bare, 120),
		})
	}
	if limit > 0 && len(results) > limit {
		results = results[:limit]
	}
	return results, nil
}

// runQuery runs the shared pipeline with gitignore respected (the default).
// It is the testable helper used by both --filter and TUI paths.
func runQuery(root string, includeHidden bool, queryStr string, includeArchived bool, limit int) ([]output.Result, error) {
	return runQueryWithOptions(root, includeHidden, true, queryStr, includeArchived, limit)
}

// buildFilterFunc returns a tui.FilterFunc that re-parses the query string
// and re-ranks on each keystroke. Empty queries and parse errors return all
// items; archived visibility is left to the TUI's ShowArchived handling.
func buildFilterFunc() tui.FilterFunc {
	return func(queryStr string, items []tui.Item) []tui.Item {
		if strings.TrimSpace(queryStr) == "" {
			out := make([]tui.Item, len(items))
			copy(out, items)
			return out
		}
		q, err := query.Parse(queryStr)
		if err != nil {
			out := make([]tui.Item, len(items))
			copy(out, items)
			return out
		}
		docs := make([]*model.Document, 0, len(items))
		for _, it := range items {
			if it.Doc != nil {
				docs = append(docs, it.Doc)
			}
		}
		ranked := search.Rank(docs, q)
		byDoc := make(map[*model.Document]tui.Item, len(items))
		for _, it := range items {
			if it.Doc != nil {
				if _, ok := byDoc[it.Doc]; !ok {
					byDoc[it.Doc] = it
				}
			}
		}
		out := make([]tui.Item, 0, len(ranked))
		for _, d := range ranked {
			if it, ok := byDoc[d]; ok {
				it.Score = 0
				out = append(out, it)
			} else {
				out = append(out, tui.Item{Doc: d})
			}
		}
		return out
	}
}
