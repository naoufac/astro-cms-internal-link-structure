import test from 'node:test';
import assert from 'node:assert/strict';
import { summarizeReport, formatSummary } from '../js/reporting.js';

const raw = paths => ({ schema: '1', engineVersion: 'test', config: { site: 'https://example.com', basePath: '', buildFormat: 'directory', trailingSlash: 'ignore', entrypoints: ['/'] }, coverage: { totalPages: 1, scannedPages: 1, complete: true }, nodes: [], edges: [], issues: { brokenPaths: paths.map(target => ({ source: '/', href: target, resolved: `https://example.com${target}`, reason: 'missing artifact' })), brokenFragments: [], external: {}, special: [] }, hierarchy: { configured: false, violations: [] }, opportunities: [], gates: { mode: 'audit', violated: true, outcomes: [] } });

test('stable baseline findings retain existing criticals and distinguish occurrences', () => {
  const before = raw(['/old/', '/old/', '/persistent/']);
  const after = raw(['/new/', '/persistent/']);
  const summary = summarizeReport(after, { baseline: before });
  assert.deepEqual(summary.comparison.counts, { new: 1, resolved: 1, existing: 1 });
  assert.equal(summarizeReport(before).counts.findings, 2);
  assert.equal(summarizeReport(before).counts.occurrences, 3);
  assert.equal(summary.findings.find(f => f.target.endsWith('/persistent/')).status, 'existing');
  assert.match(formatSummary(summary), /existing.*critical|critical.*existing/u);
  assert.deepEqual(summarizeReport(after, { baseline: summarizeReport(before) }), summary);
});

test('reject invalid baseline schema, shape, incomplete and incompatible scope', () => {
  for (const baseline of [{}, { ...raw([]), schema: '2' }, { ...raw([]), coverage: { complete: false } }, { ...raw([]), config: { ...raw([]).config, site: 'https://other.test' } }, { ...raw([]), issues: {} }]) {
    assert.throws(() => summarizeReport(raw([]), { baseline }), /baseline|report|scope/iu);
  }
});

test('incomplete current coverage never asserts baseline resolutions', () => {
  const before = raw(['/old/', '/persistent/']);
  const after = raw(['/persistent/']);
  after.coverage = { totalPages: 1, scannedPages: 0, complete: false };
  assert.throws(() => summarizeReport(after, { baseline: before }), /incomplete|complete|coverage/iu);
  assert.doesNotThrow(() => summarizeReport(after));
  const resumed = structuredClone(after);
  resumed.coverage = { totalPages: 1, scannedPages: 1, complete: true };
  assert.equal(summarizeReport(resumed, { baseline: before }).comparison.counts.resolved, 1);
});
