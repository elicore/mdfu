package task

// runOpen is a placeholder. Todo 12 replaces it with the real `mdfu task open`
// implementation in this file.
func runOpen(env Env, _ []string) Result {
	errf(env, "not implemented")
	return Result{Code: 1}
}

func init() {
	register(Subcommand{Name: "open", Order: 30, Run: runOpen})
}
