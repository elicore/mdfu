package task

// runView is a placeholder. Todo 11 replaces it with the real `mdfu task view`
// implementation in this file.
func runView(env Env, _ []string) Result {
	errf(env, "not implemented")
	return Result{Code: 1}
}

func init() {
	register(Subcommand{Name: "view", Aliases: []string{"show"}, Order: 20, Run: runView})
}
