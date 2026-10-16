//! Candidate-queue processing pipeline (the "detect" runner).
//!
//! Single-threaded, bounded, robots-gated. Produces the versioned report
//! with verified facts vs inferred assessments kept separate.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::detector::{self, PageFacts};
use crate::evidence::Evidence;
use crate::fetch::{gated_get, FetchError};
use crate::neturl::safe_parse;
use crate::queries::web_time_utc;
use crate::report::{
    CandidateOutcome, CandidateRecord, DetectorConfig, ImportedCandidate, Opportunity, RobotsCheck,
    RunCounters, RunReport, CONFIG_SCHEMA_VERSION, REPORT_SCHEMA_VERSION, RULE_ENGINE_VERSION,
    TOOL_NAME, TOOL_VERSION,
};
use crate::text::sentence_containing;
use crate::transport::HttpTransport;

/// Run the full detection pipeline over imported candidates.
#[allow(clippy::too_many_arguments)]
pub fn run_detection(
    config: &DetectorConfig,
    candidates: &[ImportedCandidate],
    cfg_path: &str,
    cand_path: &str,
    transport: &dyn HttpTransport,
) -> anyhow::Result<RunReport> {
    let started = web_time_utc();
    let competitors = crate::competitors::validate(config)?;
    if !config.network.enabled && !candidates.is_empty() {
        anyhow::bail!("network is disabled; refusing candidate fetches");
    }
    let mut warnings: Vec<String> = Vec::new();
    let mut partial = false;

    // ---- Validate config ----
    if config.schema_version != CONFIG_SCHEMA_VERSION {
        anyhow::bail!(
            "unsupported config schema_version {} (want {})",
            config.schema_version,
            CONFIG_SCHEMA_VERSION
        );
    }
    let site = safe_parse(&config.site_origin)
        .map_err(|e| anyhow::anyhow!("config site_origin invalid: {}", e.0))?;
    if config.brands.is_empty() && config.target_pages.is_empty() {
        anyhow::bail!("config must define brands and/or target_pages");
    }
    let mut owned_hosts: HashSet<String> = HashSet::new();
    for d in &config.owned_domains {
        owned_hosts.insert(crate::competitors::identity(d)?);
    }
    owned_hosts.insert(crate::competitors::identity(site.host_ascii())?);

    // ---- Dedupe candidates, preserving first occurrence order + provenance ----
    let mut dedupe: HashMap<String, &ImportedCandidate> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut duplicates: Vec<String> = Vec::new();
    let mut records: Vec<CandidateRecord> = Vec::new();
    for c in candidates {
        match safe_parse(&c.url) {
            Ok(u) => {
                let key = if c.backlink_hints.is_empty() {
                    crate::neturl::dedupe_key(&u)
                } else {
                    crate::competitors::source_key(&c.url)?
                };
                match dedupe.entry(key.clone()) {
                    std::collections::hash_map::Entry::Occupied(o) => {
                        duplicates.push(o.key().clone());
                    }
                    std::collections::hash_map::Entry::Vacant(v) => {
                        v.insert(c);
                        order.push(key);
                    }
                }
            }
            Err(e) => {
                warnings.push(format!(
                    "candidate URL rejected before queue: {} ({})",
                    c.url, e.0
                ));
                records.push(record_rejected(c, &format!("unsafe target: {}", e.0)));
            }
        }
    }
    let queue: Vec<(&ImportedCandidate, crate::neturl::SafeUrl)> = order
        .into_iter()
        .map(|k| {
            let c = dedupe[&k];
            let u = safe_parse(&c.url).expect("validated above");
            (c, u)
        })
        .collect();

    // ---- Shared network state ----
    let paced = crate::pacing::PacedTransport::new(transport, config.limits.per_origin_delay_ms);
    let transport: &dyn HttpTransport = &paced;
    let mut robots_cache: HashMap<String, crate::fetch::RobotsOutcome> = HashMap::new();
    let mut evidence = Evidence::open(config.network.evidence_dir.as_deref())?;
    let mut outbound_used = 0usize;
    let mut candidate_limit_reached = false;
    let mut outbound_limit_reached = false;

    let mut opportunities: Vec<Opportunity> = Vec::new();
    let mut counters = RunCounters {
        candidates_input: candidates.len(),
        candidates_deduplicated: queue.len(),
        candidates_rejected: records.len(),
        ..Default::default()
    };

    // First-party target pages only, with topic keywords.
    struct Tp {
        url: String,
        topics: Vec<String>,
    }
    let targets: Vec<Tp> = config
        .target_pages
        .iter()
        .filter_map(|t| {
            let u = safe_parse(&t.url).ok()?;
            if !owned_hosts
                .iter()
                .any(|d| crate::competitors::within(u.host_ascii(), d))
            {
                return None;
            }
            Some(Tp {
                url: t.url.clone(),
                topics: t.topics.iter().map(|s| s.to_lowercase()).collect(),
            })
        })
        .collect();

    for (processed, (cand, url)) in queue.iter().enumerate() {
        if processed >= config.limits.candidate_pages {
            candidate_limit_reached = true;
            partial = true;
            warnings.push(format!(
                "candidate limit {} reached; {} queued candidates skipped",
                config.limits.candidate_pages,
                queue.len() - processed
            ));
            break;
        }

        let host = url.host_ascii().to_string();

        // ---- Static exclusions (no fetch) ----
        if owned_hosts
            .iter()
            .any(|d| crate::competitors::within(&host, d))
        {
            records.push(record_rejected(
                cand,
                "source is a first-party/owned domain",
            ));
            counters.candidates_rejected += 1;
            continue;
        }
        if config
            .excluded_domains
            .iter()
            .any(|d| crate::competitors::within(&host, d))
        {
            records.push(record_rejected(cand, "domain is configured excluded"));
            counters.candidates_rejected += 1;
            continue;
        }
        if is_blocked_platform(&host) {
            records.push(record_rejected(
                cand,
                "blocked platform (social/login-walled/UGC)",
            ));
            counters.candidates_rejected += 1;
            continue;
        }

        // ---- Gated fetch ----
        let fetch = gated_get(
            url.normalized.as_str(),
            &config.network.user_agent,
            &config.limits,
            transport,
            &mut robots_cache,
        );
        let fetch = match fetch {
            Ok(f) => f,
            Err(e) => {
                partial = true;
                let (outcome, note) = match &e {
                    FetchError::RobotsUnavailable(n) => (
                        CandidateOutcome::Rejected,
                        format!("robots unavailable: {n}"),
                    ),
                    FetchError::RobotsDisallowed(n) => (
                        CandidateOutcome::Rejected,
                        format!("robots disallowed: {n}"),
                    ),
                    FetchError::UnsafeTarget(n) => {
                        (CandidateOutcome::Rejected, format!("unsafe target: {n}"))
                    }
                    FetchError::BodyTooLarge => (
                        CandidateOutcome::Rejected,
                        "response exceeded size cap".to_string(),
                    ),
                    FetchError::RedirectLoop(n) => {
                        (CandidateOutcome::Error, format!("redirect loop: {n}"))
                    }
                    FetchError::UnsafeRedirect(n) => {
                        (CandidateOutcome::Rejected, format!("unsafe redirect: {n}"))
                    }
                    FetchError::Unreachable(n) => {
                        (CandidateOutcome::Error, format!("unreachable: {n}"))
                    }
                    FetchError::Other(n) => (CandidateOutcome::Error, n.clone()),
                };
                warnings.push(format!("source {} not verified: {note}", cand.url));
                let robots = match &e {
                    FetchError::RobotsDisallowed(n) => RobotsCheck {
                        status: "disallowed".into(),
                        http_status: None,
                        detail: Some(n.clone()),
                    },
                    FetchError::RobotsUnavailable(n) => RobotsCheck {
                        status: "unavailable".into(),
                        http_status: None,
                        detail: Some(n.clone()),
                    },
                    _ => robots_check_for(url.as_str(), &robots_cache),
                };
                if matches!(outcome, CandidateOutcome::Error)
                    || matches!(e, FetchError::RobotsUnavailable(_))
                {
                    counters.candidates_failed += 1;
                }
                if !matches!(outcome, CandidateOutcome::Error) {
                    counters.candidates_rejected += 1;
                }
                records.push(CandidateRecord {
                    url: cand.url.clone(),
                    final_url: None,
                    outcome,
                    notes: note,
                    http_status: None,
                    robots,
                    capture_file: None,
                    capture_sha256: None,
                    imported_query: cand.query.clone(),
                    imported_provider: cand.provider.clone(),
                    imported_collected_at: cand.collected_at.clone(),
                    opportunities: vec![],
                });
                continue;
            }
        };

        counters.candidates_fetched += 1;
        let body = String::from_utf8_lossy(&fetch.body).to_string();
        let capture = match evidence.as_mut() {
            Some(ev) => ev.save("page", &fetch.final_url, &fetch.body, "html")?,
            None => None,
        };
        let status_ok = (200..300).contains(&fetch.status);
        let final_source = safe_parse(&fetch.final_url).expect("validated fetch URL");
        let exclusion = if owned_hosts
            .iter()
            .any(|d| crate::competitors::within(final_source.host_ascii(), d))
        {
            Some("final source is a first-party/owned domain")
        } else if config
            .excluded_domains
            .iter()
            .any(|d| crate::competitors::within(final_source.host_ascii(), d))
        {
            Some("final source domain is configured excluded")
        } else if is_blocked_platform(final_source.host_ascii()) {
            Some("final source is a blocked platform")
        } else {
            None
        };
        if let Some(reason) = exclusion {
            let mut record = record_rejected(cand, reason);
            record.final_url = Some(fetch.final_url.clone());
            record.http_status = Some(fetch.status);
            record.robots = robots_check_for(&fetch.final_url, &robots_cache);
            record.capture_file = capture.as_ref().map(|(f, _)| f.clone());
            record.capture_sha256 = capture.as_ref().map(|(_, h)| h.clone());
            records.push(record);
            counters.candidates_rejected += 1;
            continue;
        }

        // ---- Shared facts ----
        let facts = PageFacts {
            source_url: fetch.final_url.clone(),
            observed_status: fetch.status,
            checked_at_utc: web_time_utc(),
            redirect_chain: fetch.redirect_chain.clone(),
            final_url: fetch.final_url.clone(),
            owned_hosts: owned_hosts.clone(),
            brands: config.brands.clone(),
            target_url: targets
                .first()
                .map(|t| t.url.clone())
                .unwrap_or_else(|| site.normalized.to_string()),
            targets: targets
                .iter()
                .map(|t| (t.url.clone(), t.topics.clone()))
                .collect(),
            outbound_limit: config.limits.outbound_links_per_page.saturating_add(1),
        };

        if !status_ok {
            partial = true;
            warnings.push(format!(
                "source {} inaccessible: HTTP {}",
                fetch.final_url, fetch.status
            ));
            // Non-200 source page (e.g. 404): the source itself is rejected.
            records.push(CandidateRecord {
                url: cand.url.clone(),
                final_url: Some(fetch.final_url.clone()),
                outcome: CandidateOutcome::Rejected,
                notes: format!(
                    "source page HTTP {} (non-200 source rejected)",
                    fetch.status
                ),
                http_status: Some(fetch.status),
                robots: robots_check_for(&fetch.final_url, &robots_cache),
                capture_file: capture.as_ref().map(|(f, _)| f.clone()),
                capture_sha256: capture.as_ref().map(|(_, h)| h.clone()),
                imported_query: cand.query.clone(),
                imported_provider: cand.provider.clone(),
                imported_collected_at: cand.collected_at.clone(),
                opportunities: vec![],
            });
            counters.candidates_rejected += 1;
            continue;
        }

        // ---- Analyze for mention + curated-lead rules ----
        let analysis = detector::analyze_page(&body, &facts);
        let mut rec_opps: Vec<Opportunity> = Vec::new();
        if !analysis.links_to_owned_domain && !cand.backlink_hints.is_empty() {
            if let Some(gap) = crate::gap::observe(&body, &facts, config, &competitors) {
                rec_opps.push(gap);
            }
        }
        if let Some(m) = analysis.mention {
            rec_opps.push(m);
        }
        if let Some(cl) = analysis.curated_lead {
            rec_opps.push(cl);
        }

        // ---- Rule 3: broken-link replacement possibilities (bounded) ----
        if !targets.is_empty() {
            if analysis.outbound.len() > config.limits.outbound_links_per_page {
                partial = true;
                outbound_limit_reached = true;
                warnings.push(format!(
                    "outbound per-page limit {} reached on {}",
                    config.limits.outbound_links_per_page, fetch.final_url
                ));
            }
            for link in analysis
                .outbound
                .iter()
                .take(config.limits.outbound_links_per_page)
            {
                if outbound_used >= config.limits.outbound_checks {
                    outbound_limit_reached = true;
                    partial = true;
                    warnings.push(format!(
                        "outbound limit {} reached; verification skipped on {}",
                        config.limits.outbound_checks, fetch.final_url
                    ));
                    break;
                }
                outbound_used += 1;
                let dest = match gated_get(
                    &link.href,
                    &config.network.user_agent,
                    &config.limits,
                    transport,
                    &mut robots_cache,
                ) {
                    Ok(d) => d,
                    Err(FetchError::UnsafeTarget(n)) => {
                        partial = true;
                        warnings.push(format!(
                            "outbound {} skipped: unsafe target: {}",
                            link.href, n
                        ));
                        continue;
                    }
                    Err(FetchError::RobotsUnavailable(n)) => {
                        partial = true;
                        warnings.push(format!(
                            "outbound {} skipped: robots unavailable: {}",
                            link.href, n
                        ));
                        continue;
                    }
                    Err(FetchError::RobotsDisallowed(n)) => {
                        partial = true;
                        warnings.push(format!(
                            "outbound {} skipped: robots disallowed: {}",
                            link.href, n
                        ));
                        continue;
                    }
                    Err(e) => {
                        partial = true;
                        warnings.push(format!("outbound {} skipped: {e}", link.href));
                        continue;
                    }
                };
                if !(200..300).contains(&dest.status) && !matches!(dest.status, 404 | 410) {
                    partial = true;
                    warnings.push(format!(
                        "outbound {} could not be verified: HTTP {}",
                        link.href, dest.status
                    ));
                    continue;
                }
                if matches!(dest.status, 404 | 410) {
                    // Topic relevance is INFERRED and must match configured
                    // target topics against anchor + surrounding page text.
                    let page_topics: Vec<(&Tp, String)> = targets
                        .iter()
                        .filter_map(|t| {
                            t.topics.iter().find_map(|topic| {
                                sentence_containing(&body, topic).map(|quote| (t, quote))
                            })
                        })
                        .collect();
                    if page_topics.is_empty() {
                        warnings.push(format!(
                            "outbound {} is 404/410 but anchor/topic does not match any configured target topics; not recorded as an opportunity",
                            link.href
                        ));
                        continue;
                    }
                    let (chosen, quote) = &page_topics[0];
                    let mut opp = detector::build_broken_link_opportunity(
                        &facts,
                        &link.href,
                        Some(dest.final_url.clone()),
                        dest.status,
                        web_time_utc(),
                        dest.redirect_chain.clone(),
                        &link.anchor_text,
                        quote,
                        analysis.links_to_owned_domain,
                    );
                    opp.observed_facts
                        .push(format!("matched visible source text: {quote}"));
                    opp.target_url = chosen.url.clone();
                    rec_opps.push(opp);
                }
            }
        }

        // ---- Record ----
        let outcome = if rec_opps.is_empty() {
            counters.candidates_no_opportunity += 1;
            CandidateOutcome::NoOpportunity
        } else {
            counters.opportunities_total += rec_opps.len();
            CandidateOutcome::Opportunities
        };
        records.push(CandidateRecord {
            url: cand.url.clone(),
            final_url: Some(fetch.final_url.clone()),
            outcome,
            notes: build_notes(&rec_opps),
            http_status: Some(fetch.status),
            robots: robots_check_for(&fetch.final_url, &robots_cache),
            capture_file: capture.as_ref().map(|(f, _)| f.clone()),
            capture_sha256: capture.as_ref().map(|(_, h)| h.clone()),
            imported_query: cand.query.clone(),
            imported_provider: cand.provider.clone(),
            imported_collected_at: cand.collected_at.clone(),
            opportunities: rec_opps.clone(),
        });
        opportunities.append(&mut rec_opps);
    }

    let finished = web_time_utc();

    counters.outbound_checked = outbound_used;
    counters.outbound_limit_reached = outbound_limit_reached;
    counters.candidate_limit_reached = candidate_limit_reached;

    let run_status = if partial {
        "partial"
    } else {
        "completed_with_individual_exclusions"
    };
    let zero_note = if opportunities.is_empty() {
        Some(
            "No opportunities met the evidence rules in this run. Empty categories are honest: \
             zero detected does not mean zero existing on the web, only zero verified by this run."
                .to_string(),
        )
    } else {
        None
    };

    let mut audit = BTreeMap::new();
    audit.insert(
        "duplicates_removed".to_string(),
        duplicates.len().to_string(),
    );
    audit.insert(
        "evidence_files".to_string(),
        evidence
            .as_ref()
            .map(|e| e.files.len().to_string())
            .unwrap_or_else(|| "0".to_string()),
    );

    Ok(RunReport {
        schema_version: REPORT_SCHEMA_VERSION,
        tool: TOOL_NAME,
        tool_version: TOOL_VERSION.to_string(),
        rule_engine_version: RULE_ENGINE_VERSION.to_string(),
        config_file: cfg_path.to_string(),
        candidates_file: cand_path.to_string(),
        started_at_utc: started,
        finished_at_utc: finished,
        run_status: run_status.to_string(),
        partial,
        network_enabled: config.network.enabled,
        counters,
        evidence_dir: config.network.evidence_dir.clone(),
        candidates: records,
        opportunities,
        warnings,
        zero_result_note: zero_note,
        audit,
        backlink_imports: candidates
            .iter()
            .filter(|c| !c.backlink_hints.is_empty())
            .map(|c| (c.url.clone(), c.backlink_hints.clone()))
            .collect(),
    })
}

fn build_notes(opps: &[Opportunity]) -> String {
    let rules: Vec<&str> = opps.iter().map(|o| o.rule.as_str()).collect();
    if rules.is_empty() {
        "fetched and analyzed; no rule produced an opportunity".to_string()
    } else {
        format!("fetched and analyzed; rules matched: {}", rules.join(", "))
    }
}

fn record_rejected(cand: &ImportedCandidate, reason: &str) -> CandidateRecord {
    CandidateRecord {
        url: cand.url.clone(),
        final_url: None,
        outcome: CandidateOutcome::Rejected,
        notes: reason.to_string(),
        http_status: None,
        robots: RobotsCheck {
            status: "not_applicable".to_string(),
            http_status: None,
            detail: None,
        },
        capture_file: None,
        capture_sha256: None,
        imported_query: cand.query.clone(),
        imported_provider: cand.provider.clone(),
        imported_collected_at: cand.collected_at.clone(),
        opportunities: vec![],
    }
}

fn robots_check_for(
    raw_url: &str,
    cache: &HashMap<String, crate::fetch::RobotsOutcome>,
) -> RobotsCheck {
    let url = safe_parse(raw_url).expect("validated URL for robots reporting");
    match cache.get(&url.origin_key()) {
        Some(outcome) => {
            let (status, detail) = match outcome {
                crate::fetch::RobotsOutcome::Fetched(rules) => (
                    if rules.path_allowed(&url.path_and_query()) {
                        "allowed"
                    } else {
                        "disallowed"
                    },
                    Some(format!(
                        "robots.txt rules applied to {}",
                        url.path_and_query()
                    )),
                ),
                crate::fetch::RobotsOutcome::Absent => (
                    "allowed",
                    Some("robots.txt absent (404/410); no restrictions".to_string()),
                ),
                crate::fetch::RobotsOutcome::Unavailable(r) => ("unavailable", Some(r.clone())),
            };
            RobotsCheck {
                status: status.to_string(),
                http_status: None,
                detail,
            }
        }
        None => RobotsCheck {
            status: "not_applicable".to_string(),
            http_status: None,
            detail: None,
        },
    }
}

/// Platforms excluded as candidate sources: login-walled or UGC platforms
/// where automated extraction is blocked or meaningless.
fn is_blocked_platform(host: &str) -> bool {
    const BLOCKED: &[&str] = &[
        "facebook.com",
        "instagram.com",
        "quora.com",
        "reddit.com",
        "twitter.com",
        "x.com",
        "linkedin.com",
        "sites.google.com",
        "groups.google.com",
        "youtube.com",
        "pinterest.com",
        "tiktok.com",
        "threads.net",
    ];
    BLOCKED
        .iter()
        .any(|b| host == *b || host.ends_with(&format!(".{b}")))
}
