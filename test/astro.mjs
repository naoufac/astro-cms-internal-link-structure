import test from 'node:test';
import assert from 'node:assert/strict';
import { cp, mkdtemp, mkdir, symlink, writeFile, readFile, readdir, rm } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { run } from '../scripts/support.mjs';
const root = fileURLToPath(new URL('../', import.meta.url));
const astro = join(root, 'node_modules/astro/bin/astro.mjs');
async function fixture() {
  const dir = await mkdtemp(join(root, 'fixtures', 'work-'));
  await cp(join(root, 'fixtures/static'), dir, { recursive: true });
  await writeFile(join(dir, 'package.json'), JSON.stringify({ name: 'fixture', private: true, type: 'module' }));
  await mkdir(join(dir, 'node_modules'));
  await symlink(root, join(dir, 'node_modules/astro-cms-internal-link-structure'), 'dir');
  return dir;
}
async function hashes(dir, prefix = '') {
  const output = {};
  for (const entry of (await readdir(dir, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
    const path = join(dir, entry.name), key = `${prefix}${entry.name}`;
    if (entry.isDirectory()) Object.assign(output, await hashes(path, `${key}/`));
    else output[key] = createHash('sha256').update(await readFile(path)).digest('hex');
  }
  return output;
}
function build(dir, env = {}, name = 'astro-build') {
  return run(name, process.execPath, [astro, 'build'], { cwd: dir, env });
}
test('real Astro static variants: nested getStaticPaths, assets, helper rendering, relative document URLs, deterministic reports, no artifact mutation', { timeout: 600_000 }, async t => {
  for (const FORMAT of ['directory', 'file']) for (const BASE of ['/', '/knowledge']) for (const SLASH of ['always', 'never', 'ignore']) {
    await t.test(`${FORMAT} ${BASE} ${SLASH}`, async () => {
      const dir = await fixture();
      const env = { FORMAT, BASE, SLASH };
      const first = build(dir, env, `astro-${FORMAT}-${BASE === '/' ? 'root' : 'base'}-${SLASH}`);
      assert.equal(first.status, 0, `${first.stdout}\n${first.stderr}`);
      const bytes = await readFile(join(dir, 'reports/links.json'));
      const report = JSON.parse(bytes);
      assert.equal(report.coverage.scannedPages, 4); assert.equal(report.coverage.complete, true);
      assert.equal(report.gates.violated, false); assert.deepEqual(report.issues.brokenPaths, []); assert.deepEqual(report.issues.brokenFragments, []);
      assert.equal(report.hierarchy.violations.length, 0);
      const docs = await readFile(join(dir, 'dist', FORMAT === 'file' ? 'docs.html' : 'docs/index.html'), 'utf8');
      assert.match(docs, /Relative guide details/u); assert.match(docs, /aria-label="Breadcrumb"/u);
      const original = await hashes(join(dir, 'dist'));
      assert.equal(Object.keys(original).length, 6);
      const second = build(dir, env, 'astro-repeat');
      assert.equal(second.status, 0, second.stderr);
      assert.deepEqual(await readFile(join(dir, 'reports/links.json')), bytes);
      await rm(join(dir, 'dist'), { recursive: true });
      const without = build(dir, { ...env, INTEGRATION: 'off' }, 'astro-without-integration');
      assert.equal(without.status, 0, without.stderr);
      assert.deepEqual(await hashes(join(dir, 'dist')), original);
      await rm(dir, { recursive: true });
    });
  }
});
test('real Astro broken enforce fails after report; same broken audit succeeds', { timeout: 120_000 }, async () => {
  const dir = await fixture();
  const broken = build(dir, { BROKEN: '1' }, 'astro-broken-enforce');
  assert.notEqual(broken.status, 0);
  const report = JSON.parse(await readFile(join(dir, 'reports/links.json'), 'utf8'));
  assert.equal(report.gates.violated, true);
  assert.equal(report.issues.brokenPaths.length, 4); assert.equal(report.issues.brokenFragments.length, 4);
  const audit = build(dir, { BROKEN: '1', MODE: 'audit' }, 'astro-broken-audit');
  assert.equal(audit.status, 0, audit.stderr);
  const audited = JSON.parse(await readFile(join(dir, 'reports/links.json'), 'utf8'));
  assert.equal(audited.gates.violated, true); assert.deepEqual(audited.issues, report.issues);
  await rm(dir, { recursive: true });
});
test('real Astro missing binary, invalid config, URL conflict, server and preserve fail nonzero', { timeout: 120_000 }, async () => {
  for (const [key, value, expected] of [['MISSING_BINARY', '1', /Cannot execute native engine/u], ['INVALID', '1', /threshold/u], ['CONFLICT', '1', /Conflicting/u], ['SERVER', '1', /static.*unsupported/u], ['FORMAT', 'preserve', /preserve.*unsupported/u]]) {
    const dir = await fixture();
    const result = build(dir, { [key]: value }, `astro-failure-${key}`);
    assert.notEqual(result.status, 0, result.stdout);
    assert.match(`${result.stdout}\n${result.stderr}`, expected);
    await assert.rejects(readFile(join(dir, 'reports/links.json')), { code: 'ENOENT' });
    await rm(dir, { recursive: true });
  }
});
