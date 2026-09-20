package tui

import (
	"net/url"
	"regexp"
	"strings"
	"unicode/utf8"

	"github.com/charmbracelet/x/ansi"

	"github.com/elicore/mdfu/internal/model"
)

// linkInfo is a web hyperlink discovered in a document body.
type linkInfo struct {
	Label string
	URL   string
}

// Marker runes are Private Use Area code points used to carry link targets
// through the markdown renderer without displaying the raw URL. A start rune
// is linkStartBase+index; linkEndRune closes the matching link. Links never
// nest, so a single closing rune is enough.
const (
	linkStartBase = 0xE000
	linkEndRune   = 0xF8FF
)

// isWebLink reports whether raw is a URL a browser can open.
func isWebLink(raw string) bool {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil {
		return false
	}
	switch strings.ToLower(u.Scheme) {
	case "http", "https", "mailto":
		return true
	}
	return false
}

// docFirstLink returns the first web link a document exposes: its Resource URL
// when that is browser-openable, otherwise the first web link in its body.
func docFirstLink(doc *model.Document) string {
	if doc == nil {
		return ""
	}
	if isWebLink(doc.Resource) {
		return doc.Resource
	}
	_, links := extractAndHide(doc.Body, true)
	if len(links) > 0 {
		return links[0].URL
	}
	return ""
}

// linkLabel returns a short display label for a URL. It is used for autolinks,
// which carry no markdown label of their own.
func linkLabel(raw string) string {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil {
		return raw
	}
	if strings.EqualFold(u.Scheme, "mailto") {
		if u.Opaque != "" {
			return u.Opaque
		}
		return strings.TrimPrefix(raw, "mailto:")
	}
	if u.Host != "" {
		return u.Host
	}
	return raw
}

// osc8Link renders label as a terminal hyperlink pointing at raw.
func osc8Link(raw, label string) string {
	return ansi.SetHyperlink(raw) + linkSGR + label + resetSGR + ansi.ResetHyperlink()
}

// extractAndHide rewrites markdown links so the renderer shows only the label,
// never the raw URL. Web links are wrapped in marker runes for later OSC 8
// patching; non-web links (relative paths, anchors) become plain label text.
// Links inside fenced code blocks and inline code spans are left untouched.
//
// When hyperlinks is false the source is returned unchanged: the renderer then
// falls back to its normal "label url" output, keeping the target visible and
// copyable on terminals without OSC 8 support.
func extractAndHide(src string, hyperlinks bool) (string, []linkInfo) {
	if !hyperlinks {
		return src, nil
	}
	var links []linkInfo
	var out strings.Builder
	inFence := false
	var fenceChar byte
	fenceLen := 0
	for _, line := range strings.SplitAfter(src, "\n") {
		body := strings.TrimSuffix(line, "\n")
		if inFence {
			out.WriteString(line)
			if isFenceClose(body, fenceChar, fenceLen) {
				inFence = false
			}
			continue
		}
		if ch, n, ok := fenceOpen(body); ok {
			inFence = true
			fenceChar = ch
			fenceLen = n
			out.WriteString(line)
			continue
		}
		out.WriteString(scanInline(body, &links))
		if strings.HasSuffix(line, "\n") {
			out.WriteString("\n")
		}
	}
	return out.String(), links
}

// scanInline rewrites links on a single non-fenced line.
func scanInline(line string, links *[]linkInfo) string {
	var out strings.Builder
	i := 0
	for i < len(line) {
		c := line[i]
		switch {
		case c == '`':
			n := runLen(line, i, '`')
			if end := codeSpanEnd(line, i+n, n); end >= 0 {
				out.WriteString(line[i:end])
				i = end
				continue
			}
			out.WriteString(line[i : i+n])
			i += n
		case c == '!':
			// Leave image syntax (![alt](src)) to the renderer by copying the
			// whole construct; otherwise its destination would be rescanned and
			// picked up as a bare URL.
			if i+1 < len(line) && line[i+1] == '[' {
				if _, _, end, ok := parseInlineLink(line, i+1); ok {
					out.WriteString(line[i:end])
					i = end
					continue
				}
			}
			out.WriteByte(c)
			i++
		case c == '[':
			if label, dest, end, ok := parseInlineLink(line, i); ok {
				writeLink(&out, links, dest, label)
				i = end
				continue
			}
			out.WriteByte(c)
			i++
		case c == '<':
			if label, dest, end, ok := parseAutolink(line, i); ok {
				writeLink(&out, links, dest, label)
				i = end
				continue
			}
			out.WriteByte(c)
			i++
		case (c == 'h' || c == 'H') && isWordBoundary(line, i):
			if dest, end, ok := parseBareURL(line, i); ok {
				writeLink(&out, links, dest, linkLabel(dest))
				i = end
				continue
			}
			out.WriteByte(c)
			i++
		default:
			out.WriteByte(c)
			i++
		}
	}
	return out.String()
}

// writeLink appends a link to out. Web links get marker runes and a table
// entry; other links are reduced to their label.
func writeLink(out *strings.Builder, links *[]linkInfo, dest, label string) {
	if !isWebLink(dest) {
		out.WriteString(label)
		return
	}
	idx := len(*links)
	if idx >= linkEndRune-linkStartBase {
		// Out of marker range: degrade to a plain label rather than corrupt.
		out.WriteString(label)
		return
	}
	*links = append(*links, linkInfo{Label: label, URL: dest})
	out.WriteRune(rune(linkStartBase + idx))
	out.WriteString(label)
	out.WriteRune(linkEndRune)
}

// parseInlineLink parses "[label](destination "title")" starting at i,
// returning the label, destination and the index just past the closing ')'.
func parseInlineLink(s string, i int) (label, dest string, end int, ok bool) {
	if i >= len(s) || s[i] != '[' {
		return "", "", i, false
	}
	depth := 0
	j := i
	for j < len(s) {
		switch s[j] {
		case '\\':
			j += 2
			continue
		case '[':
			depth++
		case ']':
			depth--
			if depth == 0 {
				goto closed
			}
		}
		j++
	}
	return "", "", i, false
closed:
	if j+1 >= len(s) || s[j+1] != '(' {
		return "", "", i, false
	}
	label = s[i+1 : j]
	k := j + 2
	for k < len(s) && (s[k] == ' ' || s[k] == '\t') {
		k++
	}
	if k < len(s) && s[k] == '<' {
		close := strings.IndexByte(s[k+1:], '>')
		if close < 0 {
			return "", "", i, false
		}
		dest = s[k+1 : k+1+close]
		k = k + 1 + close + 1
	} else {
		start := k
		depth := 0
	scan:
		for k < len(s) {
			switch s[k] {
			case '\\':
				if k+1 < len(s) {
					k++
				}
			case '(':
				depth++
			case ')':
				if depth == 0 {
					break scan
				}
				depth--
			case ' ', '\t':
				break scan
			}
			k++
		}
		dest = s[start:k]
	}
	for k < len(s) && (s[k] == ' ' || s[k] == '\t') {
		k++
	}
	if k < len(s) && (s[k] == '"' || s[k] == '\'') {
		quote := s[k]
		k++
		for k < len(s) && s[k] != quote {
			if s[k] == '\\' && k+1 < len(s) {
				k++
			}
			k++
		}
		if k >= len(s) {
			return "", "", i, false
		}
		k++
		for k < len(s) && (s[k] == ' ' || s[k] == '\t') {
			k++
		}
	}
	if k >= len(s) || s[k] != ')' {
		return "", "", i, false
	}
	return label, dest, k + 1, true
}

// parseAutolink parses "<scheme:...>" starting at i. Only web schemes are
// accepted so HTML tags and relative references are left for the renderer.
func parseAutolink(s string, i int) (label, dest string, end int, ok bool) {
	if i >= len(s) || s[i] != '<' {
		return "", "", i, false
	}
	rel := strings.IndexByte(s[i+1:], '>')
	if rel < 0 {
		return "", "", i, false
	}
	inner := s[i+1 : i+1+rel]
	if inner == "" || strings.ContainsAny(inner, " \t\n<>\"") || !isWebLink(inner) {
		return "", "", i, false
	}
	return linkLabel(inner), inner, i + 1 + rel + 1, true
}

// parseBareURL finds an http(s) URL starting at i and trims trailing sentence
// punctuation so "see https://x.com." does not swallow the period.
func parseBareURL(s string, i int) (string, int, bool) {
	lower := strings.ToLower(s[i:])
	if !strings.HasPrefix(lower, "http://") && !strings.HasPrefix(lower, "https://") {
		return "", i, false
	}
	j := i
	for j < len(s) && !isURLTerminator(s[j]) {
		j++
	}
	raw := strings.TrimRight(s[i:j], ".,;:!?")
	for len(raw) > 0 && raw[len(raw)-1] == ')' && strings.Count(raw, ")") > strings.Count(raw, "(") {
		raw = raw[:len(raw)-1]
	}
	if u, err := url.Parse(raw); err != nil || u.Host == "" {
		return "", i, false
	}
	return raw, i + len(raw), true
}

// isURLTerminator reports whether b ends a bare URL.
func isURLTerminator(b byte) bool {
	switch b {
	case ' ', '\t', '\n', '\r', '<', '>', '"', '\'', '`':
		return true
	}
	return false
}

// isWordBoundary reports whether the byte at i starts a word.
func isWordBoundary(s string, i int) bool {
	if i == 0 {
		return true
	}
	b := s[i-1]
	return !((b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9') || b == '_')
}

// runLen counts consecutive occurrences of c from i.
func runLen(s string, i int, c byte) int {
	n := 0
	for i+n < len(s) && s[i+n] == c {
		n++
	}
	return n
}

// codeSpanEnd returns the index just past a closing backtick run of exactly n
// backticks, or -1 when there is none.
func codeSpanEnd(s string, from, n int) int {
	for j := from; j < len(s); {
		if s[j] == '`' {
			k := runLen(s, j, '`')
			if k == n {
				return j + k
			}
			j += k
			continue
		}
		j++
	}
	return -1
}

// fenceOpen reports a fenced-code-block opening (``` or ~~~, three or more).
func fenceOpen(line string) (byte, int, bool) {
	trimmed := strings.TrimLeft(line, " ")
	if len(line)-len(trimmed) > 3 || trimmed == "" {
		return 0, 0, false
	}
	c := trimmed[0]
	if c != '`' && c != '~' {
		return 0, 0, false
	}
	n := runLen(trimmed, 0, c)
	if n < 3 {
		return 0, 0, false
	}
	return c, n, true
}

// isFenceClose reports a fence closing line matching the opening char/length.
func isFenceClose(line string, c byte, n int) bool {
	trimmed := strings.TrimLeft(line, " ")
	if len(line)-len(trimmed) > 3 {
		return false
	}
	m := runLen(trimmed, 0, c)
	if m < n {
		return false
	}
	return strings.TrimSpace(trimmed[m:]) == ""
}

// patchLinks converts marker runes in rendered glamour output into OSC 8
// hyperlinks, styling the enclosed label. Any marker that cannot be matched is
// stripped so Private Use runes never leak into the preview.
func patchLinks(rendered string, links []linkInfo, hyperlinks bool) string {
	if len(links) == 0 {
		return rendered
	}
	if !strings.ContainsFunc(rendered, isLinkMarker) {
		return rendered
	}
	var out strings.Builder
	var active strings.Builder
	var saved []string
	i := 0
	for i < len(rendered) {
		if loc := ansiSGRRe.FindStringIndex(rendered[i:]); loc != nil && loc[0] == 0 {
			seq := rendered[i : i+loc[1]]
			out.WriteString(seq)
			updateActiveSGR(&active, seq)
			i += loc[1]
			continue
		}
		r, size := utf8.DecodeRuneInString(rendered[i:])
		switch {
		case r >= linkStartBase && int(r-linkStartBase) < len(links):
			idx := int(r - linkStartBase)
			saved = append(saved, active.String())
			if hyperlinks {
				out.WriteString(ansi.SetHyperlink(links[idx].URL))
			}
			out.WriteString(linkSGR)
			i += size
		case r == linkEndRune:
			out.WriteString(resetSGR)
			if n := len(saved); n > 0 {
				out.WriteString(saved[n-1])
				saved = saved[:n-1]
			}
			if hyperlinks {
				out.WriteString(ansi.ResetHyperlink())
			}
			i += size
		default:
			out.WriteString(rendered[i : i+size])
			i += size
		}
	}
	res := out.String()
	if strings.ContainsFunc(res, isLinkMarker) {
		res = removeLinkMarkers(res)
	}
	return res
}

// isLinkMarker reports whether r is one of the link carrier runes.
func isLinkMarker(r rune) bool { return r >= linkStartBase && r <= linkEndRune }

// osc8HyperlinkRe matches an OSC 8 hyperlink sequence and captures its URI,
// which is empty for a reset sequence.
var osc8HyperlinkRe = regexp.MustCompile("\x1b]8;[^;]*;([^\x07\x1b]*)(?:\x07|\x1b\\\\)")

// closeOpenHyperlinks appends an OSC 8 reset when s ends while a hyperlink is
// still open. Truncating rendered preview text can drop the closing sequence
// while keeping the opener; SGR resets do not close a hyperlink, so the status
// bar and later output would otherwise inherit it.
func closeOpenHyperlinks(s string) string {
	matches := osc8HyperlinkRe.FindAllStringSubmatchIndex(s, -1)
	if len(matches) == 0 {
		return s
	}
	last := matches[len(matches)-1]
	if last[2] != last[3] { // captured URI is non-empty: still open
		return s + ansi.ResetHyperlink()
	}
	return s
}

// removeLinkMarkers drops any leftover carrier runes.
func removeLinkMarkers(s string) string {
	var b strings.Builder
	for _, r := range s {
		if isLinkMarker(r) {
			continue
		}
		b.WriteRune(r)
	}
	return b.String()
}
