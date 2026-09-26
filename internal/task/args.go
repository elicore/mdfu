package task

import (
	"flag"
	"strings"
)

// ReorderInterspersed reorders args so that flag tokens precede the positional
// section, giving GNU-style interspersed flags. Without this, Go's flag package
// stops at the first positional argument, so `mdfu task list query --tag x`
// would fold the trailing flag into the positional section.
//
// A `--` separator is always inserted before the positional section so that a
// positional token starting with a dash stays positional. A flag that is known
// to the FlagSet and is not a bool flag consumes the following token as its
// value, including when that token starts with a dash.
//
// This is the generic form of the helper originally embedded in cmd/mdfu;
// cmd/mdfu.splitFlags now delegates here.
func ReorderInterspersed(fs *flag.FlagSet, args []string) []string {
	var flags, positional []string
	for i := 0; i < len(args); i++ {
		a := args[i]
		switch {
		case a == "--":
			positional = append(positional, args[i+1:]...)
			return joinSections(flags, positional)
		case a == "" || a == "-" || a[0] != '-':
			positional = append(positional, a)
			continue
		}
		flags = append(flags, a)
		name := strings.TrimLeft(a, "-")
		if eq := strings.IndexByte(name, '='); eq >= 0 {
			name = name[:eq]
		}
		if f := fs.Lookup(name); f != nil && !isBoolFlag(f) && !strings.Contains(a, "=") && i+1 < len(args) {
			i++
			flags = append(flags, args[i])
		}
	}
	return joinSections(flags, positional)
}

func joinSections(flags, positional []string) []string {
	if len(positional) == 0 {
		return flags
	}
	return append(append(flags, "--"), positional...)
}

func isBoolFlag(f *flag.Flag) bool {
	bf, ok := f.Value.(interface{ IsBoolFlag() bool })
	return ok && bf.IsBoolFlag()
}
