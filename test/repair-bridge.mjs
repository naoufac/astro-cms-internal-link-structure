import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, chmod, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { auditArtifacts } from '../js/index.js';

const root = fileURLToPath(new URL('../', import.meta.url));
const engine = join(root, 'target/release/astro-cms-internal-link-structure');
const cli = join(root, 'js/cli.js');
async function fixture(t) {
  const dir = await mkdtemp(join(tmpdir(), 'astro-link-repair-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const artifactRoot = join(dir, 'dist');
  await mkdir(artifactRoot);
  await writeFile(join(artifactRoot, 'index.html'), '<title>Home</title><main><a href="/missing/">Broken</a></main>');
  return { artifactRoot, site: 'https://example.test', binaryPath: engine, reportPath: join(dir, 'report.json') };
}

test('IL-02 Unicode chunks use linear byte accounting and preserve exact full JSON bytes', async t => {
  const config = await fixture(t);
  const report = await auditArtifacts(config);
  const bytes = Buffer.from(JSON.stringify({ ...report, padding: 'é月🌒'.repeat(400000) }) + '\n');
  const payload = join(config.artifactRoot, '..', 'payload.json');
  const binaryPath = join(config.artifactRoot, '..', 'chunk-engine');
  await writeFile(payload, bytes);
  await writeFile(binaryPath, `#!${process.execPath}\nimport { createReadStream } from 'node:fs';\nprocess.stdin.resume();\ncreateReadStream(${JSON.stringify(payload)}, { highWaterMark: 4093 }).pipe(process.stdout);\n`);
  await chmod(binaryPath, 0o755);
  const original = Buffer.byteLength;
  let measured = 0;
  Buffer.byteLength = function (value, ...args) {
    const length = original.call(this, value, ...args);
    measured += length;
    return length;
  };
  try {
    const actual = await auditArtifacts({ ...config, binaryPath, reportFormat: 'full' });
    assert.equal(actual.padding, 'é月🌒'.repeat(400000));
    assert.deepEqual(await readFile(config.reportPath), bytes);
    assert.ok(measured <= bytes.length * 3, `remeasured ${measured} bytes for ${bytes.length} bytes of output`);
  } finally {
    Buffer.byteLength = original;
  }
});

test('IL-02 configurable timeout rejects a slow engine and permits a sufficient deadline', async t => {
  const config = await fixture(t);
  const report = await auditArtifacts(config);
  const binaryPath = join(config.artifactRoot, '..', 'slow-engine');
  await writeFile(binaryPath, `#!${process.execPath}\nprocess.stdin.resume();\nsetTimeout(() => process.stdout.write(${JSON.stringify(JSON.stringify(report))}), 200);\n`);
  await chmod(binaryPath, 0o755);
  await assert.rejects(auditArtifacts({ ...config, binaryPath, timeoutMs: 30 }), /timed out/u);
  assert.deepEqual(await auditArtifacts({ ...config, binaryPath, timeoutMs: 5000 }), report);
});

test('IL-06 raw enforce failure retains JSON stdout, exit one, saved report and stderr', async t => {
  const config = { ...await fixture(t), mode: 'enforce' };
  const path = join(config.artifactRoot, '..', 'config.json');
  await writeFile(path, JSON.stringify(config));
  const result = spawnSync(process.execPath, [cli, '--config', path, '--raw'], { encoding: 'utf8' });
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /Internal link thresholds violated; report:/u);
  const report = JSON.parse(result.stdout);
  assert.equal(report.schema, '1');
  assert.equal(report.gates.violated, true);
  assert.equal(JSON.parse(await readFile(config.reportPath, 'utf8')).gates.violated, true);
});
