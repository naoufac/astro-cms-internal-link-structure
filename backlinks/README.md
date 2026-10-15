# backlink-opportunities-detector

Optional, opt-in backlink opportunities detector for Astro sites. Standalone
Rust CLI with JSON input/output. It is a companion component to the Astro CMS
internal link structure engine: the internal engine analyzes and improves
links WITHIN your own site; this module finds EXTERNAL backlink
opportunities on other people's pages. It never runs during Astro builds and
never modifies site content.

## What it detects (and what it does not)

Three evidence-backed rules, each verified against a live fetch of the
candidate page:

1. `unlinked-brand-mention`: an exact whole-word brand match (configured
   `brands`) in the page's visible text, where the page links to none of the
   configured owned domains anywhere. Script, style, comment, nav, footer,
   header, and aside text never matches. The evidence quote is a verbatim
   sentence from the page.
2. `curated-resource-page-lead`: the page has a resource-style heading AND a
   group of external links AND at least one configured topic keyword appears
   in the page text. A resource page is not a verified willingness to link;
   placement suitability is labeled INFERRED in every finding.
3. `broken-link-replacement-possibility`: an outbound destination observed
   terminal HTTP 404 or 410 on an independent GET (redirects followed
   manually and recorded), with an anchor that matches a configured topic.
   Timeouts, 403, 429, robots denials, DNS failures, and soft-404 pages are
   NEVER reported as broken. Relevance of a replacement is INFERRED.

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

Exit codes: 0 = report written (individual candidate rejections are part of
the report, not failures); 1 = hard failure (unreadable/invalid inputs,
network disabled for detect); 2 = usage error.

Without `network.enabled = true` in the config, `detect` refuses to run
rather than pretending. `generate-queries` never touches the network.

## Config (examples/gissecrets-config.json)

- `schema_version`: 1.
- `site_origin`, `owned_domains`, `brands`: first-party identity.
- `target_pages[].url` + `.topics`: the ONLY candidate link targets. Topics
  must match the page text for curated/broken rules; the detector never
  invents targets from page content.
- `limits`: candidate pages 25, outbound checks 100, per-origin pacing
  1500 ms, timeout 15 s, body cap 3 MiB, max 5 redirects (defaults).
- `network.enabled`: must be true for detect; `user_agent` is sent with
  every request and used for robots; `evidence_dir` (outside any site tree)
  receives raw HTML captures with SHA-256 references in the report.

## Import format

`examples/imported-results.json` is a provider-neutral transform of genuine
provider search output (query, provider, collection date, title, snippet
preserved as provenance hints). Search snippets are never treated as
evidence of page contents; only live fetches are evidence.

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
- connections are PINNED to the vetted address (reqwest `resolve_to_addrs`),
  so DNS rebinding mid-run cannot redirect a request to a private target;
- every redirect hop is re-validated (safety parse, DNS vet, no HTTPS
  downgrade, cycle detection, max 5 hops), including robots.txt redirects;
- robots.txt is fetched per origin with the configured User-Agent before any
  page request. 401/403/429/5xx or unreachable robots means the origin is
  unavailable, i.e. NOT crawled (conservative). 404/410 robots means no
  restrictions. Per-path rules are honored first-match;
- production code has no localhost bypass anywhere: tests inject a fully
  deterministic in-memory transport (`transport::fake`) instead.

## Limitations

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
cargo clippy --all-targets -- -D warnings
cargo test
```

Tests cover: mention boundaries/aliases, already-linked exclusion,
self/first-party exclusion, nav/script/comment non-matches, curated vs
incidental pages, topical-evidence gating, terminal 404/410 vs 403 vs
redirect-healthy, robots allow/disallow/401/403/429/5xx/absent/redirect,
redirect-to-private, HTTPS downgrade, loops, mixed/private/all-private DNS,
alternate IPv4 encodings, unsafe IPv6 forms, body caps, candidate/outbound
limits, evidence hashing, provenance preservation, and honest empty runs.

License: MIT (c) 2026 naoufac. This module is independent of, and does not
modify, the parent internal-link engine crate.
