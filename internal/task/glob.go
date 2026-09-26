package task

import "strings"

// Match reports whether name matches pattern under the ** glob subset defined by
// SPEC.md §D. A third-party doublestar library (for example
// github.com/bmatcuk/doublestar) was considered and rejected so that the direct
// dependency set stays at the existing six modules; the matcher is hand-rolled
// here.
//
// Exactly four rules apply:
//
//  1. ** matches zero or more path segments. **/*.md matches a.md, x/a.md, and
//     x/y/z.md.
//  2. * matches any run of characters other than the path separator /. *.md
//     matches a.md but not x/a.md.
//  3. ? and [...] are literal characters; there is no single-character wildcard
//     and no character class.
//  4. A pattern ending in /** requires at least one following segment:
//     archive/** matches archive/a.md but not archive itself.
//
// A pattern that contains no separator matches at any depth only through the
// ** rule; there is no implicit prefix matching.
func Match(pattern, name string) bool {
	pat := strings.Split(pattern, "/")
	seg := strings.Split(name, "/")
	// Rule 4: only a terminal ** that follows at least one other segment is
	// forced to consume a segment. A bare "**" still matches zero or more.
	forceTrailing := len(pat) > 1 && pat[len(pat)-1] == "**"
	return matchSegments(pat, seg, forceTrailing)
}

// matchSegments matches the remaining pattern segments against the remaining
// name segments. A "**" segment consumes zero or more name segments via
// backtracking.
func matchSegments(pat, name []string, forceTrailing bool) bool {
	if len(pat) == 0 {
		return len(name) == 0
	}
	if pat[0] == "**" {
		rest := pat[1:]
		if len(rest) == 0 && forceTrailing {
			return len(name) >= 1
		}
		for k := 0; k <= len(name); k++ {
			if matchSegments(rest, name[k:], forceTrailing) {
				return true
			}
		}
		return false
	}
	if len(name) == 0 {
		return false
	}
	if !matchSegment(pat[0], name[0]) {
		return false
	}
	return matchSegments(pat[1:], name[1:], forceTrailing)
}

// matchSegment matches a single pattern segment against a single name segment.
// The only wildcard is *, which matches any run of bytes; ? and bracket
// characters are literal.
func matchSegment(pattern, name string) bool {
	pi, si := 0, 0
	star, match := -1, 0
	for si < len(name) {
		switch {
		case pi < len(pattern) && pattern[pi] != '*' && pattern[pi] == name[si]:
			pi++
			si++
		case pi < len(pattern) && pattern[pi] == '*':
			star = pi
			match = si
			pi++
		case star != -1:
			pi = star + 1
			match++
			si = match
		default:
			return false
		}
	}
	for pi < len(pattern) && pattern[pi] == '*' {
		pi++
	}
	return pi == len(pattern)
}
