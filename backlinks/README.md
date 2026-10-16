# backlink-opportunities-detector

Optional, opt-in backlink opportunities detector for Astro sites. Standalone
Rust CLI with JSON input/output. It is a companion component to the Astro CMS
internal link structure engine: the internal engine analyzes and improves
links WITHIN your own site; this module finds EXTERNAL backlink
opportunities on other people's pages. It never runs during Astro builds and
never modifies site content.

## What it detects (and what it does not)

Four evidence-backed rules, each verified against a live fetch of the
candidate page:

1. `unlinked-brand-mention`: an exact whole-word brand match (configured
   `brands`) in the page's visible text, where the page links to none of the
   configured owned domains anywhere. Script, style, comment, nav, footer,
   header, and aside text never matches. The evidence quote is a verbatim
   sentence from DOM text with entities decoded and whitespace collapsed.
   Inline markup introduces no invented separators. Hidden attributes,
   aria-hidden=true and obvious inline display:none/visibility:hidden are
   excluded at every depth. External CSS visibility and JS rendering are not evaluated.
2. `curated-resource-page-lead`: the page has a resource-style heading AND a
   group of at least three distinct external links in that heading's section
   (ending at the next same-or-higher-level heading, excluding boilerplate)
   AND at least one configured topic keyword appears
   in the page text. A resource page is not a verified willingness to link;
   placement suitability is labeled INFERRED in every finding.
3. `broken-link-replacement-possibility`: an outbound destination observed
   terminal HTTP 404 or 410 on an independent GET (redirects followed
   manually and recorded), with actual visible source text matching a configured
   topic. The matched sentence is recorded separately from the anchor quote.
   Timeouts, 403, 429, robots denials, DNS failures, and soft-404 pages are
   NEVER reported as broken. Relevance of a replacement is INFERRED.
4. `competitor-backlink-gap-lead`: imported backlink rows identify one source
   page associated with at least two distinct configured competitor identities.
   The fetched source must actually link to both identities, match a configured
   topic in visible text and link to no owned domain. Provenance rows remain
   discovery hints, never substitutes for observed anchors. Placement fit is INFERRED.

Hard exclusions, always:

- source pages on any configured owned domain (first-party);
- source pages already linking to any owned domain;
- login-walled / social / UGC platforms (facebook, reddit, quora, google
  sites, twitter/x, linkedin, instagram, youtube, pinterest, tiktok,
  threads);
- domains in configured `excluded_domains`.

Every opportunity separates `observed_facts` (HTTP status, quotes, link
counts) from `inferred_assessment` (topical fit, placement suitability).
Nothing in the output is an authority metric: no DA, no traffic estimates, no
ranking predictions, no backlink counts, no guaranteed placements, and no
outreach is performed or drafted. Empty result categories are honest zero
results for that run.

## CLI

Native argument parsing (no shell involved):

```sh
# 1. Generate search-query suggestions from your config (no network).
backlink-opportunities-detector generate-queries \
  --config config.json --output queries.json

# 2. Run those queries in YOUR OWN search tool. Export results as JSON
#    (provider-neutral array; raw provider exports are also accepted).

# 3. Detect over the imported candidates (network is opt-in, see config).
backlink-opportunities-detector detect \
  --config config.json \
  --candidates results.json \
  --output report.json
```

Exit codes: 0 = report or queries produced (inspect partial, warnings and
candidate outcomes, not just the exit code); 1 = hard failure (unreadable/invalid
inputs, failed provider acquisition, output/capture write failure, existing output
path, or network disabled for detect); 2 = usage error (unknown subcommand/flag,
duplicate flag, missing required argument or invalid candidates/backlinks combination).
Output files must be new paths; existing files and symlinks are never overwritten.

Without `network.enabled = true` in the config, `detect` refuses to run
rather than pretending. `generate-queries` never touches the network.

## Config (examples/gissecrets-config.json)

- `schema_version`: 1.
- `site_origin`, `owned_domains`, `brands`: first-party identity.
- `competitor_domains`: bare domain identities for competitor backlink imports.
  Owned, excluded and competitor definitions use the same lowercase IDNA identity,
  with www/apex equivalence and subdomains included. Overlapping competitor,
  owned or excluded identities are invalid.
- `target_pages[].url` + `.topics`: the ONLY candidate link targets. Topics
  must match the page text for curated/broken rules; the detector never
  invents targets from page content.
- `limits`: candidate pages 25, outbound checks 100, per-origin pacing
  1500 ms, timeout 15 s, body cap 3 MiB, max 5 redirects (defaults).
  Pacing is shared by robots requests, source pages, outbound checks and every
  redirect hop at the actual destination origin. Candidate/outbound limits count
  logical checks; robots and redirects add bounded HTTP requests. Execution is
  single-threaded; the accepted `concurrency` setting does not add parallelism.
- `network.enabled`: must be true for detect; `user_agent` is sent with
  every request and used for robots; `evidence_dir` (outside any site tree)
  receives raw HTML captures with SHA-256 references in the report.
  Captures are immutable, with content hashes and collision-safe filenames;
  reopening a directory cannot overwrite earlier captures. New directories are
  mode 0700 and captures mode 0600. The tool resolves path/symlink ancestors and
  refuses the CWD project and trees marked by .git, Cargo.toml, package.json or
  astro.config.{js,mjs,ts}. Run from the audited project so even an unmarked site
  is protected. Captures can contain public form tokens: keep them outside version
  control, retain only as long as needed, and delete them manually when appropriate.
  Capture write failures abort the run rather than silently dropping evidence.

## Import format

`examples/imported-results.json` is a provider-neutral transform of genuine
provider search output (query, provider, collection date, title, snippet
preserved as provenance hints). Search snippets are never treated as
evidence of page contents; only live fetches are evidence.
Failed or malformed provider responses are import errors, not empty searches;
an explicit response.data.web=[] is a successful empty acquisition.

## Competitor backlink journey (offline-safe example)

Acquire backlink exports outside this tool using your own dataset/provider.
There is no backlink-index API client, automatic discovery or provider credential
handling here. Transform the export into an array (or {"backlinks": [...]}) whose
rows have exactly these fields:

```json
[
  {"source_url":"https://source.example/resources?topic=tarot",
   "competitor_url":"https://one.example/tarot",
   "provenance":"Synthetic example row one"},
  {"source_url":"https://source.example/resources?topic=tarot",
   "competitor_url":"https://two.example/tarot",
   "provenance":"Synthetic example row two"}
]
```

provenance must be nonempty. Exactly two distinct competitors on one source page
are sufficient; more are accepted. Repeated URLs or www/subdomain aliases of a
single competitor do not meet the two-identity threshold. Groups with fewer than
two identities are filtered before verification. The competitor domains must be
configured; unknown competitor_url hosts and unexpected row fields are errors.

Source grouping is conservative: case, IDNA, default ports and fragments normalize;
queries, protocol, www/apex and path trailing slashes remain distinct source URLs.
Ordinary candidate imports normalize trailing slashes on the path only, never in
the query. Outbound destinations preserve full query identity and drop fragments.
No query sorting, query removal or inferred canonical/redirect aliasing occurs.

From backlinks/, with the Cargo toolchain on PATH:

```sh
cargo run --locked --bin backlink-opportunities-detector -- --help
cargo run --locked --bin backlink-opportunities-detector -- generate-queries \
  --config examples/competitor-config.json
cargo run --locked --bin backlink-opportunities-detector -- detect \
  --config examples/competitor-config.json \
  --backlinks examples/competitor-backlinks.json --output /tmp/competitor-report-new.json
```

The final command intentionally exits 1 with network.enabled is false, after
parsing/validating the synthetic import. It performs no network or output write.
For an actual audit, supply real exported rows and your own config, explicitly
enable network, choose an external evidence directory and a new report path.
Do not enable network on the synthetic .example fixtures.

Completion: partial=true when limits skip verification or sources/outbound
destinations are inaccessible, with warnings describing skipped work. Static
unsafe inputs remain rejected records with counts. A completed negative means
only that the evaluated inputs did not qualify, not that the web has no leads.

## Network safety

- only http/https, standard ports only (port must equal the scheme default);
- credentials/userinfo rejected; malformed percent-encoding rejected;
- hosts: public domains, or IPv4/IPv6 literals classified as publicly
  routable. IPv6 uses a conservative allowlist (global unicast 2000::/3
  minus IETF protocol assignments, documentation, 6to4, NAT64, Teredo,
  ORCHID); IPv4 mirrors the IANA special-purpose registry;
- the url crate canonicalizes nonstandard IPv4 text forms (decimal, hex,
  octal, short) per WHATWG before classification, so `0x7f000001` or
  `2130706433` cannot smuggle loopback past the check;
- DNS answers are filtered to public addresses; mixed public/private answers
  use only the public ones; all-private means unreachable;
- connections are PINNED to the vetted address (reqwest custom DNS resolver with
  no fallback; the original hostname remains the TLS/SNI name and the actual
  response socket peer must equal the selected IP and port),
  so DNS rebinding mid-run cannot redirect a request to a private target;
- every redirect hop is re-validated (safety parse, DNS vet, no HTTPS
  downgrade, cycle detection, max 5 hops), including robots.txt redirects;
- robots.txt is fetched per origin with the configured User-Agent before any
  page request. 401/403/429/5xx or unreachable robots means the origin is
  unavailable, i.e. NOT crawled (conservative). 404/410 robots means no
  restrictions. Every page redirect destination is robots-gated. Matching uses
  case-sensitive paths, longest matching rule, Allow on equal specificity, '*'
  wildcards and terminal '$'. Matching user-agent groups are combined at the
  most-specific product-token match; '*' groups are the fallback. Percent-encoded
  unreserved octets normalize for comparison; reserved octets remain encoded;
- production bodies are read incrementally after reqwest gzip/brotli decoding,
  with the configured decompressed cap applied before each allocation extension;
  at most one excess byte is read before aborting, including redirect responses;
- production code has no localhost bypass anywhere: tests inject a fully
  deterministic in-memory transport (`transport::fake`) instead.

## Limitations

- current backlink CLI uses Unix filesystem permissions and is tested on Linux x64;
  native Windows builds are not supported;
- single fetch per candidate page, no recursion, no crawling, no JS;
- robots parse covers standard user-agent/path rules; crawl-delay is not
  auto-tuned beyond the fixed per-origin pacing config;
- brand matching is case-insensitive whole-word with documented
  normalization; it does not resolve pronouns, nicknames, or images;
- curated-lead and broken-link rules gate on configured topic keywords;
  changing topics changes findings;
- broken-link findings are possibilities, not endorsements: editorial
  acceptance is unknowable without asking;
- the detector records what it observed at run time; pages change;
- no ranking, authority, or traffic data of any kind is produced, and none
  should be inferred.

## Development

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Tests cover: mention boundaries/aliases, already-linked exclusion,
self/first-party exclusion, nav/script/comment non-matches, curated vs
incidental pages, topical-evidence gating, terminal 404/410 vs 403 vs
redirect-healthy, robots allow/disallow/401/403/429/5xx/absent/redirect,
redirect-to-private, HTTPS downgrade, loops, mixed/private/all-private DNS,
alternate IPv4 encodings, unsafe IPv6 forms, body caps, candidate/outbound
limits, evidence hashing, provenance preservation, and honest empty runs.

Optional parent-controlled socket proof (not part of tests):

```sh
cargo build --locked --example pin-probe
strace -f -e trace=connect -o /tmp/backlink-pin-connect.log target/debug/examples/pin-probe
```

This bounded probe makes exactly one production HTTPS GET to example.com:443
using selected 1.1.1.1, timeout 5 seconds and body cap 64 KiB. The original
hostname's TLS certificate must verify and response.remote_addr() must equal
1.1.1.1:443 for success. If TLS/network fails, inspect the connect trace for the
selected destination; do not claim an HTTP success. No redirects are followed.

License: MIT (c) 2026 naoufac. This module is independent of, and does not
modify, the parent internal-link engine crate.
