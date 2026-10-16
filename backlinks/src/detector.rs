//! Core detection engine: link extraction, rule application, and verdicts.
//!
//! Three rules, all evidence-first:
//! 1. `unlinked-brand-mention`: exact visible-text brand match with no link
//!    to any owned domain anywhere on the page.
//! 2. `curated-resource-page-lead`: observable resource-list structure
//!    (editorial heading + grouped external links) with inferred topical fit.
//! 3. `broken-link-replacement-possibility`: outbound destination observed
//!    terminal 404/410 by real GET, with anchor + inferred topical relevance.

use std::collections::HashSet;

use scraper::Selector;
use sha2::{Digest, Sha256};

use crate::neturl::{host_to_ascii, safe_parse, SafeUrl};
use crate::report::{
    Opportunity, RULE_BROKEN_LINK, RULE_ENGINE_VERSION, RULE_MENTION, RULE_RESOURCE_PAGE,
};
use crate::text::{contains_word, full_visible_text, sentence_containing, visible_regions};

/// One outbound anchor on a source page.
#[derive(Debug, Clone)]
pub struct OutboundLink {
    pub href: String,
    pub anchor_text: String,
}

/// Extract external anchors with anchor texts, bounded by `limit`.
/// Excludes same-origin links, owned domains, unresolvable/other-scheme
/// hrefs. Duplicate destinations collapse (first anchor text wins).
pub fn extract_outbound_links(
    html: &str,
    source: &SafeUrl,
    owned_hosts: &HashSet<String>,
    limit: usize,
) -> Vec<OutboundLink> {
    let doc = scraper::Html::parse_document(html);
    let Ok(a_sel) = Selector::parse("a[href]") else {
        return Vec::new();
    };
    let source_host = source.host_ascii();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for el in doc.select(&a_sel) {
        if out.len() >= limit {
            break;
        }
        let Some(href) = el.value().attr("href") else {
            continue;
        };
        let href = href.trim();
        if href.is_empty() || href.starts_with('#') {
            continue;
        }
        let Ok(abs) = source.normalized.join(href) else {
            continue;
        };
        if abs.scheme() != "http" && abs.scheme() != "https" {
            continue;
        }
        let Some(host) = abs.host_str() else {
            continue;
        };
        let Ok(ascii) = host_to_ascii(host) else {
            continue;
        };
        if ascii == source_host || owned_hosts.contains(&ascii) {
            continue;
        }
        let key = format!("{}://{}{}", abs.scheme(), ascii, abs.path());
        if !seen.insert(key) {
            continue;
        }
        let anchor_text: String = el.text().collect::<Vec<_>>().join(" ");
        out.push(OutboundLink {
            href: abs.to_string(),
            anchor_text: anchor_text.trim().to_string(),
        });
    }
    out
}

/// True when any anchor on the page resolves to an owned-domain host
/// (checked regardless of region boilerplate: a link anywhere counts).
pub fn page_links_to_owned_domain(
    html: &str,
    source: &SafeUrl,
    owned_hosts: &HashSet<String>,
) -> bool {
    let doc = scraper::Html::parse_document(html);
    let Ok(a_sel) = Selector::parse("a[href]") else {
        return false;
    };
    for el in doc.select(&a_sel) {
        let Some(href) = el.value().attr("href") else {
            continue;
        };
        let Ok(abs) = source.normalized.join(href.trim()) else {
            continue;
        };
        if let Some(h) = abs.host_str() {
            if let Ok(ascii) = host_to_ascii(h) {
                if owned_hosts
                    .iter()
                    .any(|d| crate::competitors::within(&ascii, d))
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Classify a heading as resource/list-style (structural signal only).
pub fn heading_is_resource_style(heading: &str) -> bool {
    let h = heading.to_lowercase();
    for w in [
        "resource",
        "link",
        "guide",
        "reading",
        "bibliograph",
        "reference",
        "further",
        "library",
        "directory",
        "source",
        "collection",
        "useful",
        "bookmark",
        "tool",
        "recommended",
        "recommendation",
        "where to",
        "learn",
        "study",
        "media",
        "book",
    ] {
        if h.contains(w) {
            return true;
        }
    }
    false
}

/// Number of unique external link destinations on the page.
pub fn count_external_links(html: &str, source: &SafeUrl) -> usize {
    extract_outbound_links(html, source, &HashSet::new(), usize::MAX).len()
}

/// SHA-256 hex digest of a captured body (evidence integrity reference).
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

/// Shared observation fields for building opportunities.
#[derive(Debug, Clone)]
pub struct PageFacts {
    pub source_url: String,
    pub observed_status: u16,
    pub checked_at_utc: String,
    pub redirect_chain: Vec<String>,
    pub final_url: String,
    pub owned_hosts: HashSet<String>,
    pub brands: Vec<String>,
    /// Explicit configured target page chosen for this candidate (the caller
    /// must have validated this is a config `target_pages` entry).
    pub target_url: String,
    /// All explicit configured first-party target pages (url + lowercase
    /// topic keywords). Candidate targets NEVER come from page text; only
    /// from this configured list.
    pub targets: Vec<(String, Vec<String>)>,
    pub outbound_limit: usize,
}

pub struct PageAnalysis {
    pub links_to_owned_domain: bool,
    pub mention: Option<Opportunity>,
    pub curated_lead: Option<Opportunity>,
    pub outbound: Vec<OutboundLink>,
}

/// Build a broken-link opportunity from a verified dead destination.
///
/// `topic_matched` must be true only when the anchor/page text shares the
/// configured topic keywords of the chosen target: otherwise the finding is
/// recorded as excluded, not as an opportunity.
#[allow(clippy::too_many_arguments)]
pub fn build_broken_link_opportunity(
    facts: &PageFacts,
    broken_url: &str,
    broken_final_url: Option<String>,
    dest_status: u16,
    dest_checked_at_utc: String,
    dest_redirect_chain: Vec<String>,
    anchor_text: &str,
    topic_evidence: &str,
) -> Opportunity {
    Opportunity {
        competitor_evidence: None,
        rule: RULE_BROKEN_LINK.to_string(),
        rule_version: RULE_ENGINE_VERSION.to_string(),
        source_url: facts.source_url.clone(),
        target_url: facts.target_url.clone(),
        evidence_quote: anchor_text.to_string(),
        evidence_location: "anchor_text".to_string(),
        broken_url: Some(broken_url.to_string()),
        broken_final_url,
        observed_status: dest_status,
        checked_at_utc: dest_checked_at_utc,
        redirect_chain: dest_redirect_chain,
        existing_link_checked: true,
        links_to_owned_domain: false,
        source_is_first_party: false,
        observed_facts: vec![
            format!("source page returned HTTP {}", facts.observed_status),
            format!(
                "outbound destination {broken_url} observed terminal HTTP {dest_status} on independent GET"
            ),
        ],
        inferred_assessment: vec![
            "relevance of the dead link's topic to the proposed replacement is INFERRED"
                .to_string(),
            format!("topic match basis: {topic_evidence}"),
        ],
    }
}

/// Analyze a fetched page for the mention + curated-lead rules.
/// Broken-link verification happens separately (needs network round-trips).
pub fn analyze_page(html: &str, facts: &PageFacts) -> PageAnalysis {
    let src = |facts: &PageFacts| -> SafeUrl {
        // Callers validated this URL before constructing facts.
        safe_parse(&facts.source_url).expect("internally validated source URL")
    };
    let source = src(facts);
    let links_to_owned = page_links_to_owned_domain(html, &source, &facts.owned_hosts);

    // Topical target selection against the page's own visible text: returns
    // (url, matched topic) for the configured target with the most topic
    // keyword hits. None when no configured topic appears at all.
    let page_text = full_visible_text(html);
    let topical_target = |facts: &PageFacts| -> Option<(String, String)> {
        let mut best: Option<(usize, String, String)> = None;
        for (url, topics) in &facts.targets {
            let mut hits: Vec<String> = Vec::new();
            for t in topics {
                if contains_word(&page_text, t) {
                    hits.push(t.clone());
                }
            }
            if !hits.is_empty() {
                let score = hits.len();
                if best.as_ref().map(|b| score > b.0).unwrap_or(true) {
                    best = Some((score, url.clone(), hits.join(", ")));
                }
            }
        }
        best.map(|(_, url, hit)| (url, hit))
    };

    // Rule 1: unlinked brand mention (exact whole-word match in visible text).
    let mut mention = None;
    if !links_to_owned {
        let (target_url, topic_hit) = match topical_target(facts) {
            Some(x) => x,
            None => (
                facts.target_url.clone(),
                "no configured topic keywords matched; target is the configured fallback"
                    .to_string(),
            ),
        };
        for brand in &facts.brands {
            if !contains_word(&page_text, brand) {
                continue;
            }
            let _ = &topic_hit;
            if let Some(quote) = sentence_containing(html, brand) {
                mention = Some(Opportunity {
                    competitor_evidence: None,
                    rule: RULE_MENTION.to_string(),
                    rule_version: RULE_ENGINE_VERSION.to_string(),
                    source_url: facts.source_url.clone(),
                    target_url: target_url.clone(),
                    evidence_quote: quote,
                    evidence_location: "visible_text".to_string(),
                    broken_url: None,
                    broken_final_url: None,
                    observed_status: facts.observed_status,
                    checked_at_utc: facts.checked_at_utc.clone(),
                    redirect_chain: facts.redirect_chain.clone(),
                    existing_link_checked: true,
                    links_to_owned_domain: false,
                    source_is_first_party: false,
                    observed_facts: vec![
                        format!("page returned HTTP {}", facts.observed_status),
                        format!("whole-word brand '{brand}' present in visible page text"),
                        "no anchor on the page resolves to any configured owned domain".to_string(),
                    ],
                    inferred_assessment: vec![format!(
                        "topical relevance of {} to {} is INFERRED, not verified",
                        facts.source_url, facts.target_url
                    )],
                });
                break;
            }
        }
    }

    // Rule 2: curated resource-page lead (heading + grouped external links).
    let mut curated = None;
    let ext_count = count_external_links(html, &source);
    let resource_heading = visible_regions(html)
        .into_iter()
        .find(|r| r.kind == "heading" && heading_is_resource_style(&r.text));
    if let Some(h) = resource_heading {
        if ext_count >= 3 {
            let Some((target_url, topic_hit)) = topical_target(facts) else {
                let outbound =
                    extract_outbound_links(html, &source, &facts.owned_hosts, facts.outbound_limit);
                return PageAnalysis {
                    links_to_owned_domain: links_to_owned,
                    mention,
                    curated_lead: None,
                    outbound,
                };
            };
            curated = Some(Opportunity {
                competitor_evidence: None,
                rule: RULE_RESOURCE_PAGE.to_string(),
                rule_version: RULE_ENGINE_VERSION.to_string(),
                source_url: facts.source_url.clone(),
                target_url,
                evidence_quote: h.text.clone(),
                evidence_location: "heading".to_string(),
                broken_url: None,
                broken_final_url: None,
                observed_status: facts.observed_status,
                checked_at_utc: facts.checked_at_utc.clone(),
                redirect_chain: facts.redirect_chain.clone(),
                existing_link_checked: true,
                links_to_owned_domain: links_to_owned,
                source_is_first_party: false,
                observed_facts: vec![
                    format!("page returned HTTP {}", facts.observed_status),
                    format!("resource-style heading observed: '{}'", h.text),
                    format!("{ext_count} unique external link destinations on page"),
                ],
                inferred_assessment: vec![
                    "editorial willingness to ADD a new link is INFERRED from page structure, not confirmed"
                        .to_string(),
                    format!(
                        "topical fit INFERRED from matched configured topic keyword(s): {topic_hit} on {}",
                        facts.source_url
                    ),
                ],
            });
        }
    }

    let outbound = extract_outbound_links(html, &source, &facts.owned_hosts, facts.outbound_limit);

    PageAnalysis {
        links_to_owned_domain: links_to_owned,
        mention,
        curated_lead: curated,
        outbound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(source: &str, target: &str, brands: &[&str]) -> PageFacts {
        let mut owned = HashSet::new();
        owned.insert("gissecrets.com".to_string());
        PageFacts {
            source_url: source.to_string(),
            observed_status: 200,
            checked_at_utc: "2026-10-15T12:00:00Z".to_string(),
            redirect_chain: vec![],
            final_url: source.to_string(),
            owned_hosts: owned,
            target_url: target.to_string(),
            targets: vec![(
                "https://gissecrets.com/tarot".to_string(),
                vec!["tarot".to_string()],
            )],
            brands: brands.iter().map(|s| s.to_string()).collect(),
            outbound_limit: 100,
        }
    }

    const MENTION_HTML: &str = r#"<html><body>
      <nav><a href="/">Home</a><a href="/about">About</a></nav>
      <main>
      <h1>Blog post</h1>
      <p>Today I read GisSecrets and their tarot writeups were great.</p>
      <a href="https://other.example/page">a totally unrelated link</a>
      </main></body></html>"#;

    const LINKED_HTML: &str = r#"<html><body><main>
      <p>See <a href="https://gissecrets.com/">GisSecrets</a> for details.</p>
      </main></body></html>"#;

    #[test]
    fn mention_detected_with_verbatim_quote_and_not_when_linked() {
        let f = facts(
            "https://source.example/post",
            "https://gissecrets.com/tarot",
            &["gissecrets"],
        );
        let a = analyze_page(MENTION_HTML, &f);
        let m = a.mention.expect("unlinked mention must be detected");
        assert_eq!(m.rule, "unlinked-brand-mention");
        assert_eq!(
            m.evidence_quote,
            "Today I read GisSecrets and their tarot writeups were great."
        );
        assert!(m.evidence_quote.contains("GisSecrets"));
        assert!(!m.links_to_owned_domain);

        let a2 = analyze_page(LINKED_HTML, &f);
        assert!(
            a2.mention.is_none(),
            "already-linked page must not produce a mention"
        );
        assert!(a2.links_to_owned_domain);
    }

    #[test]
    fn partial_word_does_not_match() {
        let html =
            r#"<html><body><main><p>The gissecretspro toolkit launched.</p></main></body></html>"#;
        let f = facts(
            "https://source.example/x",
            "https://gissecrets.com/",
            &["gissecrets"],
        );
        let a = analyze_page(html, &f);
        assert!(a.mention.is_none());
    }

    #[test]
    fn script_comment_and_nav_mentions_ignored() {
        let html = r#"<html><body>
        <nav><a href="/">GisSecrets</a></nav>
        <script>var x = "gissecrets";</script>
        <!-- gissecrets -->
        <main><p>Nothing relevant here at all.</p></main></body></html>"#;
        let f = facts(
            "https://source.example/x",
            "https://gissecrets.com/",
            &["gissecrets"],
        );
        let a = analyze_page(html, &f);
        assert!(
            a.mention.is_none(),
            "nav/script/comment brand text must not count"
        );
    }

    #[test]
    fn curated_lead_requires_heading_and_external_links() {
        let curated = r#"<html><body><main>
        <h2>Useful resources</h2>
        <p>A hand-picked list of tarot reading materials.</p>
        <ul>
        <li><a href="https://a.example/1">One</a></li>
        <li><a href="https://b.example/2">Two</a></li>
        <li><a href="https://c.example/3">Three</a></li>
        </ul>
        </main></body></html>"#;
        let f = facts(
            "https://source.example/links",
            "https://gissecrets.com/tarot",
            &[],
        );
        let a = analyze_page(curated, &f);
        let c = a.curated_lead.expect("curated lead expected");
        assert_eq!(c.rule, "curated-resource-page-lead");
        assert_eq!(c.evidence_location, "heading");
        assert_eq!(c.evidence_quote, "Useful resources");
        assert!(c.inferred_assessment.iter().any(|s| s.contains("INFERRED")));

        // Heading but no external links: incidental, not a lead.
        let incidental =
            r#"<html><body><main><h2>Resources</h2><p>Coming soon.</p></main></body></html>"#;
        let a2 = analyze_page(incidental, &f);
        assert!(a2.curated_lead.is_none());

        // External links but a non-resource heading: not a lead.
        let no_heading = r#"<html><body><main>
        <h2>Weekly diary</h2>
        <ul><li><a href="https://a.example/1">1</a></li>
        <li><a href="https://b.example/2">2</a></li>
        <li><a href="https://c.example/3">3</a></li></ul>
        </main></body></html>"#;
        let a3 = analyze_page(no_heading, &f);
        assert!(a3.curated_lead.is_none());

        // Resource structure but ZERO configured-topic relevance: no lead.
        // This is the "unrelated stats site" negative (topical-evidence gate).
        let unrelated = r#"<html><body><main>
        <h2>Search Engine Backlinks</h2>
        <ul><li><a href="https://a.example/1">1</a></li>
        <li><a href="https://b.example/2">2</a></li>
        <li><a href="https://c.example/3">3</a></li></ul>
        <p>Domain statistics and traffic estimators.</p>
        </main></body></html>"#;
        let a4 = analyze_page(unrelated, &f);
        assert!(
            a4.curated_lead.is_none(),
            "resource structure without topical evidence must not be a lead"
        );
    }

    #[test]
    fn owned_domain_and_internal_links_excluded_from_outbound() {
        let html = r#"<html><body><main>
        <a href="https://gissecrets.com/a">own</a>
        <a href="/relative">internal</a>
        <a href="https://ext.example/x">external</a>
        <a href="javascript:void(0)">junk</a>
        </main></body></html>"#;
        let f = facts("https://source.example/", "https://gissecrets.com/", &[]);
        let a = analyze_page(html, &f);
        assert_eq!(a.outbound.len(), 1);
        assert_eq!(a.outbound[0].href, "https://ext.example/x");
    }

    #[test]
    fn sha256_reference_stable() {
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
