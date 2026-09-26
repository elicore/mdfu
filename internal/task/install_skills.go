package task

// runInstallSkills is a placeholder. Todo 15 replaces it with the real `mdfu
// task install-skills` implementation in this file.
func runInstallSkills(env Env, _ []string) Result {
	errf(env, "not implemented")
	return Result{Code: 1}
}

func init() {
	register(Subcommand{Name: "install-skills", Order: 90, Run: runInstallSkills})
}
