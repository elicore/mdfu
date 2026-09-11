package scan

import (
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// Options controls WalkMarkdown behavior.
type Options struct {
	Root             string
	IncludeHidden    bool
	RespectGitignore bool
	Limit            int
}

// WalkMarkdown walks opts.Root and returns sorted paths of *.md files
// (case-insensitive extension match).
//
// Behavior:
//   - .git directory is always skipped.
//   - Hidden files/dirs (names starting with ".") are skipped unless
//     IncludeHidden is true.
//   - Symlinked dirs are not followed (filepath.WalkDir does not follow
//     symlinks by default); symlinked entries are skipped to avoid loops.
//   - TODO: RespectGitignore is currently accepted but ignored; minimal
//     .gitignore support is out of scope for Track A.
func WalkMarkdown(opts Options) ([]string, error) {
	_ = opts.RespectGitignore // TODO: implement .gitignore support (out of scope for Track A).

	root := opts.Root
	if root == "" {
		root = "."
	}

	var out []string
	err := filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}

		// Skip symlinked dirs (and symlinked files) to avoid loops.
		// WalkDir does not follow symlinks by default, so returning nil
		// here both skips the file and prevents descending.
		if d.Type()&fs.ModeSymlink != 0 {
			if d.IsDir() {
				return filepath.SkipDir
			}
			// Double-check via Lstat for symlinks whose DirEntry reports
			// IsDir()==false (e.g. symlink pointing at a directory):
			// do not descend, do not collect.
			if info, lerr := os.Lstat(path); lerr == nil && info.IsDir() {
				return filepath.SkipDir
			}
			return nil
		}

		name := d.Name()

		if d.IsDir() {
			// Always skip .git, even when hidden files are included.
			if name == ".git" {
				return filepath.SkipDir
			}
			if !opts.IncludeHidden && strings.HasPrefix(name, ".") {
				// Don't skip the root itself if the caller passed a hidden
				// dir (e.g. Root=".hidden"): only skip subdirs below root.
				if path != root {
					return filepath.SkipDir
				}
			}
			return nil
		}

		if !opts.IncludeHidden && strings.HasPrefix(name, ".") {
			return nil
		}
		if !strings.HasSuffix(strings.ToLower(name), ".md") {
			return nil
		}
		out = append(out, path)
		return nil
	})
	if err != nil {
		return nil, err
	}
	sort.Strings(out)
	if opts.Limit > 0 && len(out) > opts.Limit {
		out = out[:opts.Limit]
	}
	return out, nil
}
