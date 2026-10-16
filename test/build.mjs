import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, copyFile, chmod, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const root = fileURLToPath(new URL('../', import.meta.url));
const engine = join(root, 'target/release/astro-cms-internal-link-structure');
async function sandbox(t, mismatch = false) {
  const base = join(root, 'target/repair-build');
  await mkdir(base, { recursive: true });
  const dir = await mkdtemp(join(base, 'case-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  await mkdir(join(dir, 'scripts'));
  await copyFile(join(root, 'scripts/build.mjs'), join(dir, 'scripts/build.mjs'));
  const name = 'astro-cms-internal-link-structure';
  const target = join(dir, 'alternate-target');
  await mkdir(join(target, 'release'), { recursive: true });
  const fresh = join(target, 'release', name);
  if (mismatch) {
    await writeFile(fresh, `#!${process.execPath}\nprocess.stdin.resume(); process.stdout.write(JSON.stringify({schema:'1',engineVersion:'0.1.0',coverage:{complete:true},gates:{violated:false},nodes:[{path:'/'}],edges:[]}));\n`);
    await chmod(fresh, 0o755);
  } else await copyFile(engine, fresh);
  await mkdir(join(dir, 'target/release'), { recursive: true });
  await writeFile(join(dir, 'target/release', name), 'stale hardcoded engine');
  const destination = join(dir, 'native', `${process.platform}-${process.arch}`, name);
  await mkdir(join(destination, '..'), { recursive: true });
  await writeFile(destination, 'previous bundle');
  const bin = join(dir, 'bin');
  await mkdir(bin);
  await writeFile(join(bin, 'cargo'), `#!${process.execPath}\nimport {join} from 'node:path';\nconst executable=join(process.env.CARGO_TARGET_DIR,'release',${JSON.stringify(name)});\nconsole.log(JSON.stringify({reason:'compiler-artifact',target:{name:${JSON.stringify(name)},kind:['bin']},executable}));\n`);
  await chmod(join(bin, 'cargo'), 0o755);
  const result = spawnSync(process.execPath, [join(dir, 'scripts/build.mjs')], {
    cwd: dir, encoding: 'utf8', env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, CARGO_TARGET_DIR: target },
  });
  return { result, fresh, destination };
}

test('IL-01 bundle is compiler-reported fresh executable with alternate Cargo target', async t => {
  const { result, fresh, destination } = await sandbox(t);
  assert.equal(result.status, 0, result.stderr);
  const hash = bytes => createHash('sha256').update(bytes).digest('hex');
  const expected = hash(await readFile(fresh));
  const actual = hash(await readFile(destination));
  console.log(`compiled SHA256=${expected}; bundled SHA256=${actual}`);
  assert.equal(actual, expected);
});

test('IL-01 capability mismatch fails loudly without replacing the previous bundle', async t => {
  const { result, destination } = await sandbox(t, true);
  assert.notEqual(result.status, 0, result.stdout);
  assert.match(result.stderr, /capabilit|unsupported report/iu);
  assert.equal(await readFile(destination, 'utf8'), 'previous bundle');
});
