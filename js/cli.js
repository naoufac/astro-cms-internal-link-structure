#!/usr/bin/env node
import { readFile } from 'node:fs/promises';
import { resolve, dirname, join } from 'node:path';
import { auditArtifacts, AuditError, summarizeReport, formatSummary } from './index.js';

try {
  const args = process.argv.slice(2);
  if (args.length < 2 || args[0] !== '--config' || args.length > 3 || (args[2] !== undefined && args[2] !== '--raw')) throw new TypeError('Usage: node js/cli.js --config /path/audit-options.json [--raw]');
  const config = JSON.parse(await readFile(args[1], 'utf8'));
  const report = await auditArtifacts(config);
  if (args[2] === '--raw') process.stdout.write(JSON.stringify(report, null, 2) + '\n');
  else process.stdout.write(await readFile(resolve(config.summaryPath ?? `${config.reportPath ?? join(dirname(resolve(config.artifactRoot)), '.astro-link-reports/report.json')}.txt`), 'utf8'));
} catch (error) {
  if (error instanceof AuditError && error.report) process.stdout.write(formatSummary(summarizeReport(error.report)));
  process.stderr.write(`${error.message}\n`);
  process.exitCode = error instanceof AuditError && error.exitCode === 1 ? 1 : 2;
}
