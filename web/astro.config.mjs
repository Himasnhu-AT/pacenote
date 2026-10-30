// @ts-check
import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

// PLACEHOLDER domain: set `site` to the production URL before deploying (also src/data/site.ts
// and public/robots.txt). Canonical, OG and sitemap URLs are built from it.
export default defineConfig({
  site: 'https://pacenote.marlstudio.com',
  trailingSlash: 'always',
  integrations: [sitemap({ filter: (page) => !page.includes('/404') && !page.includes('/og/') })],
  prefetch: { prefetchAll: true, defaultStrategy: 'hover' },
  build: { inlineStylesheets: 'auto' },
  server: { port: 4410 },
});
