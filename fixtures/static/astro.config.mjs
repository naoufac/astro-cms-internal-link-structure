import { defineConfig } from 'astro/config';
import { internalLinkStructure } from 'astro-cms-internal-link-structure';
import { structure, key } from './structure.mjs';
const integration = internalLinkStructure({
  mode: process.env.MODE ?? 'enforce', hierarchy: structure, entrypoints: [key('/')],
  reportPath: 'reports/links.json',
  thresholds: { brokenPaths: 0, brokenFragments: 0, orphans: 0, unreachable: 0, hierarchyViolations: 0 },
  ...(process.env.MISSING_BINARY ? { binaryPath: '/nonexistent/internal-link-engine' } : {}),
  ...(process.env.CONFLICT ? { basePath: '/conflict' } : {}),
  ...(process.env.INVALID ? { thresholds: { brokenPaths: -1 } } : {}),
});
export default defineConfig({
  site: 'https://example.test', base: process.env.BASE ?? '/',
  trailingSlash: process.env.SLASH ?? 'ignore',
  output: process.env.SERVER ? 'server' : 'static',
  build: { format: process.env.FORMAT ?? 'directory' },
  integrations: process.env.INTEGRATION === 'off' ? [] : [integration],
});
