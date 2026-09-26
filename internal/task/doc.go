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
