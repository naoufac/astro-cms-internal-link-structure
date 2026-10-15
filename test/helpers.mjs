import test from 'node:test';
import assert from 'node:assert/strict';
import { defineStructure, parentOf, childrenOf, breadcrumbsOf, internalLinkStructure } from '../js/index.js';
import { pathToFileURL } from 'node:url';
import { mkdtemp, mkdir, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const input = { parents: { '/docs/': '/', '/docs/api/': '/docs/', '/blog/': '/' }, labels: { '/': 'Home', '/docs/': 'Docs', '/docs/api/': 'API', '/blog/': 'Blog' } };
test('helpers return only explicit data, sorted children, ancestor-only breadcrumbs and immutable snapshots', () => {
  const source = structuredClone(input), structure = defineStructure(source);
  source.parents['/blog/'] = '/docs/'; source.labels['/'] = 'Changed';
  assert.deepEqual(parentOf(structure, '/docs/api/'), { path: '/docs/', label: 'Docs' });
  assert.equal(parentOf(structure, '/'), undefined);
  assert.deepEqual(childrenOf(structure, '/'), [{ path: '/blog/', label: 'Blog' }, { path: '/docs/', label: 'Docs' }]);
  assert.deepEqual(breadcrumbsOf(structure, '/docs/api/'), [{ path: '/', label: 'Home' }, { path: '/docs/', label: 'Docs' }]);
  assert.deepEqual(breadcrumbsOf(structure, '/unknown/'), []);
  assert.throws(() => { structure.labels['/'] = 'Bad'; }, TypeError);
  assert.throws(() => parentOf(input, '/docs/'), /defineStructure/u);
});
test('structure rejects cycles, missing labels, invalid paths, selectors, fields and executable data', () => {
  for (const value of [
    { parents: { '/a/': '/b/', '/b/': '/a/' }, labels: { '/a/': 'A', '/b/': 'B' } },
    { parents: { '/a/': '/' }, labels: { '/': 'Home' } },
    { parents: { '//bad': '/' }, labels: { '//bad': 'Bad', '/': 'Home' } },
    { ...input, requireBreadcrumbs: true }, { ...input, typo: true },
    { ...input, labels: { '/': () => 'Home' } },
    { ...input, get rootLabel() { throw new Error('getter executed'); } },
  ]) assert.throws(() => defineStructure(value), TypeError);
});
test('invalid integration options fail synchronously', () => {
  for (const value of [ { typo: 1 }, { mode: 'bad' }, { buildFormat: 'preserve' }, { thresholds: { brokenPaths: -1 } }, { thresholds: { ignored: 0 } }, { site: 'ftp://example.test' }, { site: 'https://example.test/path' }, { artifactRoot: 'dist' }, { entrypoints: [] }, { timeoutMs: 0 }, { exclude: [() => true] } ]) assert.throws(() => internalLinkStructure(value));
});
test('Astro config capture rejects SSR, preserve, missing site, conflicts and public/output report paths', async () => {
  const root = await mkdtemp(join(tmpdir(), 'astro-link-config-'));
  const url = pathToFileURL(`${root}/`);
  const config = { root: url, site: 'https://example.test', output: 'static', base: '/', trailingSlash: 'ignore', build: { format: 'directory' }, outDir: new URL('dist/', url), publicDir: new URL('public/', url) };
  for (const [options, override, pattern] of [
    [{}, { output: 'server' }, /static/u], [{}, { build: { format: 'preserve' } }, /preserve/u],
    [{}, { site: undefined }, /site/u], [{ basePath: '/wrong' }, {}, /Conflicting/u],
    [{ site: 'https://other.test' }, {}, /Conflicting/u], [{ trailingSlash: 'always' }, {}, /Conflicting/u],
    [{ buildFormat: 'file' }, {}, /Conflicting/u],
    [{ reportPath: 'dist/report.json' }, {}, /outside/u], [{ reportPath: 'public/report.json' }, {}, /outside/u],
  ]) await assert.rejects(internalLinkStructure(options).hooks['astro:config:done']({ config: { ...config, ...override } }), pattern);
  await mkdir(join(root, 'dist'));
  await symlink(join(root, 'dist'), join(root, 'alias'));
  await assert.rejects(internalLinkStructure({ reportPath: 'alias/report.json' }).hooks['astro:config:done']({ config }), /outside/u);
  await internalLinkStructure({ site: 'https://example.test/', basePath: '/' }).hooks['astro:config:done']({ config });
});
