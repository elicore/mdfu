# Changelog

## [0.3.0](https://github.com/elicore/mdfu/compare/v0.2.0...v0.3.0) (2026-10-02)


### 🚀 Features

* **cli:** route mdfu task and mdfu tasks in the root command ([dfcffac](https://github.com/elicore/mdfu/commit/dfcffac4213cfd1a428d0e358fc3770cd001b17e))
* **tasks:** add .mdtaskrc and .mdfurc config resolution ([e11d383](https://github.com/elicore/mdfu/commit/e11d38308779c480f377c06905187da7c8d59c9e))
* **tasks:** add command registry, routing, and exit-code table ([f69ae91](https://github.com/elicore/mdfu/commit/f69ae91e65e8ea71583a26714d5d82975eea53a8))
* **tasks:** add id grammar, assignment planning, and blocker resolution ([28c2d44](https://github.com/elicore/mdfu/commit/28c2d44d6a47f1afbb2b288b4d35f693c881fa21))
* **tasks:** add install-skills with embedded clean-room skill assets ([668c629](https://github.com/elicore/mdfu/commit/668c629910bf40e49a4bc7a16a11377401618b18))
* **tasks:** add list rendering and atomic line-anchored editing ([98a40d8](https://github.com/elicore/mdfu/commit/98a40d8d18bd0ebcbcf6b1a975a510c07caac37c))
* **tasks:** add mdfu task archive and validate ([6a7ae1e](https://github.com/elicore/mdfu/commit/6a7ae1ecc197509e64c8226833f773cd87d3f9e7))
* **tasks:** add mdfu task list ([e42fea2](https://github.com/elicore/mdfu/commit/e42fea246f9190dbd2881086b6006d1662e7db0d))
* **tasks:** add mdfu task open and move ([a0413f2](https://github.com/elicore/mdfu/commit/a0413f2373c18e6933e1b85e9fbd2dde797ce162))
* **tasks:** add mdfu task set and ids ([c2446da](https://github.com/elicore/mdfu/commit/c2446daae1ebbdeac3bdae0e405889db3ef0314b))
* **tasks:** add mdfu task view ([d473abf](https://github.com/elicore/mdfu/commit/d473abf4fa4d88cf715eff7251a43bf631d351e9))
* **tasks:** add task block parser and serializer ([d2a2be3](https://github.com/elicore/mdfu/commit/d2a2be31c06e5d88fc1d6d169708fa4059214a43))
* **tasks:** add task discovery walker and doublestar-subset glob ([9507d8e](https://github.com/elicore/mdfu/commit/9507d8e95ddd6afb8dd90598e85a9ce1209d1392))
* **tasks:** show and edit any markdown checkbox in the tasks TUI ([853599c](https://github.com/elicore/mdfu/commit/853599c98af22f316d97639b35768b23681c11d0))
* **theme:** add task TUI style keys ([f2ef77c](https://github.com/elicore/mdfu/commit/f2ef77cd048e62d286b514c573856700893fc32a))
* **tui:** add RunTasks entry point with explicit filter injection ([83b2f0c](https://github.com/elicore/mdfu/commit/83b2f0c215026d4a9d8fa257b0e7a8a7f5618425))
* **tui:** add task move and archive actions ([6228d1a](https://github.com/elicore/mdfu/commit/6228d1a15bfaadd3a80eb586707a7bb74cbfc640))
* **tui:** add task TUI model, keymap, and filter ([956215f](https://github.com/elicore/mdfu/commit/956215f9202016e893891c390f74aea631fab652))
* **tui:** add task TUI view, actions, and editors ([f4b37a0](https://github.com/elicore/mdfu/commit/f4b37a02bc75d4e7d54c62e95690a370db1703e5))


### 🐛 Bug Fixes

* **tasks:** store tags without the leading hash and share the fixture parser ([c8a815e](https://github.com/elicore/mdfu/commit/c8a815e8eeb03ebf35518ae627810a92f2505272))


### 📚 Documentation

* announce the task CLI and TUI in README and roadmap ([2f92319](https://github.com/elicore/mdfu/commit/2f92319300883d55908a760f2bcae55b8a6d3b0c))
* document mdfu task, mdfu tasks, and the five divergences ([8ba1f46](https://github.com/elicore/mdfu/commit/8ba1f46d7d42ea5b30eeb33af0a7bb3bffa830e4))
* **tasks:** document the any-checkbox browser ([12fb922](https://github.com/elicore/mdfu/commit/12fb922b53bc68982d67d090524a0206412c17e1))
* **tasks:** write the normative task format and output specification ([41aee0a](https://github.com/elicore/mdfu/commit/41aee0a112c08c54f71a1f36987d98d482911e64))

## [0.2.0](https://github.com/elicore/mdfu/compare/v0.1.0...v0.2.0) (2026-09-22)


### 🚀 Features

* **cli:** add --config to load the optional TUI theme ([efe2ff8](https://github.com/elicore/mdfu/commit/efe2ff84d972613f2e0677a24542523a8b26ff7d))
* **cli:** make the query a positional argument and add --config default ([0a69feb](https://github.com/elicore/mdfu/commit/0a69feb6d17ce1f487e7dc274545cbbf9b287161))
* distribute prebuilt bottles via Homebrew cask ([80be067](https://github.com/elicore/mdfu/commit/80be0673aa2407491f8c9c0ef8f709ae4a848976))
* grade title matches in ranking and highlight matches in TUI ([c8512ed](https://github.com/elicore/mdfu/commit/c8512edc20a92418f1905a17ca0ff52f14df6118))
* render markdown links as OSC 8 hyperlinks in TUI preview ([da6cf26](https://github.com/elicore/mdfu/commit/da6cf26e175479ac9097834d6342fa05f0e77fef))
* **theme:** add DefaultYAML for printing the builtin config ([011e3c9](https://github.com/elicore/mdfu/commit/011e3c989e9ddffa970306e968789875744d4001))
* **theme:** add optional XDG YAML theme with builtin defaults ([1acc8ba](https://github.com/elicore/mdfu/commit/1acc8ba204d70377e4d069f816291a71ff203b01))
* **tui:** add ctrl+f frontmatter toggle and theme-driven styles ([890adf0](https://github.com/elicore/mdfu/commit/890adf012f4014c9afa80b04eef33d26ba88ddd4))
* **tui:** flatten JSON frontmatter values as inline k: v and lists as pills ([fa3e254](https://github.com/elicore/mdfu/commit/fa3e254319c9a688646ca59b6145719fe3299fce))


### 🐛 Bug Fixes

* address review feedback on OSC 8 hyperlink rendering ([3d56c5b](https://github.com/elicore/mdfu/commit/3d56c5b8067011b01ce4beda756174fd68730ddd))
* **docs:** migrate Starlight site to Astro 7 markdown processor ([531b230](https://github.com/elicore/mdfu/commit/531b230d5307d79349d765a24d2f81c3992458e7))
* highlight body-only matches in TUI list and preview ([04031d6](https://github.com/elicore/mdfu/commit/04031d6b7f43859b868f6881d1a28aeee9810e00))
* keep TUI search box visible and tighten bare-word matching ([3956f6c](https://github.com/elicore/mdfu/commit/3956f6c156891f92659c3c174f3f64a964ad41f2))
* **tui:** keep cursor on the same document when the query changes ([87c923e](https://github.com/elicore/mdfu/commit/87c923eb97924b44c76fbd497abce19cf9b17c1d))


### ⚡ Performance

* **tui:** keep the frame budget bounded on large bodies ([2ab3778](https://github.com/elicore/mdfu/commit/2ab3778576af94b0c8e3e68e1dff02489be9874a))


### ♻️ Refactoring

* **tui:** decompose the document preview into instantiable components ([5ba4f9d](https://github.com/elicore/mdfu/commit/5ba4f9dce41f529beed0caa7547704d01bd8aaea))
* **tui:** make markdown preview style explicit and cache per style ([b8c7bd1](https://github.com/elicore/mdfu/commit/b8c7bd1fb03b98bd74cbe14e3cadc9b228460d7b))


### 📚 Documentation

* **ci:** document what the local runner gives us, before, and target state ([59b8dfb](https://github.com/elicore/mdfu/commit/59b8dfb1a606b2f4eb2b14bdf0e4ba81dc8e45aa))
* document the TUI theme file and frontmatter toggle ([d385de3](https://github.com/elicore/mdfu/commit/d385de33b33fae345263f026b51f93a25ec64eda))
* refresh README and PLAN for the positional query ([a2d9948](https://github.com/elicore/mdfu/commit/a2d9948bdb668f237b6bc6d6eaea7b606efb0680))
* **tui:** document cursor re-anchoring on query edit ([32c9745](https://github.com/elicore/mdfu/commit/32c97450b9656a6fee97bd7433f88625b8425d44))
* update the docs site for the positional query and --config default ([6e4e606](https://github.com/elicore/mdfu/commit/6e4e6062ffe2e5116e1311c0782318fa652a7a6a))
