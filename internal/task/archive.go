package task

// runArchive is a placeholder. Todo 14 replaces it with the real `mdfu task
// archive` implementation in this file.
func runArchive(env Env, _ []string) Result {
	errf(env, "not implemented")
	return Result{Code: 1}
}

func init() {
	register(Subcommand{Name: "archive", Order: 70, Run: runArchive})
}
