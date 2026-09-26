package task

// runSet is a placeholder. Todo 13 replaces it with the real `mdfu task set`
// implementation in this file.
func runSet(env Env, _ []string) Result {
	errf(env, "not implemented")
	return Result{Code: 1}
}

func init() {
	register(Subcommand{Name: "set", Order: 50, Run: runSet})
}
