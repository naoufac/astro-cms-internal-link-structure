import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { resolve, dirname, relative, isAbsolute, join } from 'node:path';
import { mkdir, writeFile, realpath, readFile } from 'node:fs/promises';
import { defineStructure } from './structure.js';
import { summarizeReport, formatSummary } from './reporting.js';
import { prioritizeGsc } from './gsc.js';
export { summarizeReport, formatSummary, prioritizeGsc };
export { defineStructure, parentOf, childrenOf, breadcrumbsOf } from './structure.js';

export class AuditError extends Error {
  constructor(message, { report, exitCode, cause } = {}) {
    super(message, { cause }); this.name = 'AuditError'; this.report = report; this.exitCode = exitCode;
  }
}
const fields = new Set(['artifactRoot', 'site', 'originAliases', 'basePath', 'trailingSlash', 'buildFormat', 'mode', 'entrypoints', 'exclude', 'excludePagePaths', 'hierarchy', 'thresholds', 'reportPath', 'binaryPath', 'timeoutMs', 'reportFormat', 'fullReportPath', 'summaryPath', 'baselinePath', 'gscPath', 'gscMinImpressions', 'publicDir']);
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
  for (const key of ['artifactRoot', 'reportPath', 'binaryPath', 'fullReportPath', 'summaryPath', 'baselinePath', 'gscPath', 'publicDir']) if (result[key] !== undefined) text(result[key], key);
  if (result.reportFormat !== undefined && !['compact', 'full'].includes(result.reportFormat)) throw new TypeError('reportFormat must be compact or full');
  if (result.gscMinImpressions !== undefined && (!Number.isSafeInteger(result.gscMinImpressions) || result.gscMinImpressions < 1)) throw new TypeError('gscMinImpressions must be a positive integer');
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
    const stdout = [], stderr = [];
    let stdoutSize = 0, stderrSize = 0, failure;
    const timer = setTimeout(() => { failure = new AuditError('Native engine timed out'); child.kill('SIGKILL'); }, timeout);
    const limit = 128 * 1024 * 1024;
    child.stdout.on('data', chunk => {
      if (failure) return;
      stdoutSize += chunk.length;
      if (stdoutSize > limit) { failure = new AuditError('Native report exceeds 128 MiB'); child.kill('SIGKILL'); }
      else stdout.push(chunk);
    });
    child.stderr.on('data', chunk => {
      const kept = chunk.subarray(0, Math.max(0, 1024 * 1024 - stderrSize));
      if (kept.length) { stderr.push(kept); stderrSize += kept.length; }
    });
    child.once('error', cause => { clearTimeout(timer); reject(new AuditError(`Cannot execute native engine at ${binary}: ${cause.message}. Run npm run build from source or supply a compatible binaryPath.`, { cause })); });
    child.stdin.on('error', error => { if (error.code !== 'EPIPE') failure = error; });
    child.once('close', (exitCode, signal) => {
      clearTimeout(timer);
      if (failure) return reject(failure);
      if (![0, 1].includes(exitCode)) return reject(new AuditError(`Native engine failed (${exitCode ?? signal}): ${Buffer.concat(stderr, stderrSize).toString('utf8').trim()}`, { exitCode }));
      const bytes = Buffer.concat(stdout, stdoutSize).toString('utf8');
      try { accept({ exitCode, report: JSON.parse(bytes), bytes }); }
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
  const summaryPath = resolve(config.summaryPath ?? `${reportPath}.txt`);
  const outputs = [reportPath, summaryPath, ...(config.fullReportPath ? [resolve(config.fullReportPath)] : [])];
  const targets = [];
  for (const path of outputs) {
    await outside(path, [root, config.publicDir ?? join(dirname(root), 'public')]);
    const target = await canonical(path);
    if (targets.includes(target)) throw new TypeError('Output paths must be distinct');
    targets.push(target);
  }
  for (const path of [config.baselinePath, config.gscPath, config.binaryPath].filter(Boolean)) if (targets.includes(await canonical(resolve(path)))) throw new TypeError('Output paths must not overwrite inputs');
  const baseline = config.baselinePath ? JSON.parse(await readFile(resolve(config.baselinePath), 'utf8')) : undefined;
  const gscInput = config.gscPath ? JSON.parse(await readFile(resolve(config.gscPath), 'utf8')) : undefined;
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
  const gsc = gscInput === undefined ? undefined : prioritizeGsc(report, gscInput, { minImpressions: config.gscMinImpressions });
  const summary = summarizeReport(report, { baseline, gsc });
  for (const path of outputs) await mkdir(dirname(path), { recursive: true });
  await writeFile(reportPath, config.reportFormat === 'full' ? bytes : JSON.stringify(summary, null, 2) + '\n', 'utf8');
  await writeFile(summaryPath, formatSummary(summary), 'utf8');
  if (config.fullReportPath) await writeFile(resolve(config.fullReportPath), bytes, 'utf8');
  if (!report.coverage.complete) throw new AuditError(`Native scan incomplete; report: ${reportPath}`, { report, exitCode });
  if (exitCode === 1 || (request.mode === 'enforce' && report.gates.violated)) throw new AuditError(`Internal link thresholds violated; report: ${reportPath}`, { report, exitCode: 1 });
  return report;
}
export function internalLinkStructure(input = {}) {
  const config = options(input);
  if (config.artifactRoot !== undefined) throw new TypeError('Astro artifactRoot is supplied by build:done, not integration options');
  if (config.publicDir !== undefined) throw new TypeError('Astro publicDir is supplied by config:done');
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
        const paths = { reportPath, summaryPath: resolve(root, config.summaryPath ?? `${reportPath}.txt`) };
        for (const key of ['fullReportPath', 'baselinePath', 'gscPath']) if (config[key]) paths[key] = resolve(root, config[key]);
        for (const key of ['reportPath', 'summaryPath', 'fullReportPath']) if (paths[key]) await outside(paths[key], [fileURLToPath(astro.outDir), publicDir]);
        captured = { ...config, ...values, ...paths, publicDir };
      },
      'astro:build:done': async ({ dir, logger }) => {
        if (!captured) throw new Error('Astro config:done did not run');
        const audit = captured;
        const { publicDir } = audit;
        const artifactRoot = fileURLToPath(dir);
        await outside(audit.reportPath, [artifactRoot, publicDir]);
        const report = await auditArtifacts({ ...audit, artifactRoot });
        const persisted = JSON.parse(await readFile(audit.reportPath, 'utf8'));
        logger.info(formatSummary(persisted.schema === 'astro-link-summary/1' ? persisted : summarizeReport(report)) + `Report: ${audit.reportPath}; summary: ${audit.summaryPath}`);
      },
    },
  };
}
export default internalLinkStructure;
