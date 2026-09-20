import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { unified } from '@astrojs/markdown-remark';

// GitHub Pages project site: https://elicore.github.io/mdfu
const base = '/mdfu/';

/**
 * Astro does not rewrite root-relative links in Markdown/MDX with `base`.
 * Prefix them here so in-content links like `/guide/query-syntax/` resolve
 * under the Pages base (`/mdfu/`).
 */
function rehypePrefixBaseLinks() {
  const walk = (node) => {
    if (node.type === 'element' && node.tagName === 'a') {
      const href = node.properties?.href;
      if (
        typeof href === 'string' &&
        href.startsWith('/') &&
        !href.startsWith('//') &&
        !href.startsWith(base)
      ) {
        node.properties.href = base + href.slice(1);
      }
    }
    if (Array.isArray(node.children)) node.children.forEach(walk);
  };
  return (tree) => walk(tree);
}

// https://astro.build/config
export default defineConfig({
  site: 'https://elicore.github.io/mdfu',
  base,
  trailingSlash: 'always',
  markdown: {
    // Astro 7's default Sätteri processor has no rehype API, so opt back into
    // the unified/remark pipeline to keep the custom rehype plugin.
    processor: unified({
      rehypePlugins: [rehypePrefixBaseLinks],
    }),
  },
  integrations: [
    starlight({
      title: 'mdfu',
      description: 'Fast markdown finder CLI — search, rank, and browse Markdown notes.',
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/elicore/mdfu' },
      ],
      editLink: {
        baseUrl: 'https://github.com/elicore/mdfu/edit/main/website/',
      },
      sidebar: [
        {
          label: 'Start',
          items: [{ label: 'Introduction', slug: 'index' }],
        },
        {
          label: 'Guide',
          items: [
            { label: 'Query Syntax', slug: 'guide/query-syntax' },
            { label: 'Frontmatter Model', slug: 'guide/frontmatter-model' },
            { label: 'Search & Ranking', slug: 'guide/search-ranking' },
            { label: 'Scan', slug: 'guide/scan' },
            { label: 'TUI', slug: 'guide/tui' },
            { label: 'Configuration', slug: 'guide/configuration' },
          ],
        },
        {
          label: 'Reference',
          items: [
            { label: 'CLI', slug: 'reference/cli' },
            { label: 'Architecture', slug: 'reference/architecture' },
            { label: 'Backend Evaluation', slug: 'reference/backend-evaluation' },
          ],
        },
        {
          label: 'Project',
          items: [
            { label: 'Roadmap', slug: 'project/roadmap' },
            { label: 'History', slug: 'project/history' },
            { label: 'Local CI', slug: 'project/local-ci' },
          ],
        },
      ],
    }),
  ],
});
