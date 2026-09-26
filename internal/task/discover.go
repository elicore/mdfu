package task

import (
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// maxWalkDepth bounds task discovery recursion, matching SPEC.md §E.
const maxWalkDepth = 64

// DiscoverOptions controls Walk.
//
// Warn is an addition to the plan's two-field shape: the walker must be able to
// emit the single-line skip warning without writing to os.Stderr directly, so
// tests can capture it. A nil Warn discards the warning.
type DiscoverOptions struct {
	ExcludeDirs    []string
	FollowSymlinks bool
	Warn           io.Writer
}

// Walk returns the sorted, realpath-deduplicated markdown files under base.
//
// It includes hidden files and directories, ignores .gitignore entirely, always
// excludes node_modules, .git, and each directory named in ExcludeDirs, and
// accepts only names ending in .md (case-insensitive). When FollowSymlinks is
// true symlinks are followed through an EvalSymlinks visited-set, ELOOP is
// tolerated, and recursion is capped at maxWalkDepth. An unreadable directory
// is skipped with exactly one line on opts.Warn and does not fail the walk.
func Walk(base string, opts DiscoverOptions) ([]string, error) {
	info, err := os.Stat(base)
	if err != nil {
		return nil, err
	}
	if !info.IsDir() {
		return nil, fmt.Errorf("cannot walk %s: not a directory", base)
	}

	warn := opts.Warn
	if warn == nil {
		warn = io.Discard
	}

	w := &walker{
		follow:    opts.FollowSymlinks,
		warn:      warn,
		exclude:   map[string]bool{"node_modules": true, ".git": true},
		seenDirs:  map[string]bool{},
		seenFiles: map[string]bool{},
	}
	for _, dir := range opts.ExcludeDirs {
		if dir != "" {
			w.exclude[dir] = true
		}
	}
	w.walk(base, 0)
	sort.Strings(w.out)
	return w.out, nil
}

// walker carries the visited-sets and exclusions for one Walk call.
type walker struct {
	follow    bool
	warn      io.Writer
	exclude   map[string]bool
	seenDirs  map[string]bool
	seenFiles map[string]bool
	out       []string
}

// walk descends into dir. It marks the resolved directory realpath so a symlink
// cycle is traversed at most once.
func (w *walker) walk(dir string, depth int) {
	if depth > maxWalkDepth {
		return
	}
	real, err := filepath.EvalSymlinks(dir)
	if err != nil {
		w.skip(dir, err)
		return
	}
	if w.seenDirs[real] {
		return
	}
	w.seenDirs[real] = true

	entries, err := os.ReadDir(dir)
	if err != nil {
		w.skip(dir, err)
		return
	}
	for _, entry := range entries {
		name := entry.Name()
		path := filepath.Join(dir, name)

		if entry.IsDir() {
			if w.exclude[name] {
				continue
			}
			w.walk(path, depth+1)
			continue
		}
		// A symlink has IsDir() == false on the DirEntry itself.
		if !w.follow && entry.Type()&fs.ModeSymlink != 0 {
			continue
		}

		info, err := os.Stat(path)
		if err != nil {
			// Broken symlink or ELOOP: tolerated, not a warning.
			continue
		}
		if info.IsDir() {
			if w.exclude[name] {
				continue
			}
			w.walk(path, depth+1)
			continue
		}
		if !isMarkdownName(name) {
			continue
		}
		realFile, err := filepath.EvalSymlinks(path)
		if err != nil {
			continue
		}
		if w.seenFiles[realFile] {
			continue
		}
		w.seenFiles[realFile] = true
		w.out = append(w.out, path)
	}
}

// skip emits the single-line unreadable-directory warning.
func (w *walker) skip(dir string, err error) {
	fmt.Fprintf(w.warn, "mdfu task: skipping %s: %v\n", dir, err)
}

// isMarkdownName reports whether name ends in .md, case-insensitively.
func isMarkdownName(name string) bool {
	return strings.HasSuffix(strings.ToLower(name), ".md")
}

// Filter applies include then exclude globs to each path's slash-separated
// base-relative form and preserves input order. An empty include list means
// every path (which Walk has already restricted to .md files).
func Filter(paths []string, base string, inc, exc []string) []string {
	out := make([]string, 0, len(paths))
	for _, path := range paths {
		rel := relativeSlash(base, path)
		if len(inc) > 0 && !matchesAny(inc, rel) {
			continue
		}
		if len(exc) > 0 && matchesAny(exc, rel) {
			continue
		}
		out = append(out, path)
	}
	return out
}

// matchesAny reports whether name matches any of globs.
func matchesAny(globs []string, name string) bool {
	for _, glob := range globs {
		if Match(glob, name) {
			return true
		}
	}
	return false
}

// relativeSlash returns path relative to base with forward slashes.
func relativeSlash(base, path string) string {
	rel, err := filepath.Rel(base, path)
	if err != nil {
		return filepath.ToSlash(path)
	}
	return filepath.ToSlash(rel)
}

// LoadAll discovers, filters, and parses every task in the scope rooted at base.
//
// tasks contains the identified tasks in file order then line order; seed and
// unidentified lines are collected separately into unidentified (a seed's
// prefix is recoverable through the exported SeedPrefix helper). Body and
// BodyIndent are filled from blockRange and collectBody. File is the
// slash-separated path relative to the process working directory and Line is the
// 1-based header line. The resolved cfg.ArchivePath is excluded from discovery.
func LoadAll(base string, cfg Config) ([]Task, []Unidentified, error) {
	paths, err := Walk(base, DiscoverOptions{
		FollowSymlinks: true,
		ExcludeDirs:    excludeDirsFromEnv(),
	})
	if err != nil {
		return nil, nil, err
	}
	paths = Filter(paths, base, cfg.Include, cfg.Exclude)

	archive := resolveArchive(base, cfg.ArchivePath)
	cwd, _ := os.Getwd()

	var tasks []Task
	var unidentified []Unidentified
	for _, path := range paths {
		if archive != "" && samePath(path, archive) {
			continue
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return nil, nil, fmt.Errorf("cannot read %s: %w", path, err)
		}
		lines := strings.Split(string(data), "\n")
		mask := fenceMask(lines)
		for i, raw := range lines {
			if mask[i] {
				continue
			}
			parsed, ok := ParseHeader(raw)
			if !ok {
				continue
			}
			rel := relToCwd(cwd, path)
			if parsed.ID == "" {
				unidentified = append(unidentified, Unidentified{
					File: rel,
					Line: i + 1,
					Raw:  strings.TrimSuffix(raw, "\r"),
				})
				continue
			}
			if end, ok := blockRange(lines, i); ok {
				parsed.Body, parsed.BodyIndent = collectBody(lines, i, end)
			}
			parsed.File = rel
			parsed.Line = i + 1
			tasks = append(tasks, parsed)
		}
	}
	return tasks, unidentified, nil
}

// excludeDirsFromEnv splits $MDTASK_EXCLUDE_DIRS on colons and commas.
func excludeDirsFromEnv() []string {
	raw := os.Getenv("MDTASK_EXCLUDE_DIRS")
	if raw == "" {
		return nil
	}
	return strings.FieldsFunc(raw, func(r rune) bool { return r == ':' || r == ',' })
}

// resolveArchive resolves cfg.ArchivePath against base, defaulting it when
// empty.
func resolveArchive(base, archivePath string) string {
	if archivePath == "" {
		archivePath = defaultArchivePath
	}
	if filepath.IsAbs(archivePath) {
		return filepath.Clean(archivePath)
	}
	return filepath.Clean(filepath.Join(base, archivePath))
}

// samePath reports whether two paths name the same location for exclusion
// purposes. It compares cleaned absolute paths, which suffices even when the
// archive file does not exist yet.
func samePath(a, b string) bool {
	aa, errA := filepath.Abs(a)
	bb, errB := filepath.Abs(b)
	if errA != nil || errB != nil {
		return filepath.Clean(a) == filepath.Clean(b)
	}
	return filepath.Clean(aa) == filepath.Clean(bb)
}

// relToCwd returns path relative to cwd with forward slashes, falling back to
// path itself when Rel fails.
func relToCwd(cwd, path string) string {
	if cwd != "" {
		if rel, err := filepath.Rel(cwd, path); err == nil {
			return filepath.ToSlash(rel)
		}
	}
	return filepath.ToSlash(path)
}
