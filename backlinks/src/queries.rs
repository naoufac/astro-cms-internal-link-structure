//! Search-query suggestion generation from explicit config subjects.
//!
//! Templates are documented and deterministic; queries are exported for
//! execution in an EXTERNAL search tool. The detector never searches.

use crate::report::{
    DetectorConfig, QueryConfigSummary, QueryOutput, QuerySuggestion, CONFIG_SCHEMA_VERSION,
};

fn now_utc() -> String {
    web_time_utc()
}

/// RFC 3339 UTC timestamp without external chrono dependency.
pub fn web_time_utc() -> String {
    // std::time::SystemTime → UNIX epoch seconds → ISO 8601. Days-from-epoch
    // algorithm: Howard Hinnant's civil_from_days (public domain).
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Generate deterministic query suggestions from config subjects.
pub fn generate_queries(config: &DetectorConfig) -> QueryOutput {
    let mut queries: Vec<QuerySuggestion> = Vec::new();

    let own_suffix = origin_host_suffix(&config.site_origin);
    for brand in &config.brands {
        let brand_q = format!("\"{brand}\"");
        queries.push(QuerySuggestion {
            template: "brand_exact_no_own_site".to_string(),
            query: format!("{brand_q} -site:{own_suffix}"),
            subject: brand.clone(),
            purpose: "brand-mention".to_string(),
        });
    }
    // Topic-derived resource-list templates use the first target page's
    // topics as subjects (explicit, configured topics only).
    let mut seen_topics = std::collections::BTreeSet::new();
    for tp in &config.target_pages {
        for t in &tp.topics {
            if seen_topics.insert(t.clone()) {
                queries.push(QuerySuggestion {
                    template: "topic_resource_list".to_string(),
                    query: format!("{t} \"useful links\" resources"),
                    subject: t.clone(),
                    purpose: "resource-page".to_string(),
                });
                queries.push(QuerySuggestion {
                    template: "topic_link_collection".to_string(),
                    query: format!("{t} \"links\" resources -site:wikipedia.org"),
                    subject: t.clone(),
                    purpose: "resource-page".to_string(),
                });
            }
        }
    }

    let topics: Vec<String> = seen_topics.into_iter().collect();
    QueryOutput {
        schema_version: CONFIG_SCHEMA_VERSION,
        generated_at: now_utc(),
        config_summary: QueryConfigSummary {
            site_origin: config.site_origin.clone(),
            brands: config.brands.clone(),
            target_pages: config.target_pages.len(),
            topics,
        },
        queries,
        usage_note: "Run these queries in your own search tool of choice. Import genuine results back with detect --candidates. Search snippets are discovery hints, never evidence of page contents.",
    }
}

/// Registrable-domain suffix of a site origin (best effort, no PSL).
fn origin_host_suffix(origin: &str) -> String {
    crate::neturl::safe_parse(origin)
        .map(|u| u.host_ascii().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{Limits, NetworkPolicy, TargetPage};

    fn config() -> DetectorConfig {
        DetectorConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            site_origin: "https://gissecrets.com".into(),
            owned_domains: vec!["gissecrets.com".into()],
            brands: vec!["GisSecrets".into()],
            target_pages: vec![TargetPage {
                url: "https://gissecrets.com/tarot".into(),
                title: "Tarot".into(),
                topics: vec!["tarot".into(), "slavic mythology".into()],
            }],
            excluded_domains: vec![],
            competitor_domains: vec![],
            limits: Limits::default(),
            network: NetworkPolicy::default(),
        }
    }

    #[test]
    fn queries_are_deterministic_and_documented() {
        let cfg = config();
        let a = generate_queries(&cfg);
        let b = generate_queries(&cfg);
        let qa: Vec<String> = a.queries.iter().map(|q| q.query.clone()).collect();
        let qb: Vec<String> = b.queries.iter().map(|q| q.query.clone()).collect();
        assert_eq!(qa, qb, "generation must be deterministic");
        assert!(qa.contains(&"\"GisSecrets\" -site:gissecrets.com".to_string()));
        assert!(qa
            .iter()
            .any(|q| q.contains("tarot \"useful links\" resources")));
        assert!(a.queries.iter().all(|q| !q.template.is_empty()));
        assert!(a.queries.iter().any(|q| q.purpose == "brand-mention"));
        assert!(a.queries.iter().any(|q| q.purpose == "resource-page"));
    }

    #[test]
    fn timestamp_is_rfc3339() {
        let ts = web_time_utc();
        let re = regex_lite(&ts);
        assert!(re, "timestamp {ts} not RFC3339-like");
    }

    fn regex_lite(ts: &str) -> bool {
        let b = ts.as_bytes();
        b.len() == 20
            && b[4] == b'-'
            && b[7] == b'-'
            && b[10] == b'T'
            && b[13] == b':'
            && b[16] == b':'
            && b[19] == b'Z'
            && b.iter()
                .enumerate()
                .all(|(i, c)| matches!(i, 4 | 7 | 10 | 13 | 16 | 19) || c.is_ascii_digit())
    }
}
