import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { resolve, dirname, relative, isAbsolute, join } from 'node:path';
import { mkdir, writeFile, realpath } from 'node:fs/promises';
import { defineStructure } from './structure.js';
export { defineStructure, parentOf, childrenOf, breadcrumbsOf } from './structure.js';

export class AuditError extends Error {
  constructor(message, { report, exitCode, cause } = {}) {
    super(message, { cause }); this.name = 'AuditError'; this.report = report; this.exitCode = exitCode;
  }
}
const fields = new Set(['artifactRoot', 'site', 'originAliases', 'basePath', 'trailingSlash', 'buildFormat', 'mode', 'entrypoints', 'exclude', 'excludePagePaths', 'hierarchy', 'thresholds', 'reportPath', 'binaryPath', 'timeoutMs']);
function data(value, name) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || ![null, Object.prototype].includes(Object.getPrototypeOf(value))) throw new TypeError(`${name} must be a plain object`);
  for (const key of Reflect.ownKeys(value)) if (typeof key !== 'string' || !('value' in Object.getOwnPropertyDescriptor(value, key))) throw new TypeError(`${name} must contain data properties only`);
}
function text(value, name) {
  if (typeof value !== 'string' || !value.trim() || value.includes('\0')) throw new TypeError(`${name} must be a nonempty string`);
  return value;
}
function origin(value) {
  text(value, 'site/originAlias');
  const url = new URL(value);
  if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password || url.pathname !== '/' || url.search || url.hash) throw new TypeError('site and originAliases must be bare HTTP(S) origins; configure basePath separately');
  return url.origin;
}
function base(value) {
  if (typeof value !== 'string' || (value && !value.startsWith('/')) || /[?#\\\s]/u.test(value) || value.startsWith('//') || value.split('/').some(p => p === '.' || p === '..')) throw new TypeError('Invalid basePath');
  return value.replace(/\/+$/u, '');
}
function options(input) {
  data(input, 'options');
  for (const key of Object.keys(input)) if (!fields.has(key)) throw new TypeError(`Unknown option: ${key}`);
  const result = { ...input };
  for (const key of ['artifactRoot', 'reportPath', 'binaryPath']) if (result[key] !== undefined) text(result[key], key);
  if (result.site !== undefined) result.site = origin(result.site);
  if (result.basePath !== undefined) result.basePath = base(result.basePath);
  for (const [key, valid] of [['mode', ['audit', 'enforce']], ['buildFormat', ['directory', 'file']], ['trailingSlash', ['always', 'never', 'ignore']]]) {
    if (result[key] !== undefined && !valid.includes(result[key])) throw new TypeError(`Unsupported ${key}: ${result[key]}`);
  }
  for (const key of ['originAliases', 'entrypoints', 'exclude', 'excludePagePaths']) {
    if (result[key] === undefined) continue;
    if (!Array.isArray(result[key]) || result[key].some(v => typeof v !== 'string' || !v.trim() || v.includes('\0'))) throw new TypeError(`${key} must be an array of nonempty strings`);
    result[key] = [...result[key]];
    if (key === 'originAliases') result[key] = result[key].map(origin);
  }
  if (result.entrypoints?.length === 0) throw new TypeError('entrypoints cannot be empty');
  if (result.hierarchy !== undefined) result.hierarchy = defineStructure(result.hierarchy);
  if (result.thresholds !== undefined) {
    data(result.thresholds, 'thresholds');
    const names = ['brokenPaths', 'brokenFragments', 'orphans', 'unreachable', 'maxDepth', 'hierarchyViolations'];
    for (const [key, value] of Object.entries(result.thresholds)) if (!names.includes(key) || !Number.isSafeInteger(value) || value < 0) throw new TypeError(`Invalid threshold: ${key}`);
    result.thresholds = { ...result.thresholds };
  }
  if (result.timeoutMs !== undefined && (!Number.isSafeInteger(result.timeoutMs) || result.timeoutMs <= 0)) throw new TypeError('timeoutMs must be a positive integer');
  return result;
}
async function canonical(path) {
  try { return await realpath(path); }
  catch (error) {
    if (error.code !== 'ENOENT') throw error;
    const parent = dirname(path);
    if (parent === path) throw error;
    return join(await canonical(parent), path.slice(parent.length + (parent.endsWith('/') ? 0 : 1)));
  }
}
async function outside(reportPath, roots) {
  const target = await canonical(resolve(reportPath));
  for (const root of roots) {
    const rel = relative(await canonical(resolve(root)), target);
    if (rel === '' || (!rel.startsWith(`..${process.platform === 'win32' ? '\\' : '/'}`) && rel !== '..' && !isAbsolute(rel))) throw new TypeError('reportPath must be outside generated output and public directories');
  }
}
function execute(binary, request, timeout) {
  return new Promise((accept, reject) => {
    const child = spawn(binary, [], { shell: false, stdio: ['pipe', 'pipe', 'pipe'] });
    child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8');
    let stdout = '', stderr = '', failure;
    const timer = setTimeout(() => { failure = new AuditError('Native engine timed out'); child.kill('SIGKILL'); }, timeout);
    const limit = 128 * 1024 * 1024;
    child.stdout.on('data', chunk => { stdout += chunk; if (Buffer.byteLength(stdout) > limit) { failure = new AuditError('Native report exceeds 128 MiB'); child.kill('SIGKILL'); } });
    child.stderr.on('data', chunk => { if (stderr.length < 1024 * 1024) stderr += chunk; });
    child.once('error', cause => { clearTimeout(timer); reject(new AuditError(`Cannot execute native engine at ${binary}: ${cause.message}. Run npm run build from source or supply a compatible binaryPath.`, { cause })); });
    child.stdin.on('error', error => { if (error.code !== 'EPIPE') failure = error; });
    child.once('close', (exitCode, signal) => {
      clearTimeout(timer);
      if (failure) return reject(failure);
      if (![0, 1].includes(exitCode)) return reject(new AuditError(`Native engine failed (${exitCode ?? signal}): ${stderr.trim()}`, { exitCode }));
      try { accept({ exitCode, report: JSON.parse(stdout), bytes: stdout }); }
      catch (cause) { reject(new AuditError('Native engine returned invalid report JSON', { cause, exitCode })); }
    });
    child.stdin.end(JSON.stringify(request));
  });
}
export async function auditArtifacts(input) {
  const config = options(input);
  if (!config.artifactRoot || !config.site) throw new TypeError('artifactRoot and site are required');
  const root = resolve(config.artifactRoot);
  const reportPath = resolve(config.reportPath ?? join(dirname(root), '.astro-link-reports', 'report.json'));
  await outside(reportPath, [root]);
  const request = {
    schemaVersion: '1', artifactRoot: root, site: config.site,
    originAliases: config.originAliases ?? [], basePath: config.basePath ?? '',
    trailingSlash: config.trailingSlash ?? 'ignore', buildFormat: config.buildFormat ?? 'directory',
    mode: config.mode ?? 'audit', entrypoints: config.entrypoints ?? [(config.basePath ?? '') + (config.buildFormat === 'file' && config.basePath ? '' : '/')],
    exclude: config.exclude ?? [], excludePagePaths: config.excludePagePaths ?? [],
    hierarchy: config.hierarchy ?? null,
    thresholds: config.thresholds ?? { brokenPaths: 0, brokenFragments: 0, hierarchyViolations: 0 },
  };
  const binary = config.binaryPath ? resolve(config.binaryPath) : fileURLToPath(new URL(`../native/${process.platform}-${process.arch}/astro-cms-internal-link-structure${process.platform === 'win32' ? '.exe' : ''}`, import.meta.url));
  const { report, exitCode, bytes } = await execute(binary, request, config.timeoutMs ?? 120_000);
  if (report?.schema !== '1' || typeof report?.gates?.violated !== 'boolean' || typeof report?.coverage?.complete !== 'boolean') throw new AuditError('Native engine returned an unsupported report');
  await mkdir(dirname(reportPath), { recursive: true });
  await writeFile(reportPath, bytes, 'utf8');
  if (!report.coverage.complete) throw new AuditError(`Native scan incomplete; report: ${reportPath}`, { report, exitCode });
  if (exitCode === 1 || (request.mode === 'enforce' && report.gates.violated)) throw new AuditError(`Internal link thresholds violated; report: ${reportPath}`, { report, exitCode: 1 });
  return report;
}
export function internalLinkStructure(input = {}) {
  const config = options(input);
  if (config.artifactRoot !== undefined) throw new TypeError('Astro artifactRoot is supplied by build:done, not integration options');
  let captured;
  return {
    name: 'astro-cms-internal-link-structure',
    hooks: {
      'astro:config:done': async ({ config: astro }) => {
        if (astro.output !== 'static') throw new TypeError('Only static Astro output is supported; server/SSR output is unsupported');
        if (!['directory', 'file'].includes(astro.build.format)) throw new TypeError('Astro build.format preserve is unsupported');
        if (!astro.site) throw new TypeError('Astro site is required');
        const values = { site: origin(astro.site), basePath: base(astro.base), trailingSlash: astro.trailingSlash, buildFormat: astro.build.format };
        for (const [key, value] of Object.entries(values)) if (config[key] !== undefined && config[key] !== value) throw new TypeError(`Conflicting ${key}: use the Astro URL configuration`);
        const root = fileURLToPath(astro.root);
        const reportPath = resolve(root, config.reportPath ?? '.astro-link-reports/report.json');
        const publicDir = fileURLToPath(astro.publicDir);
        await outside(reportPath, [fileURLToPath(astro.outDir), publicDir]);
        captured = { ...config, ...values, reportPath, publicDir };
      },
      'astro:build:done': async ({ dir, logger }) => {
        if (!captured) throw new Error('Astro config:done did not run');
        const { publicDir, ...audit } = captured;
        const artifactRoot = fileURLToPath(dir);
        await outside(audit.reportPath, [artifactRoot, publicDir]);
        const report = await auditArtifacts({ ...audit, artifactRoot });
        logger.info(`Audited ${report.coverage.scannedPages} pages; thresholds ${report.gates.violated ? 'violated (audit mode)' : 'passed'}; report: ${audit.reportPath}`);
      },
    },
  };
}
export default internalLinkStructure;
