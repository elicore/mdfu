package task

// runMove is a placeholder. Todo 12 replaces it with the real `mdfu task move`
// implementation in this file.
func runMove(env Env, _ []string) Result {
	errf(env, "not implemented")
	return Result{Code: 1}
}

func init() {
	register(Subcommand{Name: "move", Order: 40, Run: runMove})
}
