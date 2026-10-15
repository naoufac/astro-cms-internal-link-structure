import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, copyFile, chmod } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { auditArtifacts, AuditError } from '../js/index.js';
import { run } from '../scripts/support.mjs';
const binary = fileURLToPath(new URL(`../native/${process.platform}-${process.arch}/astro-cms-internal-link-structure`, import.meta.url));
async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'astro-link-native-'));
  const artifactRoot = join(root, 'dist'); await mkdir(artifactRoot);
  await writeFile(join(artifactRoot, 'index.html'), '<title>Home</title><h1 id="present">Home</h1><a href="/absent/">Bad</a><a href="#missing">Bad fragment</a>');
  return { artifactRoot, site: 'https://example.test', reportPath: join(root, 'reports', 'report.json') };
}
test('real native bridge: audit succeeds, enforce writes report before rejecting, repeated bytes deterministic', async () => {
  const config = await fixture();
  const report = await auditArtifacts(config);
  assert.equal(report.coverage.complete, true); assert.equal(report.gates.violated, true);
  assert.equal(report.issues.brokenPaths.length, 1); assert.equal(report.issues.brokenFragments.length, 1);
  const first = await readFile(config.reportPath);
  assert.deepEqual(await auditArtifacts(config), report);
  assert.deepEqual(await readFile(config.reportPath), first);
  await assert.rejects(auditArtifacts({ ...config, mode: 'enforce' }), error => {
    assert.ok(error instanceof AuditError); assert.equal(error.exitCode, 1); assert.equal(error.report.gates.violated, true); return true;
  });
  assert.equal(JSON.parse(await readFile(config.reportPath, 'utf8')).gates.mode, 'enforce');
});
test('engine request/response passed through exactly, CLI exit codes remain 0/1/2', async () => {
  const config = await fixture();
  const request = { schemaVersion: '1', artifactRoot: config.artifactRoot, site: config.site, originAliases: [], basePath: '', trailingSlash: 'ignore', buildFormat: 'directory', mode: 'audit', entrypoints: ['/'], exclude: [], excludePagePaths: [], hierarchy: null, thresholds: { brokenPaths: 0, brokenFragments: 0, hierarchyViolations: 0 } };
  const direct = run('native-direct-audit', binary, [], { input: JSON.stringify(request) });
  assert.equal(direct.status, 0, direct.stderr);
  assert.deepEqual(await auditArtifacts(config), JSON.parse(direct.stdout));
  assert.equal(await readFile(config.reportPath, 'utf8'), direct.stdout);
  assert.equal(run('native-direct-enforce', binary, [], { input: JSON.stringify({ ...request, mode: 'enforce' }) }).status, 1);
  for (const invalid of ['not json', JSON.stringify({ ...request, schemaVersion: '2' }), JSON.stringify({ ...request, buildFormat: 'preserve' }), JSON.stringify({ ...request, unknown: true })]) {
    assert.equal(run('native-invalid-config', binary, [], { input: invalid }).status, 2);
  }
});
test('missing binary, invalid config, missing root and incomplete scans fail even in audit mode', async () => {
  const config = await fixture();
  await assert.rejects(auditArtifacts({ ...config, binaryPath: '/nonexistent/engine' }), /Cannot execute native engine/u);
  await assert.rejects(auditArtifacts({ ...config, artifactRoot: join(config.artifactRoot, 'missing') }), /Native engine failed/u);
  await assert.rejects(auditArtifacts({ ...config, reportPath: join(config.artifactRoot, 'report.json') }), /outside/u);
  await assert.rejects(auditArtifacts({ ...config, thresholds: { brokenPaths: -1 } }), /threshold/u);
  await writeFile(join(config.artifactRoot, 'index.html'), Buffer.from([0xff, 0xfe]));
  await assert.rejects(auditArtifacts(config), /incomplete|failed/iu);
});
test('subprocess binary path containing spaces and shell metacharacters is an executable path, never a shell command', async () => {
  const config = await fixture();
  const unusual = join(config.artifactRoot, '..', 'engine space;not-a-command');
  await copyFile(binary, unusual); await chmod(unusual, 0o755);
  assert.equal((await auditArtifacts({ ...config, binaryPath: unusual })).issues.brokenPaths.length, 1);
});
