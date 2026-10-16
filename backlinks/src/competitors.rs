use crate::{
    neturl::safe_parse,
    report::{BacklinkHint, DetectorConfig, ImportedCandidate},
};
use std::collections::{BTreeMap, BTreeSet};

pub fn identity(domain: &str) -> anyhow::Result<String> {
    if domain.is_empty()
        || domain.trim() != domain
        || domain.contains(['/', ':', '?', '#', '@', '\\'])
    {
        anyhow::bail!("competitor/owned/excluded domain must be a bare domain: {domain}");
    }
    let u = safe_parse(&format!("https://{domain}/"))
        .map_err(|e| anyhow::anyhow!("invalid domain {domain}: {}", e.0))?;
    if u.host_ascii().parse::<std::net::IpAddr>().is_ok() {
        anyhow::bail!("domain identity must not be an IP address");
    }
    Ok(u.host_ascii()
        .strip_prefix("www.")
        .unwrap_or(u.host_ascii())
        .to_string())
}

pub fn within(host: &str, domain: &str) -> bool {
    let (Ok(host), Ok(domain)) = (identity(host), identity(domain)) else {
        return false;
    };
    host == domain || host.ends_with(&format!(".{domain}"))
}

pub fn validate(config: &DetectorConfig) -> anyhow::Result<Vec<String>> {
    let mut blocked = config
        .owned_domains
        .iter()
        .chain(&config.excluded_domains)
        .map(|d| identity(d))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let site =
        safe_parse(&config.site_origin).map_err(|e| anyhow::anyhow!("invalid site: {}", e.0))?;
    blocked.push(identity(site.host_ascii())?);
    let mut domains: Vec<String> = Vec::new();
    for raw in &config.competitor_domains {
        let domain = identity(raw)?;
        if blocked
            .iter()
            .chain(&domains)
            .any(|d| within(&domain, d) || within(d, &domain))
        {
            anyhow::bail!("competitor definition overlaps owned, excluded or another competitor identity: {raw}");
        }
        domains.push(domain);
    }
    Ok(domains)
}

pub fn source_key(raw: &str) -> anyhow::Result<String> {
    let u = safe_parse(raw).map_err(|e| anyhow::anyhow!("unsafe source URL: {}", e.0))?;
    let mut normalized = u.normalized.clone();
    normalized
        .set_host(Some(u.host_ascii()))
        .map_err(|_| anyhow::anyhow!("invalid source host"))?;
    Ok(normalized.to_string())
}

pub fn parse_backlinks(
    text: &str,
    config: &DetectorConfig,
) -> anyhow::Result<Vec<ImportedCandidate>> {
    let domains = validate(config)?;
    let value: serde_json::Value = serde_json::from_str(text)?;
    let rows = value
        .as_array()
        .or_else(|| value.get("backlinks").and_then(|v| v.as_array()))
        .ok_or_else(|| anyhow::anyhow!("backlink import requires an array or backlinks wrapper"))?;
    let mut groups: BTreeMap<String, Vec<BacklinkHint>> = BTreeMap::new();
    for row in rows {
        let hint: BacklinkHint = serde_json::from_value(row.clone())?;
        if hint.provenance.trim().is_empty() {
            anyhow::bail!("backlink provenance must be nonempty");
        }
        let source = source_key(&hint.source_url)?;
        let competitor = safe_parse(&hint.competitor_url)
            .map_err(|e| anyhow::anyhow!("invalid competitor_url: {}", e.0))?;
        if !domains.iter().any(|d| within(competitor.host_ascii(), d)) {
            anyhow::bail!("import competitor_url is not a configured competitor");
        }
        groups.entry(source).or_default().push(hint);
    }
    let mut candidates = Vec::new();
    for (url, hints) in groups {
        let competitors: BTreeSet<_> = hints
            .iter()
            .filter_map(|h| {
                let u = safe_parse(&h.competitor_url).ok()?;
                domains.iter().find(|d| within(u.host_ascii(), d)).cloned()
            })
            .collect();
        if competitors.len() < 2 {
            continue;
        }
        candidates.push(ImportedCandidate {
            url,
            query: None,
            provider: Some("imported-backlinks".into()),
            collected_at: None,
            search_title: None,
            search_snippet: None,
            suggested_target: None,
            backlink_hints: hints,
        });
    }
    Ok(candidates)
}
