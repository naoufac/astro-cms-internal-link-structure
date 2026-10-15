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
  binaryPath?: string;
  timeoutMs?: number;
}
export interface AuditOptions extends CommonOptions { artifactRoot: string; site: string }
export interface Report {
  schema: '1';
  engineVersion: string;
  coverage: { totalPages: number; scannedPages: number; assets: number; excludedPages: string[]; complete: boolean; scanIncompleteReason?: string };
  config: { site: string; originAliases?: string[]; basePath: string; trailingSlash: string; buildFormat: string; mode: string; entrypoints: string[] };
  nodes: Array<{ path: string; file: string; depth?: number; orphan: boolean; unreachable: boolean; inCycle: boolean }>;
  edges: Array<{ source: string; target: string; href: string; landmark: string; rel?: string[] }>;
  issues: {
    brokenPaths: Array<{ source: string; href: string; resolved: string; reason: string }>;
    brokenFragments: Array<{ source: string; href: string; target: string; fragment: string; reason: string }>;
    external: Record<string, string[]>;
    special: unknown[];
  };
  hierarchy: { configured: boolean; cycles: string[]; missingParents: string[]; violations: Array<{ kind: string; page: string; related?: string; detail: string }> };
  opportunities: Array<{ kind: string; source: string; target: string; detail: string; [key: string]: unknown }>;
  gates: { mode: string; violated: boolean; outcomes: Array<{ name: string; observed: number; limit?: number; violated: boolean }> };
}
export class AuditError extends Error { readonly report?: Report; readonly exitCode?: number }
export function auditArtifacts(options: AuditOptions): Promise<Report>;
export function internalLinkStructure(options?: CommonOptions): AstroIntegration;
export default internalLinkStructure;
