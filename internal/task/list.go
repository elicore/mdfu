package task

// runList is a placeholder. Todo 10 replaces it with the real `mdfu task list`
// implementation in this file.
func runList(env Env, _ []string) Result {
	errf(env, "not implemented")
	return Result{Code: 1}
}

func init() {
	register(Subcommand{Name: "list", Order: 10, Run: runList})
}
