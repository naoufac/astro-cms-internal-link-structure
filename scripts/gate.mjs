import { run, evidence } from './support.mjs';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
const commands = [
  ['rust-fmt', 'cargo', ['fmt', '--check']],
  ['rust-test', 'cargo', ['test', '--locked']],
  ['rust-clippy', 'cargo', ['clippy', '--locked', '--all-targets', '--', '-D', 'warnings']],
  ['js-types', 'npm', ['run', 'test:types']],
  ['js-helpers-native-astro', 'npm', ['test']],
  ['packaging-fresh-consumer', process.execPath, ['scripts/packaging.mjs']],
];
const results = [];
for (const [name, command, args] of commands) {
  try {
    const result = run(name, command, args);
    process.stdout.write(result.stdout ?? ''); process.stderr.write(result.stderr ?? '');
    results.push({ name, status: result.status, log: result.log });
  } catch (error) { console.error(error); results.push({ name, status: 1, error: String(error) }); }
}
writeFileSync(join(evidence, 'gate-result.json'), JSON.stringify(results, null, 2) + '\n');
console.log('Gate results:', JSON.stringify(results, null, 2));
if (results.some(result => result.status !== 0)) process.exitCode = 1;
