# Changelog

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
