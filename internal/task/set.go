package task

import (
	"errors"
	"regexp"
	"strings"
)

// runSet implements `mdfu task set <id…> <token…>`: append metadata tokens to
// each named task's header. An existing tag is skipped, a new priority replaces
// the existing one in place, and a metadata-free header receives a literal
// tab-tab separator. Nothing is written to stdout.
func runSet(env Env, args []string) Result {
	scope, rest, err := resolveScope(env, args)
	if err != nil {
		return fail(env, err)
	}

	ids, tokens := splitSetArgs(rest)
	if len(ids) == 0 {
		return fail(env, errors.New("no task IDs provided"))
	}
	if len(tokens) == 0 {
		return fail(env, errors.New("no metadata tokens provided"))
	}

	tasks, _, err := scopeTaskSet(scope)
	if err != nil {
		return fail(env, err)
	}

	type setTarget struct {
		task   Task
		header string
	}
	resolved := make([]setTarget, 0, len(ids))
	for _, id := range ids {
		t, err := Resolve(id, tasks)
		if err != nil {
			return fail(env, err)
		}
		header, _ := setHeaderTokens(t.HeaderRaw, tokens)
		resolved = append(resolved, setTarget{task: t, header: header})
	}

	byFile := map[string][]setTarget{}
	var fileOrder []string
	for _, tg := range resolved {
		if _, seen := byFile[tg.task.File]; !seen {
			fileOrder = append(fileOrder, tg.task.File)
		}
		byFile[tg.task.File] = append(byFile[tg.task.File], tg)
	}

	for _, file := range fileOrder {
		fe, err := Open(file)
		if err != nil {
			return fail(env, err)
		}
		for _, tg := range byFile[file] {
			if tg.header == tg.task.HeaderRaw {
				continue
			}
			if err := fe.Verify(tg.task.ID, tg.task.Line); err != nil {
				return fail(env, err)
			}
			if err := fe.ReplaceHeaderLine(tg.task.Line, tg.header); err != nil {
				return fail(env, err)
			}
		}
		if err := fe.Commit(); err != nil {
			return fail(env, err)
		}
	}
	return Result{Code: 0}
}

// splitSetArgs splits args on commas and whitespace and classifies each piece:
// an ID-shaped token (a full PREFIX-<n> or a bare number) is an ID, everything
// else is a metadata token.
func splitSetArgs(args []string) (ids, tokens []string) {
	for _, arg := range args {
		for _, piece := range strings.FieldsFunc(arg, func(r rune) bool {
			return r == ',' || r == ' ' || r == '\t' || r == '\n' || r == '\r'
		}) {
			if taskIDRe.MatchString(piece) {
				ids = append(ids, piece)
			} else {
				tokens = append(tokens, piece)
			}
		}
	}
	return ids, tokens
}

// setHeaderTokens rewrites raw's metadata portion to include tokens. It returns
// the rewritten header and whether it differs from raw.
func setHeaderTokens(raw string, tokens []string) (string, bool) {
	trimmed := strings.TrimSuffix(raw, "\r")
	m := headerRe.FindStringSubmatch(trimmed)
	if m == nil {
		return raw, false
	}
	title, meta, sep := splitMetaSep(m[3])
	out := append([]string(nil), strings.Fields(meta)...)

	for _, tok := range tokens {
		switch {
		case tagRe.MatchString(tok):
			if containsString(out, tok) {
				continue
			}
			out = append(out, tok)
		case priorityRe.MatchString(tok):
			if idx := firstMatch(out, priorityRe); idx >= 0 {
				out[idx] = tok
			} else {
				out = append(out, tok)
			}
		case propertyRe.MatchString(tok):
			key := propertyKeyOf(tok)
			if idx := firstPropertyMatch(out, key); idx >= 0 {
				out[idx] = tok
			} else {
				out = append(out, tok)
			}
		default:
			if !containsString(out, tok) {
				out = append(out, tok)
			}
		}
	}
	if len(out) == 0 {
		return raw, false
	}
	if sep == "" {
		sep = "\t\t"
	}
	header := "- [" + m[1] + "] " + m[2] + title + sep + strings.Join(out, " ")
	return header, header != trimmed
}

// containsString reports whether list contains want.
func containsString(list []string, want string) bool {
	for _, s := range list {
		if s == want {
			return true
		}
	}
	return false
}

// firstMatch returns the index of the first element matching re, or -1.
func firstMatch(list []string, re *regexp.Regexp) int {
	for i, s := range list {
		if re.MatchString(s) {
			return i
		}
	}
	return -1
}

// propertyKeyOf returns the key of a property token, or "" when it is not one.
func propertyKeyOf(tok string) string {
	m := propertyRe.FindStringSubmatch(tok)
	if m == nil {
		return ""
	}
	return m[1]
}

// firstPropertyMatch returns the index of the first token that is a property
// with the given key, or -1.
func firstPropertyMatch(list []string, key string) int {
	for i, s := range list {
		if propertyKeyOf(s) == key && key != "" {
			return i
		}
	}
	return -1
}

func init() {
	register(Subcommand{Name: "set", Order: 50, Run: runSet})
}
