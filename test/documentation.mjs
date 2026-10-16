import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { auditArtifacts } from '../js/index.js';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
test('IL-03 README documents compact disk versus full API and all reporting workflows', async () => {
  const readme = await readFile(join(root, 'README.md'), 'utf8');
  assert.doesNotMatch(readme, /Report bytes on disk are exactly the native stdout JSON\./u);
  assert.match(readme, /astro-link-summary\/1/u);
  assert.match(readme, /returns the full native report/u);
  for (const option of ['reportFormat', 'fullReportPath', 'summaryPath', 'baselinePath', 'gscPath', 'gscMinImpressions']) {
    assert.match(readme, new RegExp(`\\| \\x60${option}\\x60 \\|`));
  }
  assert.match(readme, /astro-cms-link-audit --config audit\.config\.json/u);
  assert.match(readme, /"baselinePath":/u);
  assert.match(readme, /"gscPath":/u);
});

test('IL-03 documented compact, full, baseline and imported GSC contracts execute', async t => {
  const dir = await mkdtemp(join(tmpdir(), 'astro-link-docs-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const artifactRoot = join(dir, 'dist');
  await mkdir(artifactRoot);
  await writeFile(join(artifactRoot, 'index.html'), '<title>Home</title>');
  const config = { artifactRoot, site: 'https://example.com', binaryPath: join(root, 'target/release/astro-cms-internal-link-structure'), reportPath: join(dir, 'compact.json') };
  const raw = await auditArtifacts(config);
  assert.equal(raw.schema, '1');
  assert.ok(Array.isArray(raw.nodes));
  const compact = JSON.parse(await readFile(config.reportPath, 'utf8'));
  assert.equal(compact.schema, 'astro-link-summary/1');
  assert.equal(compact.nodes, undefined);
  await writeFile(join(dir, 'gsc.json'), JSON.stringify({ rows: [{ page: 'https://example.com/', clicks: 1, impressions: 100, ctr: 0.01, position: 12 }] }));
  const fullPath = join(dir, 'full.json');
  await auditArtifacts({ ...config, reportPath: join(dir, 'compared.json'), baselinePath: config.reportPath, gscPath: join(dir, 'gsc.json'), gscMinImpressions: 10, fullReportPath: fullPath, summaryPath: join(dir, 'summary.txt') });
  const compared = JSON.parse(await readFile(join(dir, 'compared.json'), 'utf8'));
  assert.equal(compared.comparison.counts.new, 0);
  assert.equal(compared.comparison.counts.resolved, 0);
  assert.equal(compared.gsc.priorities.length, 1);
  await auditArtifacts({ ...config, reportFormat: 'full', reportPath: join(dir, 'explicit-full.json') });
  assert.deepEqual(await readFile(fullPath), await readFile(join(dir, 'explicit-full.json')));
});

test('IL-03 README JSON examples run unchanged apart from selecting the local test engine', async t => {
  const dir = await mkdtemp(join(tmpdir(), 'astro-link-readme-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  await mkdir(join(dir, 'dist'));
  await writeFile(join(dir, 'dist/index.html'), '<title>Home Guide</title>');
  const readme = await readFile(join(root, 'README.md'), 'utf8');
  const examples = [...readme.slice(0, readme.indexOf('## Standalone native CLI')).matchAll(/```json\n([\s\S]*?)\n```/gu)].map(match => JSON.parse(match[1]));
  assert.equal(examples.length, 4);
  const [audit, baseline, gsc, gscConfig] = examples;
  const invoke = async (name, config) => {
    await writeFile(join(dir, name), JSON.stringify({ ...config, binaryPath: join(root, 'target/release/astro-cms-internal-link-structure') }));
    const result = spawnSync(process.execPath, [join(root, 'js/cli.js'), '--config', name], { cwd: dir, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(await readFile(join(dir, config.reportPath), 'utf8'));
  };
  const first = await invoke('audit.config.json', audit);
  assert.equal(first.schema, 'astro-link-summary/1');
  assert.equal(JSON.parse(await readFile(join(dir, audit.fullReportPath), 'utf8')).schema, '1');
  await writeFile(join(dir, baseline.baselinePath), JSON.stringify(first));
  const compared = await invoke('baseline.config.json', baseline);
  assert.deepEqual(compared.comparison.counts, { new: 0, resolved: 0, existing: first.findings.length });
  await writeFile(join(dir, gscConfig.gscPath), JSON.stringify(gsc));
  const prioritized = await invoke('gsc.config.json', gscConfig);
  assert.equal(prioritized.gsc.inputRows, 1);
  assert.equal(prioritized.gsc.priorities.length, 1);
});
