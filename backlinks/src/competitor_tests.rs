use crate::{
    competitors::{parse_backlinks, validate},
    report::{CandidateOutcome, DetectorConfig},
    runner::run_detection,
    transport::fake::FakeTransport,
};
use serde_json::json;

fn config() -> DetectorConfig {
    serde_json::from_value(json!({"schema_version":1,"site_origin":"https://owned.example","owned_domains":["owned.example"],"brands":[],"target_pages":[{"url":"https://owned.example/tarot","title":"Tarot","topics":["tarot"]}],"competitor_domains":["one.example","two.example"],"limits":{"per_origin_delay_ms":0,"outbound_checks":0},"network":{"enabled":true,"user_agent":"fixture"}})).unwrap()
}
fn imported() -> String {
    json!([{"source_url":"https://source.example/page?topic=tarot","competitor_url":"https://one.example/a","provenance":"synthetic fixture A"},{"source_url":"https://source.example/page?topic=tarot","competitor_url":"https://two.example/b","provenance":"synthetic fixture B"}]).to_string()
}
#[test]
fn two_competitor_gap_requires_observed_links_and_retains_import_provenance() {
    let cfg = config();
    let candidates = parse_backlinks(&imported(), &cfg).unwrap();
    let mut t = FakeTransport::new(&["source.example"]);
    t.route(
        "https://source.example/robots.txt",
        200,
        "User-agent: *\nAllow: /\n",
    );
    t.route("https://source.example/page?topic=tarot", 200, "<nav><a href='/about'>About</a></nav><main><p>Tarot research.</p><a href='https://one.example/a'>One tarot</a><a href='https://www.two.example/b'>Two tarot</a></main>");
    let r = run_detection(&cfg, &candidates, "fixture", "fixture", &t).unwrap();
    let gap = r
        .opportunities
        .iter()
        .find(|o| o.rule == "competitor-backlink-gap-lead")
        .unwrap();
    assert_eq!(gap.competitor_evidence.as_ref().unwrap().domains.len(), 2);
    assert_eq!(gap.target_url, "https://owned.example/tarot");
    assert_eq!(r.backlink_imports.values().next().unwrap().len(), 2);
    assert_eq!(t.request_count(), 2);
}
#[test]
fn rejects_owned_overlapping_excluded_and_alias_competitor_definitions() {
    for domains in [
        vec!["owned.example"],
        vec!["one.example", "www.one.example"],
        vec!["one.example", "sub.one.example"],
        vec!["bad/path"],
        vec!["localhost"],
    ] {
        let mut cfg = config();
        cfg.competitor_domains = domains.into_iter().map(String::from).collect();
        assert!(validate(&cfg).is_err());
    }
    let mut cfg = config();
    cfg.excluded_domains = vec!["one.example".into()];
    assert!(validate(&cfg).is_err());
}

#[test]
fn imports_group_exact_source_pages_preserve_queries_and_count_distinct_identities() {
    let cfg = config();
    let repeated = json!([
        {"source_url":"https://source.example/p?q=1","competitor_url":"https://one.example/a","provenance":"fixture"},
        {"source_url":"https://source.example/p?q=1","competitor_url":"https://www.one.example/b","provenance":"fixture"}
    ]);
    assert!(parse_backlinks(&repeated.to_string(), &cfg)
        .unwrap()
        .is_empty());
    let mut rows = repeated.as_array().unwrap().clone();
    rows.push(json!({"source_url":"https://source.example/p?q=2","competitor_url":"https://two.example/a","provenance":"fixture"}));
    assert!(
        parse_backlinks(&serde_json::to_string(&rows).unwrap(), &cfg)
            .unwrap()
            .is_empty()
    );
    rows.push(json!({"source_url":"https://SOURCE.example:443/p?q=1#fragment","competitor_url":"https://two.example/a","provenance":"fixture"}));
    let candidates = parse_backlinks(&serde_json::to_string(&rows).unwrap(), &cfg).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].url, "https://source.example/p?q=1");
    assert_eq!(candidates[0].backlink_hints.len(), 3);
    assert!(parse_backlinks("[]", &cfg).unwrap().is_empty());
    for invalid in [
        "not json",
        "{}",
        "[{}]",
        r#"[{"source_url":"http://127.0.0.1/x","competitor_url":"https://one.example/","provenance":"fixture"}]"#,
        r#"[{"source_url":"https://source.example/p","competitor_url":"https://unknown.example/","provenance":"fixture"}]"#,
        r#"[{"source_url":"https://source.example/p","competitor_url":"https://one.example/","provenance":""}]"#,
    ] {
        assert!(parse_backlinks(invalid, &cfg).is_err(), "{invalid}");
    }
}

#[test]
fn imported_hints_never_replace_live_evidence_or_owned_exclusions() {
    let cfg = config();
    let candidates = parse_backlinks(&imported(), &cfg).unwrap();
    for html in [
        "<p>Tarot research.</p>",
        "<p>Tarot research.</p><a href='https://one.example/a'>One</a><a href='https://www.one.example/b'>alias</a>",
        "<p>Tarot research.</p><a href='https://one.example/a'>One</a><a href='https://two.example/b'>Two</a><a href='https://blog.owned.example/'>Owned subdomain</a>",
        "<p>Unrelated cooking.</p><a href='https://one.example/a'>One</a><a href='https://two.example/b'>Two</a>",
    ] {
        let mut t = FakeTransport::new(&["source.example"]);
        t.route("https://source.example/robots.txt", 404, "");
        t.route("https://source.example/page?topic=tarot", 200, html);
        let r = run_detection(&cfg, &candidates, "fixture", "fixture", &t).unwrap();
        assert!(r.opportunities.iter().all(|o| o.rule != "competitor-backlink-gap-lead"), "{html}");
    }
}

#[test]
fn two_rows_same_source_different_competitors_group_into_candidate() {
    let cfg = config();
    let rows = json!([
        {"source_url":"https://source.example/page?topic=tarot","competitor_url":"https://one.example/a","provenance":"fixture row 1"},
        {"source_url":"https://source.example/page?topic=tarot","competitor_url":"https://two.example/b","provenance":"fixture row 2"}
    ]);
    let candidates = parse_backlinks(&rows.to_string(), &cfg).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].url, "https://source.example/page?topic=tarot");
    assert_eq!(candidates[0].backlink_hints.len(), 2);
    assert_eq!(
        candidates[0].provider.as_deref(),
        Some("imported-backlinks")
    );
}

#[test]
fn single_competitor_across_rows_never_becomes_candidate() {
    let cfg = config();
    let rows = json!([
        {"source_url":"https://source.example/only","competitor_url":"https://one.example/a","provenance":"fixture"},
        {"source_url":"https://source.example/only","competitor_url":"https://www.one.example/b","provenance":"fixture"},
        {"source_url":"https://source.example/only","competitor_url":"https://one.example/c","provenance":"fixture"}
    ]);
    assert!(parse_backlinks(&rows.to_string(), &cfg).unwrap().is_empty());
}

#[test]
fn owned_link_disqualifies_including_www_and_apex_owned_variants() {
    let cfg = config();
    let candidates = parse_backlinks(&imported(), &cfg).unwrap();
    for html in [
        "<p>Tarot research.</p><a href='https://one.example/a'>One</a><a href='https://two.example/b'>Two</a><a href='https://owned.example/'>Owned apex</a>",
        "<p>Tarot research.</p><a href='https://one.example/a'>One</a><a href='https://two.example/b'>Two</a><a href='https://www.owned.example/'>Owned www</a>",
        "<p>Tarot research.</p><a href='https://one.example/a'>One</a><a href='https://two.example/b'>Two</a><a href='https://blog.owned.example/'>Owned subdomain</a>",
    ] {
        let mut t = FakeTransport::new(&["source.example"]);
        t.route("https://source.example/robots.txt", 200, "User-agent: *\nAllow: /\n");
        t.route("https://source.example/page?topic=tarot", 200, html);
        let r = run_detection(&cfg, &candidates, "fixture", "fixture", &t).unwrap();
        assert!(
            r.opportunities.iter().all(|o| o.rule != "competitor-backlink-gap-lead"),
            "owned link variant must disqualify: {html}"
        );
    }
    let mut t = FakeTransport::new(&["source.example"]);
    t.route(
        "https://source.example/robots.txt",
        200,
        "User-agent: *\nAllow: /\n",
    );
    t.route("https://source.example/page?topic=tarot", 200, "<nav><a href='/tarot'>Site tarot</a></nav><main><p>Tarot research.</p><a href='https://one.example/a'>One</a><a href='https://two.example/b'>Two</a></main>");
    let r = run_detection(&cfg, &candidates, "fixture", "fixture", &t).unwrap();
    let gap = r
        .opportunities
        .iter()
        .find(|o| o.rule == "competitor-backlink-gap-lead")
        .expect("same-origin anchor is not an owned-domain link and must not disqualify");
    assert_eq!(gap.competitor_evidence.as_ref().unwrap().domains.len(), 2);
}

#[test]
fn unavailable_fetch_produces_no_opportunity_and_honest_zero_note() {
    let cfg = config();
    let candidates = parse_backlinks(&imported(), &cfg).unwrap();
    let mut t = FakeTransport::new(&["source.example"]);
    t.route(
        "https://source.example/robots.txt",
        503,
        "upstream unavailable",
    );
    let r = run_detection(&cfg, &candidates, "fixture", "fixture", &t).unwrap();
    assert!(r.opportunities.is_empty());
    assert_eq!(r.counters.opportunities_total, 0);
    assert!(r.zero_result_note.is_some());
    let note = r.zero_result_note.unwrap();
    assert!(note.contains("zero detected does not mean zero existing on the web"));
    let rec = r
        .candidates
        .iter()
        .find(|c| c.url == "https://source.example/page?topic=tarot")
        .unwrap();
    assert!(matches!(rec.outcome, CandidateOutcome::Rejected));
    assert!(rec.notes.contains("robots unavailable"));
    assert!(rec.http_status.is_none());
}

#[test]
fn gap_sources_obey_robots_dns_redirects_status_and_network_opt_in() {
    let cfg = config();
    let candidates = parse_backlinks(&imported(), &cfg).unwrap();
    for (robots_status, robots, status) in [
        (200, "User-agent: *\nDisallow: /", 200),
        (503, "unavailable", 200),
        (200, "", 404),
        (200, "", 403),
    ] {
        let mut t = FakeTransport::new(&["source.example"]);
        t.route("https://source.example/robots.txt", robots_status, robots);
        t.route("https://source.example/page?topic=tarot", status, "<p>Tarot.</p><a href='https://one.example/a'>One</a><a href='https://two.example/b'>Two</a>");
        let r = run_detection(&cfg, &candidates, "fixture", "fixture", &t).unwrap();
        assert!(r.opportunities.is_empty());
    }
    let mut t = FakeTransport::new(&["source.example"]);
    t.dns
        .insert("source.example".into(), vec!["127.0.0.1".parse().unwrap()]);
    let r = run_detection(&cfg, &candidates, "fixture", "fixture", &t).unwrap();
    assert!(r.opportunities.is_empty());
    assert_eq!(t.request_count(), 0);
    let mut disabled = cfg.clone();
    disabled.network.enabled = false;
    assert!(run_detection(&disabled, &candidates, "fixture", "fixture", &t).is_err());
    assert_eq!(t.request_count(), 0);
    let mut t = FakeTransport::new(&["source.example"]);
    t.route("https://source.example/robots.txt", 404, "");
    t.route_redirect(
        "https://source.example/page?topic=tarot",
        302,
        "http://127.0.0.1/private",
    );
    assert!(run_detection(&cfg, &candidates, "fixture", "fixture", &t)
        .unwrap()
        .opportunities
        .is_empty());
    let owned_import = imported().replace("source.example", "blog.owned.example");
    let owned = parse_backlinks(&owned_import, &cfg).unwrap();
    let t = FakeTransport::new(&[]);
    let r = run_detection(&cfg, &owned, "fixture", "fixture", &t).unwrap();
    assert!(r.opportunities.is_empty());
    assert_eq!(t.request_count(), 0);
    let r = run_detection(&cfg, &[], "fixture", "fixture", &t).unwrap();
    assert!(r.opportunities.is_empty());
    assert!(r.zero_result_note.is_some());
}
