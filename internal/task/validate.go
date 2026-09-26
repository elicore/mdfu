package task

// runValidate is a placeholder. Todo 14 replaces it with the real `mdfu task
// validate` implementation in this file.
func runValidate(env Env, _ []string) Result {
	errf(env, "not implemented")
	return Result{Code: 1}
}

func init() {
	register(Subcommand{Name: "validate", Order: 80, Run: runValidate})
}
