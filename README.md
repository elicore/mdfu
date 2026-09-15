# mdfu

Fuzzy finder for Markdown notes — body text plus normalized frontmatter (OKF v0.1/v0.2 and Portent/Tolaria) from one fast static binary, interactively (TUI) or pipeably (`--filter`, fzf-compatible).

Requires Go 1.25+.

```sh
go install github.com/elicore/mdfu/cmd/mdfu@latest
# or
git clone https://github.com/elicore/mdfu && cd mdfu && go build -o mdfu ./cmd/mdfu

./mdfu --root testdata
./mdfu --root testdata --filter "type:Task"
./mdfu --root testdata --filter "tag:launch kumquat"
```

```sh
go build ./... && go test ./...
```

## Docs

Documentation is an Astro Starlight site under `website/`.

- Live: https://elicore.github.io/mdfu/
- Source: `website/src/content/docs/` (preview: `cd website && npm ci && npm run dev`)
- Roadmap: `PLAN.md`
