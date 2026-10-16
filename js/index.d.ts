import type { AstroIntegration } from 'astro';
export interface HierarchyInput {
  parents: Readonly<Record<string, string>>;
  labels: Readonly<Record<string, string>>;
  requireChildToParent?: boolean;
  requireParentToChild?: boolean;
  requireBreadcrumbs?: boolean;
  breadcrumbSelector?: string;
  rootLabel?: string;
}
declare const validated: unique symbol;
export type Structure = Readonly<HierarchyInput> & { readonly [validated]: true };
export interface LinkItem { readonly path: string; readonly label: string }
export function defineStructure(input: HierarchyInput): Structure;
export function parentOf(structure: Structure, page: string): LinkItem | undefined;
export function childrenOf(structure: Structure, page: string): LinkItem[];
export function breadcrumbsOf(structure: Structure, page: string): LinkItem[];
export interface Thresholds {
  brokenPaths?: number; brokenFragments?: number; orphans?: number;
  unreachable?: number; maxDepth?: number; hierarchyViolations?: number;
}
export interface CommonOptions {
  site?: string;
  basePath?: string;
  trailingSlash?: 'always' | 'never' | 'ignore';
  buildFormat?: 'directory' | 'file';
  originAliases?: string[];
  mode?: 'audit' | 'enforce';
  entrypoints?: string[];
  exclude?: string[];
  excludePagePaths?: string[];
  hierarchy?: HierarchyInput;
  thresholds?: Thresholds;
  reportPath?: string;
  reportFormat?: 'compact' | 'full';
  fullReportPath?: string;
  summaryPath?: string;
  baselinePath?: string;
  gscPath?: string;
  gscMinImpressions?: number;
  publicDir?: string;
  binaryPath?: string;
  timeoutMs?: number;
}
export interface AuditOptions extends CommonOptions { artifactRoot: string; site: string }
export interface Report {
  schema: '1';
  engineVersion: string;
  coverage: { totalPages: number; scannedPages: number; assets: number; excludedPages: string[]; complete: boolean; scanIncompleteReason?: string };
  config: { site: string; originAliases?: string[]; basePath: string; trailingSlash: string; buildFormat: string; mode: string; entrypoints: string[]; auditScope?: { excludedPages: string[]; hierarchy: HierarchyInput | null } };
  nodes: Array<{ path: string; file: string; depth?: number; orphan: boolean; unreachable: boolean; inCycle: boolean; editorialDepth: number | null; editorialOrphan: boolean; editorialUnreachable: boolean; editorialSources: number; metadata: PageMetadata | null }>;
  edges: Array<{ source: string; target: string; href: string; landmark: string; rel?: string[]; editorial: boolean; anchor: string }>;
  issues: {
    brokenPaths: Array<{ source: string; href: string; resolved: string; reason: string }>;
    brokenFragments: Array<{ source: string; href: string; target: string; fragment: string; reason: string }>;
    external: Record<string, string[]>;
    special: unknown[];
  };
  hierarchy: { configured: boolean; cycles: string[]; missingParents: string[]; violations: Array<{ kind: string; page: string; related?: string; detail: string }> };
  opportunities: Array<{ kind: string; source: string; target: string; detail: string; [key: string]: unknown }>;
  gates: { mode: string; violated: boolean; outcomes: Array<{ name: string; observed: number; limit?: number; violated: boolean }> };
  sitemap: { files: string[]; urls: number; issues: Array<{ kind: string; url: string; detail: string }>; basis: string };
  anchorFindings: Array<{ kind: string; source: string; target: string; anchor: string; detail: string }>;
}
export interface PageMetadata {
  title: string;
  canonicals: string[];
  canonicalStatus: 'absent' | 'self' | 'external' | 'noncanonical' | 'malformed' | 'multiple';
  canonicalResolved: string | null;
  robots: Record<string, string[]>;
  noindex: boolean;
  eligible: boolean;
  basis: string;
}
export interface Finding {
  id: string; rule: string; severity: 'critical' | 'warning' | 'advisory' | 'review';
  source: string; target: string; evidence: string; detail: string; occurrences: number;
  status?: 'new' | 'resolved' | 'existing';
}
export interface Summary {
  schema: 'astro-link-summary/1'; engineVersion: string; config: Report['config'];
  coverage: Report['coverage']; gates: Report['gates'];
  counts: { findings: number; occurrences: number; critical: number; pages: number; fullEdges: number; editorialEdges: number; editorialPairs: number; editorialReachable: number; editorialUnreachable: number; editorialOrphans: number; editorialMaxDepth: number; eligible: number; missingCanonical: number };
  findings: Finding[];
  comparison: { counts: { new: number; resolved: number; existing: number }; resolved: Finding[] } | null;
  issues: Pick<Report['issues'], 'brokenPaths' | 'brokenFragments'>;
  hierarchy: Report['hierarchy']; opportunities: Report['opportunities']; sitemap?: Report['sitemap'];
  gsc?: GscPrioritization; omitted: string[]; basis: string;
}
export interface GscMetrics { clicks: number; impressions: number; ctr: number; position: number }
export interface GscImport {
  domain?: string; kind?: 'page' | 'query' | 'page-query'; start?: string; end?: string;
  dimensions?: Array<'page' | 'query'>;
  rows?: Array<GscMetrics & ({ page: string; query?: string } | { keys: string[] })>;
  result?: { rows: Array<GscMetrics & { keys: string[] }> };
}
export interface GscPageReview extends GscMetrics {
  page: string; queryRelevance: 'unknown' | 'title-match' | 'no-title-match';
  queries: Array<{ query: string; impressions: number; position: number }>;
  relevantQueries: Array<{ query: string; impressions: number; position: number }>;
  editorialSources: number; score?: number; reason: string;
}
export interface GscPrioritization {
  inputRows: number; queryOnlyRows: number; priorities: GscPageReview[];
  missingArtifacts: GscPageReview[]; excluded: GscPageReview[];
  minImpressions: number; positionRange: number[]; start?: string; end?: string; basis: string;
}
export function summarizeReport(report: Report, options?: { baseline?: Report | Summary; gsc?: GscPrioritization }): Summary;
export function formatSummary(summary: Summary): string;
export function prioritizeGsc(report: Report, input: GscImport, options?: { minImpressions?: number }): GscPrioritization;
export class AuditError extends Error { readonly report?: Report; readonly exitCode?: number }
export function auditArtifacts(options: AuditOptions): Promise<Report>;
export function internalLinkStructure(options?: CommonOptions): AstroIntegration;
export default internalLinkStructure;
