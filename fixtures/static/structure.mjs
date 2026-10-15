import { defineStructure } from 'astro-cms-internal-link-structure';
const file = process.env.FORMAT === 'file';
const prefix = (process.env.BASE ?? '/').replace(/\/$/u, '');
export const key = p => `${prefix}${p === '/' ? (file && prefix ? '' : '/') : file ? p.replace(/\/$/u, '') : p}`;
export const href = p => /\.[a-z]+$/u.test(p) ? p : process.env.SLASH === 'never' && p !== '/' ? p.replace(/\/$/u, '') : process.env.SLASH === 'always' && !p.endsWith('/') ? `${p}/` : p;
export const structure = defineStructure({
  parents: { [key('/docs/')]: key('/'), [key('/docs/guide/')]: key('/docs/'), [key('/docs/deep/topic/')]: key('/docs/') },
  labels: { [key('/')]: 'Home', [key('/docs/')]: 'Docs', [key('/docs/guide/')]: 'Guide', [key('/docs/deep/topic/')]: 'Deep topic' },
  requireChildToParent: true, requireParentToChild: true,
  requireBreadcrumbs: true, breadcrumbSelector: 'nav[aria-label="Breadcrumb"]',
});
