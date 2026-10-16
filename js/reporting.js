const schema = 'astro-link-summary/1';
const string = v => typeof v === 'string' && v.length > 0;
const object = v => v !== null && typeof v === 'object' && !Array.isArray(v);
function scope(config) {
  if (!object(config) || !string(config.site) || typeof config.basePath !== 'string' || !['directory', 'file'].includes(config.buildFormat) || !['always', 'never', 'ignore'].includes(config.trailingSlash) || !Array.isArray(config.entrypoints) || !config.entrypoints.length || !config.entrypoints.every(string)) throw new TypeError('Invalid report/baseline config');
  const origin = new URL(config.site);
  if (!['http:', 'https:'].includes(origin.protocol) || origin.origin !== config.site) throw new TypeError('Invalid report/baseline site');
  if (config.originAliases !== undefined && (!Array.isArray(config.originAliases) || !config.originAliases.every(string))) throw new TypeError('Invalid report/baseline aliases');
  return { site: config.site, basePath: config.basePath, buildFormat: config.buildFormat, trailingSlash: config.trailingSlash, originAliases: [...new Set([config.site, ...(config.originAliases ?? [])])].sort(), entrypoints: [...config.entrypoints].sort(), auditScope: config.auditScope ?? { excludedPages: [], hierarchy: null } };
}
function validateRaw(r) {
  if (!object(r) || r.schema !== '1' || !object(r.coverage) || typeof r.coverage.complete !== 'boolean' || !object(r.gates) || typeof r.gates.violated !== 'boolean' || !Array.isArray(r.nodes) || !Array.isArray(r.edges) || !object(r.issues) || !Array.isArray(r.issues.brokenPaths) || !Array.isArray(r.issues.brokenFragments) || !Array.isArray(r.hierarchy?.violations) || !Array.isArray(r.opportunities)) throw new TypeError('Invalid raw report/baseline schema or fields');
  scope(r.config);
  for (const f of r.issues.brokenPaths) if (![f.source, f.href, f.resolved, f.reason].every(string)) throw new TypeError('Invalid broken path report/baseline');
  for (const f of r.issues.brokenFragments) if (![f.source, f.href, f.target, f.fragment, f.reason].every(string)) throw new TypeError('Invalid fragment report/baseline');
}
function identity(rule, source, target, evidence) { return JSON.stringify([rule, source, target, evidence ?? '']); }
function collect(r) {
  const found = new Map();
  function add(rule, severity, source, target, detail, evidence = '') {
    const id = identity(rule, source, target, evidence);
    const f = found.get(id);
    if (f) { f.occurrences++; return; }
    found.set(id, { id, rule, severity, source, target, evidence, detail, occurrences: 1 });
  }
  for (const f of r.issues.brokenPaths) add('broken-path', 'critical', f.source, f.resolved, f.reason);
  for (const f of r.issues.brokenFragments) add('broken-fragment', 'critical', f.source, f.target, f.reason, f.fragment);
  for (const f of r.hierarchy.violations) add(`hierarchy-${f.kind}`, 'warning', f.page, f.related ?? '', f.detail);
  for (const f of r.opportunities) add(`opportunity-${f.kind}`, 'advisory', f.source, f.target, f.detail);
  for (const f of r.anchorFindings ?? []) add(`anchor-${f.kind}`, 'advisory', f.source, f.target, f.detail, f.anchor);
  for (const f of r.sitemap?.issues ?? []) add(`sitemap-${f.kind}`, 'warning', f.url, '', f.detail);
  for (const n of r.nodes) {
    if (n.metadata && !['self', 'absent'].includes(n.metadata.canonicalStatus)) add(`canonical-${n.metadata.canonicalStatus}`, 'warning', n.path, n.metadata.canonicalResolved ?? '', n.metadata.basis);
    if (n.metadata?.noindex) add('noindex', 'review', n.path, '', 'Local HTML declares noindex; confirm intended exclusion');
    if (r.config.entrypoints.includes(n.path)) continue;
    if (n.unreachable) add('full-unreachable', 'warning', n.path, '', 'No full-graph path from configured entrypoints');
    if (n.orphan) add('full-orphan', 'warning', n.path, '', 'No distinct full-graph inbound source');
    if (n.editorialUnreachable) add('editorial-unreachable', 'advisory', n.path, '', 'No editorial-graph path from configured entrypoints');
    if (n.editorialOrphan) add('editorial-orphan', 'advisory', n.path, '', 'No distinct editorial inbound source');
  }
  return [...found.values()].sort((a, b) => a.id.localeCompare(b.id, 'en'));
}
function baselineSummary(baseline) {
  if (baseline?.schema === '1') { validateRaw(baseline); if (!baseline.coverage.complete) throw new TypeError('Baseline is incomplete'); return summarizeReport(baseline); }
  if (!object(baseline) || baseline.schema !== schema || baseline.coverage?.complete !== true || !Array.isArray(baseline.findings)) throw new TypeError('Invalid baseline summary schema');
  scope(baseline.config);
  const ids = new Set();
  for (const f of baseline.findings) {
    if (!object(f) || !string(f.rule) || !['critical', 'warning', 'advisory', 'review'].includes(f.severity) || typeof f.source !== 'string' || typeof f.target !== 'string' || typeof f.evidence !== 'string' || !string(f.detail) || !Number.isSafeInteger(f.occurrences) || f.occurrences < 1 || f.id !== identity(f.rule, f.source, f.target, f.evidence) || ids.has(f.id)) throw new TypeError('Invalid baseline finding');
    ids.add(f.id);
  }
  if (baseline.counts?.findings !== baseline.findings.length || baseline.counts?.occurrences !== baseline.findings.reduce((s, f) => s + f.occurrences, 0)) throw new TypeError('Invalid baseline counts');
  return baseline;
}
export function summarizeReport(report, { baseline, gsc } = {}) {
  validateRaw(report);
  const findings = collect(report);
  let comparison = null;
  if (baseline !== undefined) {
    const before = baselineSummary(baseline);
    if (!report.coverage.complete) throw new TypeError('Current report coverage is incomplete; baseline comparison unavailable');
    if (JSON.stringify(scope(before.config)) !== JSON.stringify(scope(report.config))) throw new TypeError('Baseline scope/config does not match current audit');
    const old = new Map(before.findings.map(f => [f.id, f]));
    const current = new Set(findings.map(f => f.id));
    for (const f of findings) f.status = old.has(f.id) ? 'existing' : 'new';
    const resolved = before.findings.filter(f => !current.has(f.id)).map(f => ({ ...f, status: 'resolved' }));
    comparison = { counts: { new: findings.filter(f => f.status === 'new').length, resolved: resolved.length, existing: findings.filter(f => f.status === 'existing').length }, resolved };
  }
  const editorial = report.edges.filter(e => e.editorial);
  return {
    schema, engineVersion: report.engineVersion, config: report.config, coverage: report.coverage, gates: report.gates,
    counts: { findings: findings.length, occurrences: findings.reduce((s, f) => s + f.occurrences, 0), critical: findings.filter(f => f.severity === 'critical').length, pages: report.nodes.length, fullEdges: report.edges.length, editorialEdges: editorial.length, editorialPairs: new Set(editorial.map(e => JSON.stringify([e.source, e.target]))).size, editorialReachable: report.nodes.filter(n => n.editorialUnreachable === false).length, editorialUnreachable: report.nodes.filter(n => n.editorialUnreachable).length, editorialOrphans: report.nodes.filter(n => n.editorialOrphan).length, editorialMaxDepth: Math.max(0, ...report.nodes.map(n => n.editorialDepth ?? 0)), eligible: report.nodes.filter(n => n.metadata?.eligible).length, missingCanonical: report.nodes.filter(n => n.metadata?.canonicalStatus === 'absent').length },
    findings, comparison,
    issues: { brokenPaths: report.issues.brokenPaths, brokenFragments: report.issues.brokenFragments },
    hierarchy: report.hierarchy, opportunities: report.opportunities, sitemap: report.sitemap,
    ...(gsc === undefined ? {} : { gsc }),
    omitted: ['nodes', 'edges', 'issues.external', 'issues.special'],
    basis: 'Offline artifacts; no live HTTP, Google selected canonical, indexation, ranking or traffic gains verified. All findings retained without truncation.',
  };
}
export function formatSummary(summary) {
  const c = summary.counts;
  const lines = [`Internal link audit: ${summary.config.site}`, `${c.pages} pages; full graph ${c.fullEdges} occurrences; editorial ${c.editorialEdges} occurrences / ${c.editorialPairs} distinct pairs`, `Editorial: ${c.editorialReachable} reachable, ${c.editorialUnreachable} unreachable, ${c.editorialOrphans} no-inbound, max depth ${c.editorialMaxDepth}`, `${c.findings} deduplicated findings / ${c.occurrences} occurrences; ${c.critical} critical; gates ${summary.gates.violated ? 'VIOLATED' : 'passed'}; coverage ${summary.coverage.complete ? 'complete' : 'INCOMPLETE'}`];
  if (summary.comparison) lines.push(`Baseline: ${summary.comparison.counts.new} new, ${summary.comparison.counts.resolved} resolved, ${summary.comparison.counts.existing} existing. Existing critical findings remain below.`);
  for (const f of summary.findings) lines.push(`[${f.severity}${f.status ? ` ${f.status}` : ''}] ${f.rule}: ${f.source}${f.target ? ` -> ${f.target}` : ''} (${f.occurrences} occurrences)${f.evidence ? ` [${f.evidence}]` : ''}: ${f.detail}`);
  if (summary.comparison) for (const f of summary.comparison.resolved) lines.push(`[resolved] ${f.rule}: ${f.source} -> ${f.target}`);
  if (summary.gsc) {
    lines.push(`Imported GSC: ${summary.gsc.inputRows} rows; ${summary.gsc.priorities.length} page reviews. ${summary.gsc.basis}`);
    for (const p of summary.gsc.priorities) lines.push(`[GSC review] ${p.page}: ${p.impressions} impressions, position ${p.position}, ${p.editorialSources} distinct editorial sources; query relevance ${p.queryRelevance}; ${p.reason}`);
  }
  lines.push(summary.basis);
  return lines.join('\n') + '\n';
}
