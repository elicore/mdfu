// Package open launches a URL with the platform's default handler.
package open

import (
	"errors"
	"fmt"
	"net/url"
	"os/exec"
	"runtime"
	"strings"
)

// Open launches rawurl in the user's default browser. Only web schemes are
// accepted so a crafted document cannot invoke an arbitrary program. The
// process is started asynchronously; its exit status is not reported.
func Open(rawurl string) error {
	rawurl = strings.TrimSpace(rawurl)
	if rawurl == "" {
		return errors.New("open: empty URL")
	}
	u, err := url.Parse(rawurl)
	if err != nil {
		return fmt.Errorf("open: %w", err)
	}
	switch strings.ToLower(u.Scheme) {
	case "http", "https", "mailto":
	default:
		return fmt.Errorf("open: unsupported scheme %q", u.Scheme)
	}

	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		cmd = exec.Command("open", rawurl)
	case "windows":
		cmd = exec.Command("rundll32", "url.dll,FileProtocolHandler", rawurl)
	default:
		cmd = exec.Command("xdg-open", rawurl)
	}
	if err := cmd.Start(); err != nil {
		return fmt.Errorf("open: %w", err)
	}
	go func() { _ = cmd.Wait() }()
	return nil
}
