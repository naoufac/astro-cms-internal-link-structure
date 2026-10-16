import test from 'node:test';
import assert from 'node:assert/strict';
import { prioritizeGsc } from '../js/gsc.js';
const report = { config: { site: 'https://example.com', basePath: '', buildFormat: 'directory' }, nodes: [{ path: '/guide/', metadata: { title: 'Unique Guide', eligible: true } }, { path: '/private/', metadata: { title: 'Private', eligible: false } }], edges: [{ source: '/', target: '/guide/', editorial: true }, { source: '/', target: '/guide/', editorial: true }, { source: '/menu/', target: '/guide/', editorial: false }] };
const metrics = { clicks: 0, impressions: 20, ctr: 0, position: 12 };
test('GSC page wrapper is useful but query relevance unknown, distinct sources', () => {
  const gsc = prioritizeGsc(report, { domain: 'example.com', kind: 'page', start: '2026-01-01', end: '2026-02-01', result: { rows: [{ keys: ['https://example.com/guide'], ...metrics }] } });
  assert.equal(gsc.priorities.length, 1);
  assert.equal(gsc.priorities[0].queryRelevance, 'unknown');
  assert.equal(gsc.priorities[0].editorialSources, 1);
});
test('GSC pair rows use actual query, noindex/low impressions excluded, missing is not HTTP404', () => {
  const gsc = prioritizeGsc(report, { rows: [{ page: 'https://example.com/guide', query: 'unique guide', ...metrics }, { page: 'https://example.com/private', ...metrics }, { page: 'https://example.com/missing', ...metrics }] });
  assert.equal(gsc.priorities[0].queryRelevance, 'title-match');
  assert.equal(gsc.priorities.length, 1);
  assert.match(gsc.missingArtifacts[0].reason, /HTTP status unknown/u);
  assert.equal(prioritizeGsc(report, { rows: [{ page: 'https://example.com/guide', ...metrics, impressions: 1 }] }).priorities.length, 0);
  assert.equal(prioritizeGsc(report, { rows: [] }).priorities.length, 0);
  assert.equal(prioritizeGsc(report, { dimensions: ['query'], rows: [{ keys: ['unique guide'], ...metrics }] }).priorities.length, 0);
});
test('GSC rejects unknown dimensionality, bad metrics, wrong domain and malformed input', () => {
  for (const value of [{}, { rows: [{ keys: ['https://example.com/guide'], ...metrics }] }, { rows: [{ page: 'https://else.test/guide', ...metrics }] }, { rows: [{ page: 'https://example.com/guide', ...metrics, impressions: -1 }] }, { dimensions: ['date'], rows: [] }, { dimensions: ['page', 'query'], rows: [{ keys: ['one'], ...metrics }] }]) assert.throws(() => prioritizeGsc(report, value), /GSC/u);
});

test('native percent-encoded Unicode GSC page matches its report node path', () => {
  const native = { config: { site: 'https://example.com', originAliases: ['https://example.com'], basePath: '', buildFormat: 'directory', trailingSlash: 'ignore', entrypoints: ['/'] }, nodes: [{ path: '/', metadata: { title: 'Home', eligible: true } }, { path: '/caf%C3%A9/', metadata: { title: 'Café', eligible: true } }], edges: [] };
  const gsc = prioritizeGsc(native, { rows: [{ page: 'https://example.com/caf%C3%A9/', ...metrics }] });
  assert.equal(gsc.missingArtifacts.length, 0);
  assert.equal(gsc.priorities[0].page, '/caf%C3%A9/');
});

test('GSC wrapper rejects impossible calendar dates and keeps leap days', () => {
  const rows = { domain: 'example.com', kind: 'page', result: { rows: [{ keys: ['https://example.com/guide'], ...metrics }] } };
  for (const [start, end] of [['2026-02-30', '2026-03-02'], ['2026-04-31', '2026-05-01'], ['2023-02-29', '2023-03-01'], ['2026-13-01', '2026-13-02']]) assert.throws(() => prioritizeGsc(report, { ...rows, start, end }), /date/iu);
  assert.doesNotThrow(() => prioritizeGsc(report, { ...rows, start: '2024-02-29', end: '2024-03-01' }));
});

test('GSC clicks and impressions must be nonnegative integers', () => {
  for (const patch of [{ clicks: 0.5, impressions: 20.5 }, { impressions: 20.5 }, { clicks: 1.5 }]) assert.throws(() => prioritizeGsc(report, { rows: [{ page: 'https://example.com/guide', ...metrics, ...patch }] }), /clicks|impressions|count/iu);
  assert.doesNotThrow(() => prioritizeGsc(report, { rows: [{ page: 'https://example.com/guide', ...metrics, ctr: 0.025, position: 12.5 }] }));
});
