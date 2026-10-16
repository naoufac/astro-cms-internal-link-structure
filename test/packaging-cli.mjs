import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = fileURLToPath(new URL('../', import.meta.url));
test('IL-04 packed consumer exposes npm bin, JS CLI and native subpath', { timeout: 120000 }, async t => {
  const base = join(root, 'target/repair-consumer');
  await mkdir(base, { recursive: true });
  const dir = await mkdtemp(join(base, 'case-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const run = (command, args, cwd = dir) => {
    const result = spawnSync(command, args, { cwd, encoding: 'utf8', timeout: 90000, maxBuffer: 8 * 1024 * 1024, env: { ...process.env, npm_config_offline: 'true' } });
    assert.equal(result.status, 0, `${command} ${args.join(' ')}\n${result.stdout}\n${result.stderr}`);
    return result.stdout;
  };
  const packed = JSON.parse(run('npm', ['pack', '--offline', '--ignore-scripts', '--pack-destination', dir, '--json'], root));
  await writeFile(join(dir, 'package.json'), JSON.stringify({ name: 'audit-cli-consumer', private: true, type: 'module' }));
  run('npm', ['install', '--offline', '--ignore-scripts', '--legacy-peer-deps', '--no-audit', '--no-fund', join(dir, packed[0].filename)]);
  await mkdir(join(dir, 'dist'));
  await writeFile(join(dir, 'dist/index.html'), '<title>Home</title>');
  const config = join(dir, 'audit.config.json');
  await writeFile(config, JSON.stringify({ artifactRoot: join(dir, 'dist'), site: 'https://example.com', reportPath: join(dir, 'report.json') }));
  const stdout = run('npm', ['exec', '--offline', '--', 'astro-cms-link-audit', '--config', config]);
  assert.match(stdout, /Internal link audit: https:\/\/example\.com/u);
  const resolved = run(process.execPath, ['--input-type=module', '-e', `console.log(import.meta.resolve('astro-cms-internal-link-structure/js/cli.js')); console.log(import.meta.resolve('astro-cms-internal-link-structure/native/${process.platform}-${process.arch}/astro-cms-internal-link-structure'));`]);
  assert.match(resolved, /\/js\/cli\.js/u);
  assert.match(resolved, /\/native\//u);
});
