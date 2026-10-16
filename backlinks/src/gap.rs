use crate::{
    competitors::within,
    detector::{page_links_to_owned_domain, PageFacts},
    neturl::safe_parse,
    report::{
        CompetitorEvidence, CompetitorLink, DetectorConfig, Opportunity, RULE_ENGINE_VERSION,
    },
    text::{contains_word, full_visible_text, sentence_containing},
};
use scraper::{Html, Selector};
use std::collections::BTreeSet;

pub fn observe(
    html: &str,
    facts: &PageFacts,
    config: &DetectorConfig,
    competitors: &[String],
) -> Option<Opportunity> {
    let source = safe_parse(&facts.final_url).ok()?;
    if facts
        .owned_hosts
        .iter()
        .chain(&config.excluded_domains)
        .any(|d| within(source.host_ascii(), d))
        || page_links_to_owned_domain(html, &source, &facts.owned_hosts)
    {
        return None;
    }
    let text = full_visible_text(html);
    let (target, topic) = facts.targets.iter().find_map(|(url, topics)| {
        topics
            .iter()
            .find(|t| contains_word(&text, t))
            .map(|t| (url, t))
    })?;
    let quote = sentence_containing(html, topic)?;
    let doc = Html::parse_document(html);
    let mut links = Vec::new();
    let mut seen = BTreeSet::new();
    for el in doc.select(&Selector::parse("a[href]").unwrap()) {
        let abs = match source.normalized.join(el.attr("href").unwrap_or("")) {
            Ok(u) => u,
            Err(_) => continue,
        };
        let u = match safe_parse(abs.as_str()) {
            Ok(u) => u,
            Err(_) => continue,
        };
        let Some(domain) = competitors.iter().find(|d| within(u.host_ascii(), d)) else {
            continue;
        };
        if !seen.insert(u.normalized.to_string()) {
            continue;
        }
        links.push(CompetitorLink {
            domain: domain.clone(),
            url: u.normalized.to_string(),
            quote: el.text().collect::<String>().trim().to_string(),
        });
    }
    let domains: BTreeSet<_> = links.iter().map(|l| l.domain.clone()).collect();
    if domains.len() < 2 {
        return None;
    }
    Some(Opportunity {
        rule: "competitor-backlink-gap-lead".into(), rule_version: RULE_ENGINE_VERSION.into(), source_url: facts.final_url.clone(), target_url: target.clone(),
        evidence_quote: quote, evidence_location: "visible_text".into(), broken_url: None, broken_final_url: None,
        observed_status: facts.observed_status, checked_at_utc: facts.checked_at_utc.clone(), redirect_chain: facts.redirect_chain.clone(),
        existing_link_checked: true, links_to_owned_domain: false, source_is_first_party: false,
        observed_facts: vec![format!("source returned HTTP {} after safety and robots checks", facts.observed_status), format!("anchors to {} distinct configured competitor identities observed in fetched HTML", domains.len()), "No anchor resolves to an owned domain or its subdomains; imported hints are not live evidence".into()],
        inferred_assessment: vec![format!("Topical suitability INFERRED from configured topic '{topic}' in source visible text; review placement manually"), "No authority, conversion, willingness to link or ranking guarantee".into()],
        competitor_evidence: Some(CompetitorEvidence { domains: domains.into_iter().collect(), links }),
    })
}
