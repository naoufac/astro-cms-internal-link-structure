function fail(message) { throw new TypeError(`GSC import: ${message}`); }
function host(value) { return value.toLowerCase().replace(/^www\./u, ''); }
const HEX = new Map([...Array(16).keys()].flatMap(v => [[48 + v, v], [87 + v, v], [55 + v, v]]));
const unreserved = b => (b >= 48 && b <= 57) || (b >= 65 && b <= 90) || (b >= 97 && b <= 122) || b === 45 || b === 46 || b === 95 || b === 126;
function pathIdentity(path) {
  const bytes = Buffer.from(path, 'utf8');
  let out = '';
  for (let i = 0; i < bytes.length;) {
    const b = bytes[i];
    if (b === 37 && i + 2 < bytes.length && HEX.has(bytes[i + 1]) && HEX.has(bytes[i + 2])) {
      const v = HEX.get(bytes[i + 1]) * 16 + HEX.get(bytes[i + 2]);
      if (unreserved(v)) out += String.fromCharCode(v);
      else out += `%${String.fromCharCode(bytes[i + 1]).toUpperCase()}${String.fromCharCode(bytes[i + 2]).toUpperCase()}`;
      i += 3;
      continue;
    }
    if (unreserved(b) || b === 47) out += String.fromCharCode(b);
    else if (b === 37) out += '%25';
    else out += `%${b.toString(16).toUpperCase().padStart(2, '0')}`;
    i += 1;
  }
  return out;
}
function calendarDate(token) {
  if (!/^\d{4}-\d{2}-\d{2}$/u.test(token)) return false;
  const [y, m, d] = token.split('-').map(Number);
  const leap = y % 4 === 0 && (y % 100 !== 0 || y % 400 === 0);
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return m >= 1 && m <= 12 && d >= 1 && d <= days[m - 1];
}
function metric(row) {
  for (const name of ['clicks', 'impressions', 'ctr', 'position']) if (typeof row[name] !== 'number' || !Number.isFinite(row[name]) || row[name] < 0) fail(`invalid ${name}`);
  for (const name of ['clicks', 'impressions']) if (!Number.isSafeInteger(row[name])) fail(`${name} must be a nonnegative integer count`);
  if (row.ctr > 1 || row.clicks > row.impressions || (row.impressions > 0 && row.position < 1)) fail('inconsistent metrics');
}
function parse(input) {
  if (!input || typeof input !== 'object' || Array.isArray(input)) fail('expected an object with rows');
  let dimensions = input.dimensions;
  if (input.kind !== undefined) {
    if (!['page', 'query', 'page-query'].includes(input.kind)) fail('unsupported wrapper kind');
    if (dimensions !== undefined) fail('do not combine kind and dimensions');
    dimensions = input.kind === 'page-query' ? ['page', 'query'] : [input.kind];
    if (typeof input.domain !== 'string' || !input.domain.trim()) fail('wrapper domain required');
    for (const date of [input.start, input.end]) if (typeof date !== 'string' || !calendarDate(date)) fail('wrapper start/end dates required');
    if (input.start > input.end) fail('reversed date range');
  }
  if (dimensions !== undefined && (!Array.isArray(dimensions) || !dimensions.length || dimensions.length > 2 || new Set(dimensions).size !== dimensions.length || dimensions.some(d => !['page', 'query'].includes(d)))) fail('raw dimensions must explicitly contain page and/or query only');
  const rows = input.result?.rows ?? input.rows;
  if (!Array.isArray(rows)) fail('rows array required, including for empty imports');
  const seen = new Set();
  const parsed = rows.map(row => {
    if (!row || typeof row !== 'object' || Array.isArray(row)) fail('invalid row');
    metric(row);
    let page, query;
    if ('keys' in row) {
      if (!dimensions || !Array.isArray(row.keys) || row.keys.length !== dimensions.length || row.keys.some(k => typeof k !== 'string' || !k.trim()) || 'page' in row || 'query' in row) fail('raw keys require explicit matching dimensions; cannot mix neutral fields');
      page = row.keys[dimensions.indexOf('page')]; query = row.keys[dimensions.indexOf('query')];
    } else {
      if (dimensions) fail('dimensional raw exports require keys');
      ({ page, query } = row);
      if (typeof page !== 'string' || !page.trim() || (query !== undefined && (typeof query !== 'string' || !query.trim()))) fail('neutral rows require page and optional query');
    }
    const key = JSON.stringify([page, query]);
    if (seen.has(key)) fail('duplicate dimensional row; aggregate exports before importing');
    seen.add(key);
    return { ...row, page, query };
  });
  return { rows: parsed, domain: input.domain, dimensions, start: input.start, end: input.end };
}
function pageKey(url, report) {
  let path;
  try { path = pathIdentity(url.pathname); } catch { fail('invalid page encoding'); }
  const base = report.config.basePath ?? '';
  if (base && path !== base && !path.startsWith(`${base}/`)) return undefined;
  const candidates = new Set([path, path === '/' ? '/' : `${path.replace(/\/+$/u, '')}/`, path.replace(/\/+$/u, '') || '/']);
  if (report.config.buildFormat === 'file') {
    candidates.add(path.replace(/\.html$/u, ''));
    if (path.endsWith('/index.html')) candidates.add(path.slice(0, -11) || '/');
  }
  return report.nodes.find(n => candidates.has(n.path));
}
export function prioritizeGsc(report, input, { minImpressions = 10 } = {}) {
  if (!Number.isSafeInteger(minImpressions) || minImpressions < 1) fail('minImpressions must be a positive integer');
  const parsed = parse(input);
  const allowed = new Set([report.config.site, ...(report.config.originAliases ?? [])].map(s => host(new URL(s).hostname)));
  if (parsed.domain !== undefined && (typeof parsed.domain !== 'string' || !allowed.has(host(parsed.domain.replace(/^sc-domain:/u, ''))))) fail('wrong domain');
  const groups = new Map(), queryOnly = [];
  for (const row of parsed.rows) {
    if (row.page === undefined) { queryOnly.push(row); continue; }
    let url;
    try { url = new URL(row.page); } catch { fail('invalid page URL'); }
    if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.hash || !allowed.has(host(url.hostname))) fail('wrong domain or invalid page URL');
    const node = pageKey(url, report);
    const key = node?.path ?? url.href;
    if (!groups.has(key)) groups.set(key, { page: key, node, rows: [] });
    groups.get(key).rows.push(row);
  }
  const priorities = [], missingArtifacts = [], excluded = [];
  for (const group of groups.values()) {
    if (group.rows.some(r => r.query === undefined) && group.rows.some(r => r.query !== undefined)) fail('cannot combine page totals with query-page rows for the same page');
    const impressions = group.rows.reduce((s, r) => s + r.impressions, 0);
    const clicks = group.rows.reduce((s, r) => s + r.clicks, 0);
    const position = impressions ? group.rows.reduce((s, r) => s + r.position * r.impressions, 0) / impressions : 0;
    const queries = group.rows.filter(r => r.query !== undefined).map(r => ({ query: r.query, impressions: r.impressions, position: r.position }));
    const title = group.node?.metadata?.title?.toLowerCase() ?? '';
    const relevantQueries = queries.filter(q => { const tokens = q.query.toLowerCase().match(/[\p{L}\p{N}]+/gu) ?? []; const titleTokens = new Set(title.match(/[\p{L}\p{N}]+/gu) ?? []); return tokens.length > 0 && tokens.every(t => titleTokens.has(t)); });
    const queryRelevance = queries.length ? (relevantQueries.length ? 'title-match' : 'no-title-match') : 'unknown';
    const editorialSources = new Set(report.edges.filter(e => e.editorial && !e.rel?.includes('nofollow') && e.source !== group.page && e.target === group.page).map(e => e.source)).size;
    const item = { page: group.page, impressions, clicks, ctr: impressions ? clicks / impressions : 0, position, queryRelevance, queries, relevantQueries, editorialSources };
    if (!group.node) { if (impressions > 0) missingArtifacts.push({ ...item, reason: 'Missing local artifact; HTTP status unknown, not a verified 404' }); continue; }
    if (!group.node.metadata?.eligible) { excluded.push({ ...item, reason: 'Local noindex/canonical ineligibility or unavailable metadata' }); continue; }
    if (impressions < minImpressions || position < 8 || position > 20) { excluded.push({ ...item, reason: 'Below minimum impressions or outside review position range 8-20' }); continue; }
    const score = impressions * (21 - position) * (relevantQueries.length ? 1.5 : 1) / (1 + editorialSources);
    priorities.push({ ...item, score, reason: 'Review contextual support and page suitability; imported historical measurements are not predicted gains' });
  }
  priorities.sort((a, b) => b.score - a.score || a.page.localeCompare(b.page, 'en'));
  return { inputRows: parsed.rows.length, queryOnlyRows: queryOnly.length, priorities, missingArtifacts, excluded, minImpressions, positionRange: [8, 20], ...(parsed.start ? { start: parsed.start, end: parsed.end } : {}), basis: 'Imported data only. Page-only query relevance is unknown. Query-only rows are never joined to page exports. No network, verified current rankings or traffic gains.' };
}
