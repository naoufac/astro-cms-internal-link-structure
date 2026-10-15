import type { AstroIntegration } from 'astro';
import { internalLinkStructure, auditArtifacts, defineStructure, parentOf, childrenOf, breadcrumbsOf, AuditError, type Report } from '../js/index.js';
const structure = defineStructure({ parents: { '/docs/': '/' }, labels: { '/': 'Home', '/docs/': 'Docs' } });
const integration: AstroIntegration = internalLinkStructure({ hierarchy: structure, mode: 'enforce', thresholds: { brokenPaths: 0 } });
const label: string | undefined = parentOf(structure, '/docs/')?.label;
const children: string[] = childrenOf(structure, '/').map(item => item.path);
const breadcrumbs: string[] = breadcrumbsOf(structure, '/docs/').map(item => item.label);
const promise: Promise<Report> = auditArtifacts({ artifactRoot: './dist', site: 'https://example.test', buildFormat: 'file', trailingSlash: 'never' });
const report: Report | undefined = new AuditError('test').report;
// @ts-expect-error unsupported output format
internalLinkStructure({ buildFormat: 'preserve' });
// @ts-expect-error explicit labels required
defineStructure({ parents: {} });
// @ts-expect-error only validated structures are accepted
parentOf({ parents: {}, labels: {} }, '/');
// @ts-expect-error standalone scan needs artifactRoot
auditArtifacts({ site: 'https://example.test' });
void [integration, label, children, breadcrumbs, promise, report];
