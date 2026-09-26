package task

// runIDs is a placeholder. Todo 13 replaces it with the real `mdfu task ids`
// implementation in this file.
func runIDs(env Env, _ []string) Result {
	errf(env, "not implemented")
	return Result{Code: 1}
}

func init() {
	register(Subcommand{Name: "ids", Order: 60, Run: runIDs})
}
