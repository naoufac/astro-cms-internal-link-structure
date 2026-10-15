# Astro CMS internal link structure

Read-only internal link auditing for static Astro output. The ESM integration sends a versioned JSON request to a native Rust subprocess after Astro finishes writing artifacts. It does not inject scripts, styles, HTML, links or metadata, and does not rewrite site files.

This is a locally built package, not a claim of npm registry publication. The tested platform is Astro 7.3.5, Node 22.22.2, npm 10.9.7 and Linux x64. Other Astro versions and native platforms are not claimed compatible. Static `directory` and `file` formats are supported; SSR/server output and `build.format: 'preserve'` are explicitly rejected.

## Supported URL semantics

The engine and the Astro integration share one URL model:

- Static output only. `output: 'server'` and `build.format: 'preserve'` throw.
- Page keys follow the real emitted routes. Directory format keys end with a slash (`/docs/guide/`); file format keys do not (`/docs/guide`); the site root is `/` in both. Keys carry the `base` prefix: with `base: '/knowledge'`, the root page key is `/knowledge/` (directory) or `/knowledge` (file).
- `site` must be a bare HTTP(S) origin. Deployment prefixes belong in Astro `base`, never in `site`.
- `trailingSlash` controls link hrefs and the per-page document URL used for relative resolution: `always` resolves relative links against a slash-terminated document URL, `never` against a slashless one, `ignore` follows the build format (slash for directory, no slash for file). This matches what a browser does with the served page URL.
- Resolved link paths are normalized to the page-key form for the configured `buildFormat` before classification, so `/docs`, `/docs/` and (file format) `/docs.html` all classify against the same emitted page where the format and trailingSlash policy make them equivalent. Absolute hrefs on the configured origin, root-relative hrefs and relative hrefs all resolve through this same model.
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

`npm run build` explicitly runs `cargo build --release --locked` and copies `target/release/astro-cms-internal-link-structure` to `native/<platform>-<arch>/astro-cms-internal-link-structure`. There is no install, prepare or postinstall compilation/download hook. Build before packing. A local tarball contains the native executable for the machine on which it was built, plus Rust source, Cargo.lock and the toolchain file. It is not a portable or universal binary. Linux libc/loader compatibility still matters. Rebuild from the included source on other platforms, or supply `binaryPath` pointing to a compatible version of this engine. A missing or non-executable binary is always an error, never a silent JS fallback.

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

The default report is `<Astro root>/.astro-link-reports/report.json`, outside generated/public output. Relative integration report paths resolve against the Astro root. Reports inside the actual output or public directory are rejected, including through symlink aliases. Enforce writes the native report before throwing for threshold violations. Invalid options, failed or incomplete scans and missing binaries fail in both modes. A configuration/runtime failure may have no report; an old report must never be taken as proof of a successful latest scan.

## Options

Options apply to `internalLinkStructure(options)` (Astro integration) and `auditArtifacts(options)` (standalone) unless noted.

| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| `mode` | `'audit' \| 'enforce'` | `'audit'` | `audit` records violations without throwing; `enforce` throws `AuditError` (with `report` and `exitCode: 1`) after the report is written. |
| `hierarchy` | `defineStructure()` result | none | Explicit parent/label maps plus `requireChildToParent`, `requireParentToChild`, `requireBreadcrumbs`, `breadcrumbSelector`, `rootLabel`. Parent relations are never guessed from folder names. |
| `entrypoints` | `string[]` | base root in the selected format | Explicit emitted page keys used as reachability roots. |
| `thresholds` | object | `brokenPaths: 0`, `brokenFragments: 0`, `hierarchyViolations: 0` | Nonnegative integer limits: `brokenPaths`, `brokenFragments`, `orphans`, `unreachable`, `maxDepth`, `hierarchyViolations`. A gate violates when the observed value strictly exceeds the limit. `{}` disables all threshold gates but never bypasses configuration or scan failures. |
| `reportPath` | `string` | `.astro-link-reports/report.json` | Integration paths resolve against the Astro root; standalone paths resolve against the current directory. Must stay outside generated output and `public/`. |
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

`auditArtifacts(options): Promise<Report>` returns the real native report unchanged. Report bytes on disk are exactly the native stdout JSON.

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

The report is deterministic for identical artifacts and configuration. Top-level fields:

- `schema`: `'1'` (note: the request field is `schemaVersion`, the report field is `schema`).
- `engineVersion`: native engine version string.
- `coverage`: `totalPages`, `scannedPages` (after exclusions), `assets`, `excludedPages`, `complete`, and `scanIncompleteReason` when a page could not be read.
- `config`: `site`, `originAliases`, `basePath`, `trailingSlash`, `buildFormat`, `mode`, `entrypoints` actually used.
- `nodes`: one record per eligible page: `path` (page key), `file` (path under artifactRoot), `depth` from entrypoints, `orphan` (no inbound internal page links), `unreachable` (not reachable from entrypoints), `inCycle`.
- `edges`: `{source, target, href, landmark, rel}` per internal page-to-page link, where `target` is the normalized page key.
- `issues`: `brokenPaths` (`{source, href, resolved, reason}`), `brokenFragments` (`{source, href, target, fragment, reason}`), `external` (origin to source/href list), `special` (mailto/tel/unparseable/outside-base hrefs).
- `hierarchy`: `configured`, `cycles`, `missingParents`, `violations` (`{kind, page, related, detail}`) for missing child-to-parent links, missing parent-to-child links, missing breadcrumbs, cycles and unknown parents.
- `opportunities`: engine-derived advisory records (`{kind, source, target, detail}`), for example configured siblings that are not cross-linked.
- `gates`: `mode`, `outcomes` (`{name, observed, limit, violated}`) and `violated`.

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

The audit sees emitted static HTML and local assets, not client-rendered runtime navigation, authenticated routes or a deployed server's redirects. It does not verify external HTTP status, guarantee indexation or improve rankings by itself. No production edits or deployment are performed.

Integration hook reference: https://docs.astro.build/en/reference/integrations-reference/

MIT. Copyright 2026 naoufac.
