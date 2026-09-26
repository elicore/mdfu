// Package task implements the mdfu task engine: a pure Go parser, planner, and
// resolver for checkbox tasks in Markdown files.
//
// The engine is deliberately free of CLI, terminal, and filesystem-mutation
// concerns beyond reading configuration. It understands three header forms
// (identified, seed, and unidentified), the metadata token grammar, task
// bodies, fence masking, blocker resolution, ID assignment planning, and task
// configuration.
//
// The task configuration is JSON and is read from .mdtaskrc or .mdfurc. The
// .mdfurc name is the TASK configuration ONLY: it is entirely distinct from the
// YAML theme configuration that mdfu reads from $XDG_CONFIG_HOME/mdfu/config.yaml
// or receives through the --config flag. The two files never merge and share no
// keys.
package task

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
)

// Config is the task-scope configuration read from .mdtaskrc or .mdfurc.
type Config struct {
	Path            string
	Include         []string
	Exclude         []string
	ExcludePrefixes []string
	ArchivePath     string
}

const defaultArchivePath = "_archive.md"

// FindConfigPath walks upward from startDir looking for a task configuration
// file. At each directory level .mdtaskrc wins over .mdfurc; .mdfurc is used
// only when the level has no .mdtaskrc. The walk ends at the filesystem root.
func FindConfigPath(startDir string) (string, bool) {
	dir, err := filepath.Abs(startDir)
	if err != nil {
		dir = startDir
	}
	dir = filepath.Clean(dir)
	for {
		if candidate := filepath.Join(dir, ".mdtaskrc"); fileExists(candidate) {
			return candidate, true
		}
		if candidate := filepath.Join(dir, ".mdfurc"); fileExists(candidate) {
			return candidate, true
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return "", false
		}
		dir = parent
	}
}

// Load reads the task configuration discovered from dir. When no configuration
// file exists it returns a defaults Config and no error. Malformed JSON and an
// unreadable or directory rc path are fatal.
func Load(dir string) (Config, error) {
	path, ok := FindConfigPath(dir)
	if !ok {
		return Config{ArchivePath: defaultArchivePath}, nil
	}

	info, err := os.Stat(path)
	if err != nil {
		return Config{}, fmt.Errorf("cannot read %s: %w", path, err)
	}
	if info.IsDir() {
		return Config{}, fmt.Errorf("cannot read %s: is a directory", path)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return Config{}, fmt.Errorf("cannot read %s: %w", path, err)
	}

	cfg := Config{ArchivePath: defaultArchivePath}
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		return Config{}, fmt.Errorf("Invalid JSON in %s: %s", path, err.Error())
	}

	if v, ok := raw["path"]; ok {
		if s, ok := decodeString(v); ok {
			cfg.Path = s
		}
	}
	if v, ok := raw["files"]; ok {
		var files map[string]json.RawMessage
		if err := json.Unmarshal(v, &files); err == nil {
			if inc, ok := files["include"]; ok {
				cfg.Include = decodeStringArray(inc)
			}
			if exc, ok := files["exclude"]; ok {
				cfg.Exclude = decodeStringArray(exc)
			}
		}
	}
	if v, ok := raw["excludePrefixes"]; ok {
		cfg.ExcludePrefixes = decodeStringArray(v)
	}
	if v, ok := raw["archivePath"]; ok {
		if s, ok := decodeString(v); ok {
			cfg.ArchivePath = s
		}
	}
	return cfg, nil
}

// ResolveBasePath applies the base-path precedence: an explicit flag, then the
// environment, then the config path resolved relative to cfgDir, then ".".
func ResolveBasePath(flagPath, envPath, cfgPath, cfgDir string) string {
	switch {
	case flagPath != "":
		return flagPath
	case envPath != "":
		return envPath
	case cfgPath != "":
		if filepath.IsAbs(cfgPath) {
			return filepath.Clean(cfgPath)
		}
		return filepath.Clean(filepath.Join(cfgDir, cfgPath))
	default:
		return "."
	}
}

// BaseDir returns the nearest ancestor of startDir containing a .git entry, or
// startDir when there is none.
func BaseDir(startDir string) string {
	dir := startDir
	for {
		if _, err := os.Stat(filepath.Join(dir, ".git")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}
	return startDir
}

// fileExists reports whether path exists, including as a directory.
func fileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}

// decodeString decodes a JSON string, reporting ok == false for any other type.
func decodeString(raw json.RawMessage) (string, bool) {
	var s string
	if err := json.Unmarshal(raw, &s); err != nil {
		return "", false
	}
	return s, true
}

// decodeStringArray decodes a JSON array of strings, dropping wrongly-typed
// entries. It returns nil when the value is not an array.
func decodeStringArray(raw json.RawMessage) []string {
	var entries []json.RawMessage
	if err := json.Unmarshal(raw, &entries); err != nil {
		return nil
	}
	out := make([]string, 0, len(entries))
	for _, entry := range entries {
		if s, ok := decodeString(entry); ok {
			out = append(out, s)
		}
	}
	return out
}
