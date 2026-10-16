# Astro CMS internal link structure

Read-only internal link auditing for static Astro output. The ESM integration sends a versioned JSON request to a native Rust subprocess after Astro finishes writing artifacts. It does not inject scripts, styles, HTML, links or metadata, and does not rewrite site files.

This is a locally built package, not a claim of npm registry publication. The tested platform is Astro 7.3.5, Node 22.22.2, npm 10.9.7 and Linux x64. Other Astro versions and native platforms are not claimed compatible. Static `directory` and `file` formats are supported; SSR/server output and `build.format: 'preserve'` are explicitly rejected.

## Supported URL semantics

The engine and the Astro integration share one URL model:

- Static output only. `output: 'server'` and `build.format: 'preserve'` throw.
- Page keys follow the real emitted routes. Directory format keys end with a slash (`/docs/guide/`); file format keys do not (`/docs/guide`); the site root is `/` in both. Keys carry the `base` prefix: with `base: '/knowledge'`, the root page key is `/knowledge/` (directory) or `/knowledge` (file).
- `site` must be a bare HTTP(S) origin. Deployment prefixes belong in Astro `base`, never in `site`.
- Relative hrefs resolve against each page's emitted-document URL, which the build format alone determines: file format document URLs always carry the `.html` suffix (the site root is `<base>/index.html`), directory format document URLs always end with a slash. This is the public path of the emitted HTML file. `trailingSlash` is captured, validated and echoed in the report config; it does not change document URLs or link classification. Tolerant URL aliases of an emitted page are accepted during artifact resolution and classification only; the engine performs no server redirect or HTTP status validation.
- Resolved link paths are normalized to the page-key form for the configured `buildFormat` before classification, so `/docs`, `/docs/` and (file format) `/docs.html` all classify against the same emitted page where the format makes them equivalent. Absolute hrefs on the configured origin, root-relative hrefs and relative hrefs all resolve through this same model.
- Asset links (for example `/style.css`, `/download.txt`) are checked against emitted files. Fragments are checked against element ids and named anchors in the target page.

## Build from source

Prerequisites: Node 22.12+ (22.22.2 tested), npm, Rust 1.99.0 with cargo, rustfmt and clippy, and a native linker/build environment. `rust-toolchain.toml` selects the Rust toolchain. Install Rust and these components explicitly before building. Dependency installation may require network access. Ordinary audits and Astro builds do not fetch pages or use the network.

```sh
npm ci
npm run build
npm test
npm run gate
npm pack --pack-destination /tmp
```

`npm run build` explicitly runs `cargo build --release --locked --message-format=json` and copies the executable path reported by Cargo to `native/<platform>-<arch>/astro-cms-internal-link-structure`. This honors Cargo's actual target directory, including `CARGO_TARGET_DIR` and Cargo configuration, rather than assuming `target/release`. Before replacing the bundle, it runs a local fixture probe and rejects engines missing the metadata, editorial graph, sitemap or anchor fields required by the JS consumer; matching schema/version strings alone are not sufficient. There is no install, prepare or postinstall compilation/download hook. Build before packing. A local tarball contains the native executable for the machine on which it was built, plus Rust source, Cargo.lock and the toolchain file. It is not a portable or universal binary. Linux libc/loader compatibility still matters. Rebuild from the included source on other platforms, or supply `binaryPath` pointing to a compatible version of this engine. A missing or non-executable binary is always an error, never a silent JS fallback.

Install the resulting tarball in a separate Astro consumer:

```sh
npm install /absolute/path/astro-cms-internal-link-structure-0.1.0.tgz
```

`npm run gate` runs Rust format checking, locked tests, clippy with warnings denied, TypeScript consumer checks, helper and native-bridge tests, real Astro fixtures, npm packing, and installation/builds in a fresh temporary consumer outside the source checkout. It checks successful builds, intentional enforce failure with a saved report, audit success on the same errors, deterministic reports and byte-identical generated artifacts with/without the integration. No tests are skipped when the engine is unavailable. All gate statuses propagate. Set `EVIDENCE_DIR=/absolute/path/outside/source` to retain complete subprocess logs and packaging results; its default is the system temporary directory's `astro-link-integration-evidence`.

## Astro integration quickstart

```js
import { defineConfig } from 'astro/config';
import { internalLinkStructure, defineStructure } from 'astro-cms-internal-link-structure';

const hierarchy = defineStructure({
  parents: { '/docs/': '/', '/docs/api/': '/docs/' },
  labels: { '/': 'Home', '/docs/': 'Documentation', '/docs/api/': 'API' },
  requireChildToParent: true,
  requireParentToChild: true,
  requireBreadcrumbs: true,
  breadcrumbSelector: 'nav[aria-label="Breadcrumb"]',
});

export default defineConfig({
  site: 'https://example.com',
  output: 'static',
  trailingSlash: 'always',
  build: { format: 'directory' },
  integrations: [internalLinkStructure({
    mode: 'enforce',
    hierarchy,
    entrypoints: ['/'],
    thresholds: { brokenPaths: 0, brokenFragments: 0, hierarchyViolations: 0 },
    reportPath: '.astro-link-reports/report.json',
  })],
});
```

`astro:config:done` captures `site`, `base`, `trailingSlash` and `build.format` from the resolved Astro config and rejects any explicit option override that conflicts with them. `astro:build:done` scans the actual output directory Astro reports, not an assumed `dist` path. The integration does not run scans during development.

The default report is `<Astro root>/.astro-link-reports/report.json`, outside generated/public output. Relative integration report paths resolve against the Astro root. Reports inside the actual output or public directory are rejected, including through symlink aliases. Enforce writes the selected JSON report and text summary before throwing for threshold violations. Invalid options, failed or incomplete scans and missing binaries fail in both modes. A configuration/runtime failure may have no report; an old report must never be taken as proof of a successful latest scan.

## Options

Options apply to `internalLinkStructure(options)` (Astro integration) and `auditArtifacts(options)` (standalone) unless noted.

| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| `mode` | `'audit' \| 'enforce'` | `'audit'` | `audit` records violations without throwing; `enforce` throws `AuditError` (with `report` and `exitCode: 1`) after the report is written. |
| `hierarchy` | `defineStructure()` result | none | Explicit parent/label maps plus `requireChildToParent`, `requireParentToChild`, `requireBreadcrumbs`, `breadcrumbSelector`, `rootLabel`. Parent relations are never guessed from folder names. |
| `entrypoints` | `string[]` | base root in the selected format | Explicit emitted page keys used as reachability roots. |
| `thresholds` | object | `brokenPaths: 0`, `brokenFragments: 0`, `hierarchyViolations: 0` | Nonnegative integer limits: `brokenPaths`, `brokenFragments`, `orphans`, `unreachable`, `maxDepth`, `hierarchyViolations`. A gate violates when the observed value strictly exceeds the limit. `{}` disables all threshold gates but never bypasses configuration or scan failures. |
| `reportPath` | `string` | integration: `<root>/.astro-link-reports/report.json`; standalone: `<artifactRoot parent>/.astro-link-reports/report.json` | Explicit relative integration paths resolve against the Astro root; standalone paths resolve against cwd, not the config file. All outputs must stay outside generated output and `public/`, be distinct and not overwrite inputs. |
| `reportFormat` | `'compact' \| 'full'` | `'compact'` | Selects the JSON stored at `reportPath`; does not change the full API return value or CLI `--raw` output. |
| `fullReportPath` | `string` | none | Optional additional full JSON, exactly the native stdout bytes, alongside the selected report. |
| `summaryPath` | `string` | `<reportPath>.txt` | Human-readable summary, including all findings, written in both formats. |
| `baselinePath` | `string` | none | Previously saved full or compact complete report with matching audit scope. Adds new/existing/resolved comparison to compact JSON and text, not full native JSON. |
| `gscPath` | `string` | none | Local imported GSC JSON file, never fetched. Adds `gsc` prioritization to compact JSON and text, not full native JSON. |
| `gscMinImpressions` | `number` | `10` | Positive integer minimum for GSC page reviews; review position range is 8-20 inclusive. |
| `originAliases` | `string[]` | `[]` | Additional bare HTTP(S) origins treated as internal. Never fetched. |
| `exclude` | `string[]` | `[]` | Exact page keys removed from eligibility and traversal. Not globs. |
| `excludePagePaths` | `string[]` | `[]` | Same semantics as `exclude`; both are applied. |
| `binaryPath` | `string` | bundled `native/<platform>-<arch>` binary | Executable path used with `spawn(binary, [], { shell: false })` and stdin JSON. Never an interpolated shell command. |
| `timeoutMs` | `number` | `120000` | Positive integer. Timeout, invalid report JSON, unexpected exits and stdout over 128 MiB all fail. |
| `artifactRoot` | `string` | supplied by `astro:build:done` | Standalone only. Absolute or cwd-relative path to the built output. Required standalone; forbidden as an integration option. |
| `site` | `string` | Astro `site` | Standalone only (or an identical override for the integration). Bare HTTP(S) origin. |
| `basePath` | `string` | Astro `base` | Standalone only (or an identical override). `''` or `/prefix`. |
| `trailingSlash` | `'always' \| 'never' \| 'ignore'` | Astro `trailingSlash` | Standalone only (or an identical override). |
| `buildFormat` | `'directory' \| 'file'` | Astro `build.format` | Standalone only (or an identical override). `preserve` is rejected everywhere. |

Unknown options, non-plain objects, accessor properties and invalid values throw `TypeError` synchronously.

## Standalone artifact audit

```js
import { auditArtifacts } from 'astro-cms-internal-link-structure';

const report = await auditArtifacts({
  artifactRoot: '/absolute/path/to/dist',
  site: 'https://example.com',
  basePath: '',
  trailingSlash: 'ignore',
  buildFormat: 'directory',
  mode: 'audit',
  entrypoints: ['/'],
  reportPath: '/absolute/path/to/reports/links.json',
});
console.log(report.issues, report.opportunities);
```

`auditArtifacts(options): Promise<Report>` returns the full native report unchanged (`schema: '1'`), including nodes and edges. By default, `reportPath` instead stores compact `astro-link-summary/1` JSON and `<reportPath>.txt` stores its human summary. This API/disk distinction is intentional. Use `reportFormat: 'full'` to store exactly the native stdout bytes at `reportPath`, or set `fullReportPath` to retain a separate full report while keeping the compact default. Baseline comparison and GSC additions appear only in the compact report and text summary. Full native JSON and the API return value do not contain those JS-derived additions. Compact storage does not avoid generating and parsing the full native report in memory; stdout is limited to 128 MiB.

## Node CLI

After installing the local tarball, save this as `audit.config.json` in a consumer with an existing static `dist/` directory:

```json
{
  "artifactRoot": "./dist",
  "site": "https://example.com",
  "mode": "audit",
  "reportPath": "./reports/links.json",
  "fullReportPath": "./reports/full.json",
  "summaryPath": "./reports/links.txt",
  "timeoutMs": 300000
}
```

```sh
npm exec --offline -- astro-cms-link-audit --config audit.config.json
npm exec --offline -- astro-cms-link-audit --config audit.config.json --raw > reports/raw.json
```

The first command prints the human summary. `--raw` prints the full native JSON (not the compact file), including on enforce failure when a report exists; the failure summary and diagnostic go to stderr. Exit codes are `0` for audit/passing enforce, `1` for enforce violations with a saved report, `2` for configuration/runtime errors. Errors without a report print diagnostics only on stderr. A previous file is not evidence that a failed scan produced a new report. `timeoutMs` is a configurable native subprocess deadline in milliseconds (default 120000), not a deadline for JS post-processing. The example raises it to five minutes; omit it to use the default. Relative config values resolve from the process cwd, not the JSON file's directory.

For direct Node invocation, the package exports `astro-cms-internal-link-structure/js/cli.js`. Native binary paths are exported under `astro-cms-internal-link-structure/native/*` and can be located with `import.meta.resolve()`; they are executables, not JS modules.

## Baseline comparison

Keep an earlier complete report at a separate path before rerunning, for example after the first command above:

```sh
cp reports/links.json reports/baseline.json
```

Save `baseline.config.json` (the input and output must not be the same file):

```json
{
  "artifactRoot": "./dist",
  "site": "https://example.com",
  "reportPath": "./reports/compared.json",
  "baselinePath": "./reports/baseline.json"
}
```

```sh
npm exec --offline -- astro-cms-link-audit --config baseline.config.json
```

Both full `schema: '1'` and compact `astro-link-summary/1` baselines are accepted. Site, aliases, base, build format, trailing slash, entrypoints, exclusions and hierarchy scope must match. Incomplete baselines/current scans cannot establish resolved findings. Compact `findings[].status` is `new` or `existing`; `comparison` contains counts plus resolved findings. Existing critical findings remain visible and still affect configured gates. A baseline does not turn enforce into a new-findings-only gate.

## Optional offline GSC workflow

Export measurements separately; the auditor never calls GSC. Save a neutral local import as `gsc.json`, replacing these illustrative measurements with your real exported rows:

```json
{
  "rows": [
    { "page": "https://example.com/", "query": "home guide", "clicks": 2, "impressions": 100, "ctr": 0.02, "position": 12 }
  ]
}
```

Save `gsc.config.json`:

```json
{
  "artifactRoot": "./dist",
  "site": "https://example.com",
  "reportPath": "./reports/gsc-links.json",
  "gscPath": "./gsc.json",
  "gscMinImpressions": 10
}
```

```sh
npm exec --offline -- astro-cms-link-audit --config gsc.config.json
```

Imports also support raw `{dimensions: ['page', 'query'], rows: [{keys: [...], clicks, impressions, ctr, position}]}` and saved wrappers `{domain, kind: 'page' | 'query' | 'page-query', start: 'YYYY-MM-DD', end: 'YYYY-MM-DD', result: {rows: [...]}}` with matching dimensional keys. Use `rows: []` for an empty import. Do not mix dimensional and neutral rows, duplicate dimensional rows, or page totals and query-page rows for one page. Domains must match the audit site/aliases. Page-only query relevance is unknown; query-only rows are counted, never arbitrarily joined to pages.

Compact `gsc` contains `priorities`, `excluded`, `missingArtifacts`, input counts and scope/basis. Eligible pages with sufficient impressions and position 8-20 are scored by `impressions * (21 - position) * relevanceFactor / (1 + distinctEditorialSources)`, where the factor is 1.5 for a title-matching query, otherwise 1. Noindex/noncanonical pages are excluded. Missing artifacts have unknown HTTP status, not a proven 404. This historical-data heuristic is a review order, not predicted traffic or ranking gains.

## Standalone native CLI

The Rust binary behind the bridge is also usable directly. It reads one JSON request from stdin and writes the report JSON to stdout.

```sh
native/linux-x64/astro-cms-internal-link-structure <<'JSON'
{
  "schemaVersion": "1",
  "artifactRoot": "/absolute/path/to/dist",
  "site": "https://example.com",
  "originAliases": [],
  "basePath": "",
  "trailingSlash": "ignore",
  "buildFormat": "directory",
  "mode": "audit",
  "entrypoints": ["/"],
  "exclude": [],
  "excludePagePaths": [],
  "hierarchy": null,
  "thresholds": { "brokenPaths": 0, "brokenFragments": 0, "hierarchyViolations": 0 }
}
JSON
```

Exit codes: `0` successful audit or passing enforce, `1` enforce threshold violations (report still printed), `2` invalid configuration or scan failure (diagnostics on stderr). The request schema is versioned; unsupported `schemaVersion`, unknown fields and invalid values exit `2`.

## Report schema

The report is deterministic for identical artifacts and configuration.

### Default compact JSON (`astro-link-summary/1`)

Top-level fields are `schema`, `engineVersion`, `config`, `coverage`, `gates`, `counts`, `findings`, `comparison`, `issues` (broken paths/fragments only), `hierarchy`, `opportunities`, `sitemap`, optional `gsc`, `omitted` and `basis`. There are no `nodes` or `edges`, or external/special href inventories. `counts` includes full/editorial edge occurrences, distinct editorial pairs, reachability/depth, eligibility and finding/occurrence totals. Findings are deduplicated with stable `id`, `rule`, `severity`, `source`, `target`, `evidence`, `detail`, `occurrences` and optional baseline `status`. Every finding is retained; compact does not mean a top-N truncation. Finding-heavy compact reports can be larger than full JSON.

### Full native JSON (`schema: '1'`)

This is the API return, native stdout, Node CLI `--raw` output and explicit full-file format. Top-level fields:

- `schema`: `'1'` (note: the request field is `schemaVersion`, the report field is `schema`).
- `engineVersion`: native engine version string.
- `coverage`: `totalPages`, `scannedPages` (after exclusions), `assets`, `excludedPages`, `complete`, and `scanIncompleteReason` when a page could not be read.
- `config`: `site`, `originAliases`, `basePath`, `trailingSlash`, `buildFormat`, `mode`, `entrypoints` actually used, plus `auditScope` for exclusions and hierarchy.
- `nodes`: one record per non-excluded page: `path` (page key), `file` (path under artifactRoot), `depth` from entrypoints, `orphan` (no distinct inbound internal page source), `unreachable`, `inCycle`, `editorialDepth`, `editorialOrphan`, `editorialUnreachable`, `editorialSources`, and `metadata` (title, canonical evidence/status, robots/noindex, eligibility and offline basis). Metadata eligibility is local indexing evidence, not verified Google indexation. Editorial graph metrics exclude menu/hidden/noneditorial links and nofollow support.
- `edges`: `{source, target, href, landmark, rel, editorial, anchor}` per internal page-to-page link, where `target` is the normalized page key. Anchor evidence recognizes visible text, image alt, and empty-text fallback via the first same-document `aria-labelledby` ID, then aria-label/title. The first matching document ID wins. This is not a full accessibility tree.
- `issues`: `brokenPaths` (`{source, href, resolved, reason}`), `brokenFragments` (`{source, href, target, fragment, reason}`), `external` (origin to source/href list), `special` (mailto/tel/unparseable/outside-base hrefs).
- `hierarchy`: `configured`, `cycles`, `missingParents`, `violations` (`{kind, page, related, detail}`) for missing child-to-parent links, missing parent-to-child links, missing breadcrumbs, cycles and unknown parents.
- `opportunities`: engine-derived advisory records (`{kind, source, target, detail}`). `hierarchy` records a missing configured child-to-parent or parent-to-child link. `contextual` records a source page whose visible unlinked body text contains the target's own label (or `<title>` as fallback) verbatim as a phrase of at least two words, with source and target eligible, distinct and not already linked; sibling relations and URL similarity are never used as evidence.
- `gates`: `mode`, `outcomes` (`{name, observed, limit, violated}`) and `violated`.
- `sitemap`: local `files`, URL count, consistency `issues` and offline `basis`; no remote sitemap is fetched.
- `anchorFindings`: advisory vague/ambiguous anchor evidence (`{kind, source, target, anchor, detail}`), not enforce thresholds.

Reports can contain private page text and paths; keep them out of public output and review before sharing.

Internal opportunities are engine output, not JS-generated heuristics. They are advisory evidence for human review, not automatic edits, guaranteed relevance, ranking improvements or backlink placements. This wrapper exposes whatever the built engine actually reports and does not manufacture findings.

## Data-only hierarchy helpers

```astro
---
import { breadcrumbsOf, childrenOf, parentOf } from 'astro-cms-internal-link-structure';
import { hierarchy } from '../structure.mjs';
const page = '/docs/api/';
const crumbs = breadcrumbsOf(hierarchy, page);
const parent = parentOf(hierarchy, page);
const children = childrenOf(hierarchy, page);
---
<nav aria-label="Breadcrumb">
  {crumbs.map(item => <a href={item.path}>{item.label}</a>)}
</nav>
{parent && <a href={parent.path}>{parent.label}</a>}
<ul>{children.map(item => <li><a href={item.path}>{item.label}</a></li>)}</ul>
```

Export `hierarchy = defineStructure(...)` from your own module. Every parent/child endpoint needs an explicit nonempty label. The helper copies and freezes the supplied data and rejects cycles, malformed paths, accessors, unknown fields and missing labels. No title, slug or label is invented. `parentOf` returns `{path, label}` or undefined; `childrenOf` returns sorted immediate children; `breadcrumbsOf` returns root-to-parent ancestors, excluding the current page. Query helpers accept only the validated `defineStructure` result. They return data, not HTML. Consumers decide where and how to render, and how deployment URL policy maps data keys into hrefs. With file format or `trailingSlash: 'never'`, adapt hrefs as needed without changing hierarchy keys.

## External backlink opportunities

External prospect scanning belongs to the independent `backlinks/` Rust crate in the source repository. It is opt-in, not invoked or downloaded by this npm integration, and is not bundled into the internal engine. No external backlink scan or outreach runs in ordinary Astro builds or fixture tests. Refer to that crate's CLI documentation and its own validation before using it; this package makes no claim that live external discovery has been completed.

## Validation limits

The full fixture matrix is enforced by `test/astro.mjs` on a real Astro build: directory and file formats, root and `/knowledge` base, and `trailingSlash` always/never/ignore, including deterministic repeat reports, byte-identical artifacts with the integration disabled, broken-link enforce failure with a saved report and audit-mode success on the same broken fixture. `npm run gate` must pass on the matching native source and binary before release; it fails if the engine is missing or any permutation regresses.

The audit sees emitted static HTML and local assets, not client-rendered runtime navigation, authenticated routes or a deployed server's redirects. Contextual text filtering recognizes excluded HTML elements, hidden attributes and basic inline hiding styles; it does not compute stylesheet visibility or execute JavaScript. Review opportunity findings before acting on them. It does not verify external HTTP status, guarantee indexation or improve rankings by itself. No production edits or deployment are performed.

Integration hook reference: https://docs.astro.build/en/reference/integrations-reference/

MIT. Copyright 2026 naoufac.
