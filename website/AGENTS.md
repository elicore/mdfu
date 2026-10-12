# website/ — Astro Starlight docs

## OVERVIEW

Separate npm package (Node 22, `"type": "module"`) publishing https://elicore.github.io/mdfu — NOT part of the Cargo workspace. `docs/` at repo root is just a redirect stub.

## WHERE TO LOOK

| Task | Location |
|------|----------|
| Content (single source of truth for user docs) | `src/content/docs/{guide,reference,project}/` |
| Sidebar structure | `astro.config.mjs` (`starlight.sidebar`) — MUST edit when adding a page |
| Base-path link fixups | `astro.config.mjs` — custom `rehypePrefixBaseLinks` rewrites root-relative `href`s under `/mdfu/` |
| Build output | `dist/` (gitignored here, unlike root) |

## CONVENTIONS

- Astro 7 markdown uses the unified/remark pipeline explicitly (default Sätteri processor has no rehype API) — the custom rehype plugin depends on this.
- `base: '/mdfu/'`, `trailingSlash: 'always'` — internal links written as `/guide/foo/` work only because of the rehype plugin.
- CI: `.github/workflows/docs.yml` builds and checks internal links (Python link checker). Run locally: `make act-docs`.

## ANTI-PATTERNS

- Don't put user-facing docs in root `docs/` — it's a deliberate redirect (single source of truth).
- Don't edit `dist/` or commit it — gitignored via `website/.gitignore`.
- Don't add a `Cargo.toml` here (module-guard allows exactly one workspace root, at the repository root).
