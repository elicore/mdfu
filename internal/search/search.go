package search

import (
	"fmt"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/elicore/mdfu/internal/model"
	"github.com/elicore/mdfu/internal/query"
	"github.com/sahilm/fuzzy"
)

// Title match scoring. An exact-title hit beats a whole-word hit, which in
// turn beats a substring buried inside a longer title word. This keeps a
// search for "butter" from tying with titles like "Buttermilk"/"Butternut".
const (
	exactTitleBoost   = 200 // title is exactly the bare word
	titleBoost        = 100 // bare word appears as a whole word in the title
	partialTitleBoost = 50  // bare word appears inside a longer title word
)

// MatchesDoc reports whether d satisfies all hard filters in q.
// Bare words are NOT checked here; they only affect Rank scoring.
func MatchesDoc(d *model.Document, q *query.Query) bool {
	if d == nil {
		return false
	}
	if q == nil {
		return true
	}
	// Tags subset (case-insensitive).
	if len(q.Tags) > 0 {
		set := make(map[string]struct{}, len(d.Tags))
		for _, t := range d.Tags {
			set[strings.ToLower(strings.TrimSpace(t))] = struct{}{}
		}
		for _, want := range q.Tags {
			if _, ok := set[strings.ToLower(strings.TrimSpace(want))]; !ok {
				return false
			}
		}
	}
	// NotTags exclusion.
	if len(q.NotTags) > 0 {
		set := make(map[string]struct{}, len(d.Tags))
		for _, t := range d.Tags {
			set[strings.ToLower(strings.TrimSpace(t))] = struct{}{}
		}
		for _, ban := range q.NotTags {
			if _, ok := set[strings.ToLower(strings.TrimSpace(ban))]; ok {
				return false
			}
		}
	}
	// DocType exact (fold).
	if q.DocType != "" {
		if !strings.EqualFold(strings.TrimSpace(d.DocType), strings.TrimSpace(q.DocType)) {
			return false
		}
	}
	// Title fuzzy-contains-fold.
	if q.Title != "" {
		if !fuzzyContainsFold(q.Title, d.Title) {
			return false
		}
	}
	// Status exact-fold, with archived bool awareness.
	if q.Status != "" {
		if strings.EqualFold(strings.TrimSpace(d.Status), strings.TrimSpace(q.Status)) {
			// exact match, pass
		} else if strings.EqualFold(strings.TrimSpace(q.Status), "archived") && d.IsArchived() {
			// pass: status:archived matches Archived=true or Status=archived
		} else {
			return false
		}
	}
	// Path substring-fold.
	if q.Path != "" {
		if !strings.Contains(strings.ToLower(d.Path), strings.ToLower(q.Path)) {
			return false
		}
	}
	// Generic filters.
	for k, v := range q.Generic {
		fieldStr, ok := lookupField(d, k)
		if !ok {
			return false
		}
		if !fuzzyContainsFold(v, fieldStr) {
			return false
		}
	}
	// Date filters.
	if q.Created != nil {
		if isEitherField(q.Created.Field) {
			if !matchEither(d.CreatedAt, d.UpdatedAt, q.Created) {
				return false
			}
		} else {
			if !matchDate(d.CreatedAt, q.Created) {
				return false
			}
		}
	}
	if q.Updated != nil {
		if isEitherField(q.Updated.Field) {
			if !matchEither(d.CreatedAt, d.UpdatedAt, q.Updated) {
				return false
			}
		} else {
			if !matchDate(d.UpdatedAt, q.Updated) {
				return false
			}
		}
	}
	return true
}

// Rank filters docs via MatchesDoc, then scores each bare word per doc
// (see scoreDoc): the word must match (AND semantics), and its contribution
// is the substring match length plus a graded title boost. Sort is score
// desc, tie-break most-recent UpdatedAt, then Path.
// Empty Bare returns filtered docs sorted by Path.
func Rank(docs []*model.Document, q *query.Query) []*model.Document {
	filtered := make([]*model.Document, 0, len(docs))
	for _, d := range docs {
		if d == nil {
			continue
		}
		if MatchesDoc(d, q) {
			filtered = append(filtered, d)
		}
	}
	var bare []string
	if q != nil {
		bare = q.Bare
	}
	nonEmpty := false
	for _, w := range bare {
		if strings.TrimSpace(w) != "" {
			nonEmpty = true
			break
		}
	}
	if !nonEmpty {
		sort.SliceStable(filtered, func(i, j int) bool {
			return filtered[i].Path < filtered[j].Path
		})
		return filtered
	}
	type scored struct {
		d     *model.Document
		score int
	}
	scoredDocs := make([]scored, 0, len(filtered))
	for _, d := range filtered {
		s, ok := scoreDoc(d, bare)
		if !ok {
			continue
		}
		scoredDocs = append(scoredDocs, scored{d: d, score: s})
	}
	sort.SliceStable(scoredDocs, func(i, j int) bool {
		if scoredDocs[i].score != scoredDocs[j].score {
			return scoredDocs[i].score > scoredDocs[j].score
		}
		// Tie-break: most-recent UpdatedAt first (nil = oldest).
		ai, bj := scoredDocs[i].d.UpdatedAt, scoredDocs[j].d.UpdatedAt
		if ai != nil && bj != nil {
			if !ai.Equal(*bj) {
				return ai.After(*bj)
			}
		} else if ai != nil && bj == nil {
			return true
		} else if ai == nil && bj != nil {
			return false
		}
		return scoredDocs[i].d.Path < scoredDocs[j].d.Path
	})
	out := make([]*model.Document, 0, len(scoredDocs))
	for _, s := range scoredDocs {
		out = append(out, s.d)
	}
	return out
}

// FilterArchived returns docs with archived ones removed unless includeArchived.
// It does not mutate the input slice.
func FilterArchived(docs []*model.Document, includeArchived bool) []*model.Document {
	if includeArchived {
		out := make([]*model.Document, len(docs))
		copy(out, docs)
		return out
	}
	out := make([]*model.Document, 0, len(docs))
	for _, d := range docs {
		if d == nil {
			continue
		}
		if d.IsArchived() {
			continue
		}
		out = append(out, d)
	}
	return out
}

// scoreDoc scores d against bare words. ok=false means a bare word had
// no match (AND semantics) and the doc should be excluded.
//
// The full search blob (which includes the body and all frontmatter) is
// matched by case-insensitive substring. Running the subsequence fuzzy matcher
// over a multi-thousand-word body makes almost any short query match (e.g.
// "kumquat" matches hundreds of unrelated notes), so the fuzzy fallback is
// restricted to the title, where typo tolerance is most useful and the text is
// short enough to keep matches meaningful.
//
// Per bare word the score adds:
//
//	len(word)              substring found in the blob (body/frontmatter)
//	fuzzy score            not in blob, but fuzzy-matches the title
//	+ exactTitleBoost      title equals the word exactly
//	+ titleBoost           word appears as a whole word in the title
//	+ partialTitleBoost    word appears inside a longer title word
//
// The last three are mutually exclusive (see titleMatchBoost). So a search
// for "butter" scores "Butter" (200 + 6) above "Buttermilk" (50 + 6), which
// in turn edges out a body-only mention (6).
func scoreDoc(d *model.Document, bare []string) (score int, ok bool) {
	blob := d.SearchBlob
	if strings.TrimSpace(blob) == "" {
		blob = strings.TrimSpace(d.Title + " " + d.Body)
	}
	lowerBlob := strings.ToLower(blob)
	lowerTitle := strings.ToLower(strings.TrimSpace(d.Title))
	total := 0
	for _, w := range bare {
		w = strings.TrimSpace(w)
		if w == "" {
			continue
		}
		lw := strings.ToLower(w)
		if strings.Contains(lowerBlob, lw) {
			total += len([]rune(lw))
		} else {
			m := fuzzy.Find(w, []string{d.Title})
			if len(m) == 0 {
				return 0, false
			}
			total += m[0].Score
		}
		total += titleMatchBoost(lowerTitle, lw)
	}
	return total, true
}

// titleMatchBoost grades how the bare word appears in the lowercased,
// trimmed title: an exact title match outranks a whole-word match, which
// outranks a substring inside a longer word (or no match at all).
func titleMatchBoost(lowerTitle, lw string) int {
	switch {
	case lw == "" || lowerTitle == "":
		return 0
	case lowerTitle == lw:
		return exactTitleBoost
	case containsWord(lowerTitle, lw):
		return titleBoost
	case strings.Contains(lowerTitle, lw):
		return partialTitleBoost
	default:
		return 0
	}
}

// containsWord reports whether word occurs in text delimited by non-word
// characters, so "butter" matches "Butter chicken" but not "Buttermilk".
func containsWord(text, word string) bool {
	if word == "" {
		return false
	}
	offset := 0
	for {
		i := strings.Index(text[offset:], word)
		if i < 0 {
			return false
		}
		start := offset + i
		end := start + len(word)
		beforeOK := start == 0
		if !beforeOK {
			r, _ := utf8.DecodeLastRuneInString(text[:start])
			beforeOK = !isWordRune(r)
		}
		afterOK := end >= len(text)
		if !afterOK {
			r, _ := utf8.DecodeRuneInString(text[end:])
			afterOK = !isWordRune(r)
		}
		if beforeOK && afterOK {
			return true
		}
		offset = start + 1
	}
}

func isWordRune(r rune) bool {
	return unicode.IsLetter(r) || unicode.IsDigit(r) || r == '_'
}

// fuzzyContainsFold reports case-insensitive substring or fuzzy match.
func fuzzyContainsFold(pattern, text string) bool {
	if pattern == "" {
		return true
	}
	if text == "" {
		return false
	}
	if strings.Contains(strings.ToLower(text), strings.ToLower(pattern)) {
		return true
	}
	m := fuzzy.Find(pattern, []string{text})
	return len(m) > 0
}

func isEitherField(f string) bool {
	switch strings.ToLower(f) {
	case "date", "before", "after":
		return true
	default:
		return false
	}
}

func matchDate(t *time.Time, f *query.DateFilter) bool {
	if f == nil {
		return true
	}
	if t == nil {
		return false
	}
	if f.From != nil {
		if f.FromIncl {
			if t.Before(*f.From) {
				return false
			}
		} else {
			if !t.After(*f.From) {
				return false
			}
		}
	}
	if f.To != nil {
		if f.ToIncl {
			if t.After(*f.To) {
				return false
			}
		} else {
			if !t.Before(*f.To) {
				return false
			}
		}
	}
	return true
}

// matchEither reports whether either timestamp satisfies f.
// Both nil fails when a filter is present.
func matchEither(createdAt, updatedAt *time.Time, f *query.DateFilter) bool {
	if f == nil {
		return true
	}
	if matchDate(createdAt, f) || matchDate(updatedAt, f) {
		// matchDate(nil) is false, so both-nil yields false here
		// unless... guard explicitly:
		if createdAt == nil && updatedAt == nil {
			return false
		}
		return true
	}
	return false
}

// lookupField finds the stringified value for a generic key.
// It first looks in Raw case-insensitively, then in normalized fields.
func lookupField(d *model.Document, key string) (string, bool) {
	lk := strings.ToLower(strings.TrimSpace(key))
	// Raw first (case-insensitive).
	if d.Raw != nil {
		for k, v := range d.Raw {
			if strings.EqualFold(k, lk) || strings.EqualFold(k, key) {
				return stringifyValue(v), true
			}
		}
		// Also try normalized key forms (e.g. belongs_to vs belongsto).
		normWant := normalizeKey(lk)
		for k, v := range d.Raw {
			if normalizeKey(strings.ToLower(k)) == normWant {
				return stringifyValue(v), true
			}
		}
	}
	if s, ok := normalizedField(d, lk); ok {
		return s, true
	}
	return "", false
}

func normalizeKey(k string) string {
	return strings.ReplaceAll(strings.ToLower(strings.TrimSpace(k)), "_", "")
}

// normalizedField maps generic keys onto Document normalized fields.
func normalizedField(d *model.Document, lk string) (string, bool) {
	switch normalizeKey(lk) {
	case "type", "doctype":
		return d.DocType, true
	case "title":
		return d.Title, true
	case "description", "desc":
		return d.Description, true
	case "status":
		return d.Status, true
	case "resource":
		return d.Resource, true
	case "role":
		return d.Role, true
	case "path":
		return d.Path, true
	case "tags", "tag":
		return strings.Join(d.Tags, " "), true
	case "body", "content":
		return d.Body, true
	case "format":
		return string(d.Format), true
	case "organized":
		if d.Organized == nil {
			return "false", true
		}
		return strconv.FormatBool(*d.Organized), true
	case "archived":
		if d.IsArchived() {
			return "true", true
		}
		return "false", true
	case "belongsto", "belongs", "relatedto", "related":
		// Disambiguate after normalization.
		n := normalizeKey(lk)
		if strings.HasPrefix(n, "belongs") {
			return strings.Join(d.BelongsTo, " "), true
		}
		return strings.Join(d.RelatedTo, " "), true
	case "created", "createdat", "updated", "updatedat", "modified", "lastmodified", "date", "timestamp":
		var parts []string
		if d.CreatedAt != nil {
			parts = append(parts, d.CreatedAt.Format("2006-01-02"), d.CreatedAt.Format(time.RFC3339))
		}
		if d.UpdatedAt != nil {
			parts = append(parts, d.UpdatedAt.Format("2006-01-02"), d.UpdatedAt.Format(time.RFC3339))
		}
		return strings.Join(parts, " "), true
	default:
		return "", false
	}
}

// stringifyValue flattens a Raw frontmatter value for fuzzy matching.
func stringifyValue(v any) string {
	switch t := v.(type) {
	case nil:
		return ""
	case string:
		return t
	case bool:
		return strconv.FormatBool(t)
	case *bool:
		if t == nil {
			return ""
		}
		return strconv.FormatBool(*t)
	case int:
		return strconv.Itoa(t)
	case int8, int16, int32, int64:
		return fmt.Sprintf("%d", t)
	case uint, uint8, uint16, uint32, uint64:
		return fmt.Sprintf("%d", t)
	case float32, float64:
		return fmt.Sprintf("%v", t)
	case []string:
		return strings.Join(t, " ")
	case []any:
		parts := make([]string, 0, len(t))
		for _, e := range t {
			parts = append(parts, stringifyValue(e))
		}
		return strings.Join(parts, " ")
	case time.Time:
		return t.Format(time.RFC3339) + " " + t.Format("2006-01-02")
	case *time.Time:
		if t == nil {
			return ""
		}
		return t.Format(time.RFC3339) + " " + t.Format("2006-01-02")
	default:
		return fmt.Sprintf("%v", t)
	}
}
