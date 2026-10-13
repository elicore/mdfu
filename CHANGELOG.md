# Changelog

## [0.4.0](https://github.com/elicore/mdfu/compare/v0.3.0...v0.4.0) (2026-10-13)


### ⚠ BREAKING CHANGES

* **rust:** `mdfu` is now the Rust binary, built from the Cargo workspace at the repository root; the Go implementation was deleted after the parity gate. Building from source needs Rust 1.98.1+ (pinned in `rust-toolchain.toml`) instead of Go — a Go toolchain is now required only by `make parity`, which checks out the frozen baseline recorded in `PARITY.md`. Non-TTY output, including `mdfu task list --json`, stays byte-compatible with the Go baseline; TTY box-drawing glyphs and exact SGR styling are relaxed because ratatui replaces lipgloss. `--no-ignore` now takes effect (the Go flag was accepted but ignored), so gitignored files are scanned when it is passed.

### 🚀 Features

* **cli:** add --config to load the optional TUI theme ([efe2ff8](https://github.com/elicore/mdfu/commit/efe2ff84d972613f2e0677a24542523a8b26ff7d))
* **cli:** add filter pipeline and TUI filter function ([086636b](https://github.com/elicore/mdfu/commit/086636b67907f5b78995d518de771b096ee58c3f))
* **cli:** add root flag parsing and mode dispatch ([948c0ad](https://github.com/elicore/mdfu/commit/948c0ad4e602ee05a020a1df57aa56100279d4fb))
* **cli:** make the query a positional argument and add --config default ([0a69feb](https://github.com/elicore/mdfu/commit/0a69feb6d17ce1f487e7dc274545cbbf9b287161))
* **cli:** output formatters and filter stub ([282802d](https://github.com/elicore/mdfu/commit/282802d71704252551fba7dbe24edf4990984910))
* **cli:** port task/tasks router semantics ([07b05ca](https://github.com/elicore/mdfu/commit/07b05ca4e8fcdce2be5419191faed5591b7531e5))
* **cli:** route mdfu task and mdfu tasks in the root command ([dfcffac](https://github.com/elicore/mdfu/commit/dfcffac4213cfd1a428d0e358fc3770cd001b17e))
* **cli:** wire interactive task browser for TTY tasks ([79f6116](https://github.com/elicore/mdfu/commit/79f61161e1aa3608764aba4dde1c2978a0a03336))
* **core:** implement --no-ignore gitignore handling (behavior change) ([6b7d5b1](https://github.com/elicore/mdfu/commit/6b7d5b1d99377d01612710ccf05794df74e576d3))
* **core:** port directory scan with parity-default gitignore behavior ([9f9c302](https://github.com/elicore/mdfu/commit/9f9c30271a00e2d229bf118062304574672718e3))
* **core:** port Document model from internal/model ([fae3c1f](https://github.com/elicore/mdfu/commit/fae3c1f12a4cb3c3a03fea259558e114b48ca6c2))
* **core:** port frontmatter parsing and normalization ([b6e51c6](https://github.com/elicore/mdfu/commit/b6e51c6740cfa06922cf49ae603ea22053bd5320))
* **core:** port output formats with Go-compatible JSON escaping ([5f74800](https://github.com/elicore/mdfu/commit/5f748003fbe9c8f52e0a076c20d91eae62b9b8ea))
* **core:** port query parser and date filters ([43ec368](https://github.com/elicore/mdfu/commit/43ec368a09a57b7b25313bfdba5e4e300f44cd8f))
* **core:** port search ranking with sahilm-compatible fuzzy matcher ([dd9618d](https://github.com/elicore/mdfu/commit/dd9618de0bd5f37011a051d82cad7074dea55055))
* **core:** port URL open with scheme allowlist ([6875126](https://github.com/elicore/mdfu/commit/6875126a340ecaf1f65fd26bfb0ca7889bc385b5))
* **core:** port XDG YAML theme config ([cca94f7](https://github.com/elicore/mdfu/commit/cca94f7fbda237621de4eee713a9b02c2e435cae))
* distribute prebuilt bottles via Homebrew cask ([80be067](https://github.com/elicore/mdfu/commit/80be0673aa2407491f8c9c0ef8f709ae4a848976))
* grade title matches in ranking and highlight matches in TUI ([c8512ed](https://github.com/elicore/mdfu/commit/c8512edc20a92418f1905a17ca0ff52f14df6118))
* **mdfu:** wire TUI document reload hook ([07f4527](https://github.com/elicore/mdfu/commit/07f4527a7a54e9d597587fea241f3063a9a70cf0))
* **parse:** normalized model and OKF/Portent parser with fixtures ([df06910](https://github.com/elicore/mdfu/commit/df0691051ce78570d6924be7611ecff3505bd47e))
* render markdown links as OSC 8 hyperlinks in TUI preview ([da6cf26](https://github.com/elicore/mdfu/commit/da6cf26e175479ac9097834d6342fa05f0e77fef))
* **scan:** scaffold CLI flags and markdown walker ([d2c7a30](https://github.com/elicore/mdfu/commit/d2c7a30fb093512194ad29e87953ff74d0a16092))
* **search:** query parser with qualifiers and fuzzy ranker ([6bace15](https://github.com/elicore/mdfu/commit/6bace1593a4bc4a45210b6b3e619e46131b6b08d))
* **task:** port atomic file edits with byte-exact fidelity ([55990c9](https://github.com/elicore/mdfu/commit/55990c9f7404a92b0668f7233b321103adff4ef7))
* **task:** port blockers and list/view rendering ([a7a8030](https://github.com/elicore/mdfu/commit/a7a80301d004c3a798dcb284a656515748e80fe1))
* **task:** port body extraction and fence masking ([70e6d13](https://github.com/elicore/mdfu/commit/70e6d139906da16f9d147fd872ef5741ccfd3b3b))
* **task:** port header grammar, tokens, and round-trip rendering ([a3e7be7](https://github.com/elicore/mdfu/commit/a3e7be72b3abec77fb4af0ff216bb9ae1debe03a))
* **task:** port ID resolution and all-or-nothing assignment planning ([9448281](https://github.com/elicore/mdfu/commit/94482819e7eb3989236bbf1c593c08b0b00c00da))
* **task:** port nine subcommands and embedded skills ([04f8311](https://github.com/elicore/mdfu/commit/04f8311dce4ff2c894351373b8037596ca078b14))
* **task:** port task config, discovery, and glob subset ([0d04598](https://github.com/elicore/mdfu/commit/0d045989e2a6f429a2b656e47aafe419fe40a766))
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
* **tests:** library-level functional + regression suite (scan-&gt;parse-&gt;query-&gt;rank) ([a32eba5](https://github.com/elicore/mdfu/commit/a32eba514e481cb96ac275c27a94db529b39289c))
* **theme:** add DefaultYAML for printing the builtin config ([011e3c9](https://github.com/elicore/mdfu/commit/011e3c989e9ddffa970306e968789875744d4001))
* **theme:** add optional XDG YAML theme with builtin defaults ([1acc8ba](https://github.com/elicore/mdfu/commit/1acc8ba204d70377e4d069f816291a71ff203b01))
* **theme:** add task TUI style keys ([f2ef77c](https://github.com/elicore/mdfu/commit/f2ef77cd048e62d286b514c573856700893fc32a))
* **tui:** add $EDITOR reload for the note picker ([ca6cf5f](https://github.com/elicore/mdfu/commit/ca6cf5f2c0d9be60ecacef024e7ce372393dfc8b))
* **tui:** add content view-model boundary types ([73cbd59](https://github.com/elicore/mdfu/commit/73cbd5928bd1bc05aa3c98a3376745c3c4799f7f))
* **tui:** add ctrl+f frontmatter toggle and theme-driven styles ([890adf0](https://github.com/elicore/mdfu/commit/890adf012f4014c9afa80b04eef33d26ba88ddd4))
* **tui:** add note picker with injected filtering ([afaff9b](https://github.com/elicore/mdfu/commit/afaff9b9fc9156e19c1955adc445110f8dcc8124))
* **tui:** add reusable widget layer over view-models ([858df43](https://github.com/elicore/mdfu/commit/858df43a16bf5f63748d415cffff5fa448a4fc9b))
* **tui:** add RunTasks entry point with explicit filter injection ([83b2f0c](https://github.com/elicore/mdfu/commit/83b2f0c215026d4a9d8fa257b0e7a8a7f5618425))
* **tui:** add slash-to-search with esc back to list ([33f34f6](https://github.com/elicore/mdfu/commit/33f34f6b7c6780e9cf51be484874717e32817cb2))
* **tui:** add task actions and reload ([c276ec7](https://github.com/elicore/mdfu/commit/c276ec77beee96a909424ad51ba407913b2009db))
* **tui:** add task move and archive actions ([6228d1a](https://github.com/elicore/mdfu/commit/6228d1a15bfaadd3a80eb586707a7bb74cbfc640))
* **tui:** add task title/body editors and move prompt ([2799bce](https://github.com/elicore/mdfu/commit/2799bce2724fc14c8a241d6d7b0175d34bcf6e10))
* **tui:** add task TUI model, keymap, and filter ([956215f](https://github.com/elicore/mdfu/commit/956215f9202016e893891c390f74aea631fab652))
* **tui:** add task TUI view, actions, and editors ([f4b37a0](https://github.com/elicore/mdfu/commit/f4b37a02bc75d4e7d54c62e95690a370db1703e5))
* **tui:** add task UI model with injected filtering ([4f7f12f](https://github.com/elicore/mdfu/commit/4f7f12fe800aa3a9c73ac163d51eff384b253c03))
* **tui:** allowlist list-mode keys and clear filter before abort ([37e4630](https://github.com/elicore/mdfu/commit/37e4630cddac05037d839834339bd0e6b8e769c1))
* **tui:** flatten JSON frontmatter values as inline k: v and lists as pills ([fa3e254](https://github.com/elicore/mdfu/commit/fa3e254319c9a688646ca59b6145719fe3299fce))
* **tui:** integrate document preview pane ([400c936](https://github.com/elicore/mdfu/commit/400c93653f84ee9353ce7369a2b067da50c6d2fb))
* **tui:** interactive BubbleTea picker with preview, multi-select, archived toggle ([63afefa](https://github.com/elicore/mdfu/commit/63afefa0c6f2b0d5dd5e2da2183738666cfb2dae))
* **tui:** mode-aware status hints and transient status line ([e5286bd](https://github.com/elicore/mdfu/commit/e5286bdfbb2e5996bf97f8080973f8b58de52e17))
* **tui:** open focused note in $EDITOR with "e" ([e3657b3](https://github.com/elicore/mdfu/commit/e3657b344f91459fbe05ddcd2058f97703bc171d))
* **tui:** port hyperlink handling and match highlighting ([640fcc3](https://github.com/elicore/mdfu/commit/640fcc3195b14a959d9ce5d18b464c026f6be16d))
* **tui:** reload edited document via injected hook ([c481343](https://github.com/elicore/mdfu/commit/c48134335ec99b52bb02ae4ba0b7a7a7cb84e31d))
* **tui:** start note picker in list mode with hidden search bar ([50b172f](https://github.com/elicore/mdfu/commit/50b172ffc140cc04558b971d41545b37af85fefc))
* **tui:** wire match emphasis, preview links, and pane focus to the ported helpers ([7c119b8](https://github.com/elicore/mdfu/commit/7c119b8eadacc1a30380a4a8327b70a597d041d1))
* **wire:** scan-parse-query-rank-output pipeline plus TUI wiring ([09076b1](https://github.com/elicore/mdfu/commit/09076b171cbf56aaaafe4368d84e933a85a8a792))


### 🐛 Bug Fixes

* address review feedback on OSC 8 hyperlink rendering ([3d56c5b](https://github.com/elicore/mdfu/commit/3d56c5b8067011b01ce4beda756174fd68730ddd))
* **docs:** migrate Starlight site to Astro 7 markdown processor ([531b230](https://github.com/elicore/mdfu/commit/531b230d5307d79349d765a24d2f81c3992458e7))
* highlight body-only matches in TUI list and preview ([04031d6](https://github.com/elicore/mdfu/commit/04031d6b7f43859b868f6881d1a28aeee9810e00))
* keep TUI search box visible and tighten bare-word matching ([3956f6c](https://github.com/elicore/mdfu/commit/3956f6c156891f92659c3c174f3f64a964ad41f2))
* **tasks:** store tags without the leading hash and share the fixture parser ([c8a815e](https://github.com/elicore/mdfu/commit/c8a815e8eeb03ebf35518ae627810a92f2505272))
* **tui:** keep cursor on the same document when the query changes ([87c923e](https://github.com/elicore/mdfu/commit/87c923eb97924b44c76fbd497abce19cf9b17c1d))


### ⚡ Performance

* measure scan/rank budgets on a 5k-file corpus ([42657a0](https://github.com/elicore/mdfu/commit/42657a059e0501988bf7e49348e5e4e57000cb86))
* **reports:** add TUI search refresh regression investigation ([b6495d7](https://github.com/elicore/mdfu/commit/b6495d7af6b9797f65aea16bf6d5374825ebae9f))
* **tui:** keep the frame budget bounded on large bodies ([2ab3778](https://github.com/elicore/mdfu/commit/2ab3778576af94b0c8e3e68e1dff02489be9874a))


### ♻️ Refactoring

* rename module path and URLs to github.com/elicore/mdfu ([12e9b07](https://github.com/elicore/mdfu/commit/12e9b0767f81e8770f2bc876fbc4529069a8ab20))
* **rust:** move the cargo workspace to the repository root ([2599606](https://github.com/elicore/mdfu/commit/259960601b370cdd1b4729e48e1f2c437b63cff6))
* **tui:** decompose the document preview into instantiable components ([5ba4f9d](https://github.com/elicore/mdfu/commit/5ba4f9dce41f529beed0caa7547704d01bd8aaea))
* **tui:** make markdown preview style explicit and cache per style ([b8c7bd1](https://github.com/elicore/mdfu/commit/b8c7bd1fb03b98bd74cbe14e3cadc9b228460d7b))


### 📚 Documentation

* **agents:** add project knowledge base and document TUI list/search modes ([0d1c2d9](https://github.com/elicore/mdfu/commit/0d1c2d97463697fe66ca638977cf0e5709a66020))
* announce the 0.4.0 Rust rewrite ([28e477a](https://github.com/elicore/mdfu/commit/28e477a427ed4c9ac005a8c24a3e484ee284452c))
* announce the task CLI and TUI in README and roadmap ([2f92319](https://github.com/elicore/mdfu/commit/2f92319300883d55908a760f2bcae55b8a6d3b0c))
* **changelog:** restore the 0.3.0 section lost with the release commit ([10a74b4](https://github.com/elicore/mdfu/commit/10a74b44bcdaabb51c7ac4428812e5d72dcca4a9))
* **ci:** document what the local runner gives us, before, and target state ([59b8dfb](https://github.com/elicore/mdfu/commit/59b8dfb1a606b2f4eb2b14bdf0e4ba81dc8e45aa))
* describe wired preview behavior and inert theme keys ([57b7873](https://github.com/elicore/mdfu/commit/57b7873cbeb333f21993d2cf8f32695da40df835))
* document mdfu task, mdfu tasks, and the five divergences ([8ba1f46](https://github.com/elicore/mdfu/commit/8ba1f46d7d42ea5b30eeb33af0a7bb3bffa830e4))
* document the Rust port and widget/content boundary ([48b08a6](https://github.com/elicore/mdfu/commit/48b08a6aa09544a3e238ae2e38b6ce08d11bcaba))
* document the TUI theme file and frontmatter toggle ([d385de3](https://github.com/elicore/mdfu/commit/d385de33b33fae345263f026b51f93a25ec64eda))
* mark F-H done, refresh README post-wire verification ([05966bf](https://github.com/elicore/mdfu/commit/05966bf39cbdc9bd717ca9427da50e10ca1e54d4))
* migrate markdown docs to Astro Starlight site ([60b4285](https://github.com/elicore/mdfu/commit/60b4285055edd7de0e7b968aeb37810b25ce2d09))
* README + headless TUI screenshot (Track H) ([f869793](https://github.com/elicore/mdfu/commit/f869793fe715d97c56a7b45e05ca34e89200bc94))
* refresh README and PLAN for the positional query ([a2d9948](https://github.com/elicore/mdfu/commit/a2d9948bdb668f237b6bc6d6eaea7b606efb0680))
* **reports:** add qmd feature gap analysis ([d17f3c2](https://github.com/elicore/mdfu/commit/d17f3c2133547070ef2cfd280655025c2f4004bd))
* **rust:** add parity and exit-code contract matrices ([10e5d2e](https://github.com/elicore/mdfu/commit/10e5d2ea4a5cf5043cc6126c5b85cf3f8b5035dc))
* **tasks:** document the any-checkbox browser ([12fb922](https://github.com/elicore/mdfu/commit/12fb922b53bc68982d67d090524a0206412c17e1))
* **tasks:** write the normative task format and output specification ([41aee0a](https://github.com/elicore/mdfu/commit/41aee0a112c08c54f71a1f36987d98d482911e64))
* **tui:** document cursor re-anchoring on query edit ([32c9745](https://github.com/elicore/mdfu/commit/32c97450b9656a6fee97bd7433f88625b8425d44))
* **tui:** document list/search modes and editor key ([6d6750b](https://github.com/elicore/mdfu/commit/6d6750b9b7cdad03ab86a92ed44b7a2dd9310c27))
* update PLAN.md with track status (A-E done, F-H pending) ([0167c0a](https://github.com/elicore/mdfu/commit/0167c0a775f59d8afca6fb442dac6c6c1f7621ed))
* update the docs site for the positional query and --config default ([6e4e606](https://github.com/elicore/mdfu/commit/6e4e6062ffe2e5116e1311c0782318fa652a7a6a))


### 📦 Build

* **release:** add Rust release pipeline and cask updates ([48ddfec](https://github.com/elicore/mdfu/commit/48ddfecd91a3e34b4c9ed9204198a07115f97e1f))
* **rust:** scaffold mdfu cargo workspace and rust CI ([a3fc238](https://github.com/elicore/mdfu/commit/a3fc2387a87ef7c22fad8c8e0a38a4f0021e885f))

## [0.3.0](https://github.com/elicore/mdfu/compare/v0.2.0...v0.3.0) (2026-10-12)


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
