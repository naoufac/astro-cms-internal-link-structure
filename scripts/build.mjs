import { spawnSync } from 'node:child_process';
import { mkdirSync, copyFileSync, chmodSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
const root = fileURLToPath(new URL('../', import.meta.url));
const result = spawnSync('cargo', ['build', '--release', '--locked', '--message-format=json'], { cwd: root, stdio: ['ignore', 'pipe', 'inherit'], encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, shell: false });
if (result.error) throw result.error;
const messages = result.stdout.trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
for (const message of messages) if (message.reason === 'compiler-message' && message.message?.rendered) process.stderr.write(message.message.rendered);
if (result.status !== 0) process.exit(result.status ?? 1);
const name = `astro-cms-internal-link-structure${process.platform === 'win32' ? '.exe' : ''}`;
const executable = messages.findLast(message => message.reason === 'compiler-artifact' && message.target?.name === 'astro-cms-internal-link-structure' && message.target.kind.includes('bin') && message.executable)?.executable;
if (!executable) throw new Error('Cargo did not report the compiled native executable');
const probe = mkdtempSync(join(tmpdir(), 'astro-link-capabilities-'));
try {
  mkdirSync(join(probe, 'target'));
  writeFileSync(join(probe, 'index.html'), '<title>Home</title><nav><a href="/target/">Menu</a></nav><main><a href="/target/">Target guide</a></main>');
  writeFileSync(join(probe, 'target/index.html'), '<title>Target guide</title><meta name="robots" content="noindex">');
  const check = spawnSync(executable, [], { input: JSON.stringify({ schemaVersion: '1', artifactRoot: probe, site: 'https://example.test', entrypoints: ['/'], thresholds: {} }), encoding: 'utf8', timeout: 30000, maxBuffer: 1024 * 1024, shell: false });
  if (check.error || check.status !== 0) throw new Error(`Native engine capability probe failed: ${check.error?.message ?? check.stderr}`);
  const report = JSON.parse(check.stdout);
  const supported = report.schema === '1' && report.coverage?.complete === true && typeof report.gates?.violated === 'boolean'
    && report.nodes?.length === 2 && report.nodes.every(n => typeof n.metadata?.eligible === 'boolean' && typeof n.metadata?.title === 'string' && typeof n.metadata?.canonicalStatus === 'string' && typeof n.metadata?.noindex === 'boolean' && Number.isInteger(n.editorialSources) && typeof n.editorialUnreachable === 'boolean' && typeof n.editorialOrphan === 'boolean' && (n.editorialDepth === null || Number.isInteger(n.editorialDepth)))
    && report.nodes.find(n => n.path === '/target/')?.metadata.noindex === true
    && report.edges?.length === 2 && report.edges.every(e => typeof e.editorial === 'boolean' && typeof e.anchor === 'string') && report.edges.some(e => e.editorial) && report.edges.some(e => !e.editorial)
    && Array.isArray(report.sitemap?.issues) && Array.isArray(report.sitemap?.files) && Array.isArray(report.anchorFindings);
  if (!supported) throw new Error('Native engine capability mismatch: required metadata/editorial/sitemap/anchor report fields are missing or invalid');
} finally {
  rmSync(probe, { recursive: true, force: true });
}
const destination = join(root, 'native', `${process.platform}-${process.arch}`);
mkdirSync(destination, { recursive: true });
copyFileSync(executable, join(destination, name));
chmodSync(join(destination, name), 0o755);
console.log(`Native engine compiled, capabilities verified and copied from ${executable} to ${join(destination, name)}`);
