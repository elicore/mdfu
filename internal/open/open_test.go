package open

import "testing"

func TestOpenRejectsNonWebSchemes(t *testing.T) {
	for _, raw := range []string{
		"",
		"   ",
		"javascript:alert(1)",
		"file:///etc/passwd",
		"ftp://example.com",
		"command:rm -rf /",
		"://bad",
	} {
		if err := Open(raw); err == nil {
			t.Fatalf("Open(%q) = nil, want error", raw)
		}
	}
}
