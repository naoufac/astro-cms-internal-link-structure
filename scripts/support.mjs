import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
export const evidence = process.env.EVIDENCE_DIR ?? join(tmpdir(), 'astro-link-integration-evidence');
mkdirSync(evidence, { recursive: true });
let counter = 0;
export function run(name, command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', maxBuffer: 128 * 1024 * 1024, timeout: 600_000, shell: false, ...options, env: { ...process.env, ASTRO_TELEMETRY_DISABLED: '1', ...options.env } });
  const log = join(evidence, `${process.pid}-${++counter}-${name}.log`);
  writeFileSync(log, `$ ${JSON.stringify([command, ...args])}\ncwd: ${options.cwd ?? process.cwd()}\n${result.stdout ?? ''}${result.stderr ?? ''}\nexit: ${result.status}; signal: ${result.signal}; error: ${result.error ?? ''}\n`);
  console.log(`${name}: exit ${result.status}; log ${log}`);
  if (result.error) throw result.error;
  return { ...result, log };
}
