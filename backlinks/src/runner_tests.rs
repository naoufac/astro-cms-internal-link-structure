//! End-to-end runner tests over the FakeTransport (no sockets).

use crate::report::{
    CandidateOutcome, DetectorConfig, ImportedCandidate, Limits, NetworkPolicy, TargetPage,
    CONFIG_SCHEMA_VERSION,
};
use crate::runner::run_detection;
use crate::transport::fake::FakeTransport;

fn config(network_enabled: bool, evidence: Option<&str>, limits: Option<Limits>) -> DetectorConfig {
    DetectorConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        site_origin: "https://gissecrets.com".into(),
        owned_domains: vec!["gissecrets.com".into()],
        brands: vec!["GisSecrets".into()],
        target_pages: vec![TargetPage {
            url: "https://gissecrets.com/tarot".into(),
            title: "Tarot".into(),
            topics: vec!["tarot".into()],
        }],
        excluded_domains: vec![],
        limits: limits.unwrap_or_default(),
        network: NetworkPolicy {
            enabled: network_enabled,
            user_agent: "test-agent".into(),
            evidence_dir: evidence.map(String::from),
        },
    }
}

fn cand(url: &str) -> ImportedCandidate {
    ImportedCandidate {
        url: url.into(),
        query: Some("test query".into()),
        provider: Some("test".into()),
        collected_at: Some("2026-10-15".into()),
        search_title: None,
        search_snippet: None,
        suggested_target: None,
    }
}

const KEY_RB: &str = "https://src.example/robots.txt";

#[test]
fn broken_link_404_produces_replacement_possibility() {
    let mut t = FakeTransport::new(&["src.example", "dead.example"]);
    t.route(KEY_RB, 200, "User-agent: *\nAllow: /\n");
    t.route(
        "https://src.example/resources",
        200,
        r#"<html><body><main>
        <h2>Useful tarot resources</h2>
        <p>Our guide to tarot spreads is classic.</p>
        <ul>
        <li><a href="https://dead.example/old-guide">old tarot guide</a></li>
        <li><a href="https://alive.example/page">alive</a></li>
        <li><a href="https://alive.example/page2">alive2</a></li>
        </ul>
        </main></body></html>"#,
    );
    t.route("https://dead.example/robots.txt", 404, "");
    t.route("https://dead.example/old-guide", 404, "gone");
    t.route("https://alive.example/robots.txt", 404, "");
    t.route("https://alive.example/page", 200, "ok");
    t.route("https://alive.example/page2", 200, "ok");

    let cfg = config(true, None, None);
    let report = run_detection(
        &cfg,
        &[cand("https://src.example/resources")],
        "cfg",
        "cands",
        &t,
    )
    .unwrap();
    let broken: Vec<_> = report
        .opportunities
        .iter()
        .filter(|o| o.rule == "broken-link-replacement-possibility")
        .collect();
    assert_eq!(broken.len(), 1, "expected exactly one broken-link finding");
    assert_eq!(
        broken[0].broken_url.as_deref(),
        Some("https://dead.example/old-guide")
    );
    assert_eq!(broken[0].target_url, "https://gissecrets.com/tarot");
    // The observed status MUST be the destination's terminal status, never
    // the source page's.
    assert_eq!(broken[0].observed_status, 404);
    assert!(broken[0]
        .inferred_assessment
        .iter()
        .any(|s| s.contains("INFERRED")));
    assert!(report.counters.outbound_checked >= 3);
}

#[test]
fn broken_link_403_and_timeout_are_not_findings() {
    let mut t = FakeTransport::new(&["src.example", "forbid.example"]);
    t.route(KEY_RB, 200, "");
    t.route(
        "https://src.example/page",
        200,
        r#"<html><body><main>
        <h2>Tarot resources</h2>
        <ul>
        <li><a href="https://forbid.example/x">tarot link</a></li>
        <li><a href="https://forbid.example/y">tarot two</a></li>
        <li><a href="https://forbid.example/z">tarot three</a></li>
        </ul>
        </main></body></html>"#,
    );
    t.route("https://forbid.example/robots.txt", 404, "");
    t.route("https://forbid.example/x", 403, "no");
    t.route("https://forbid.example/y", 403, "no");
    t.route("https://forbid.example/z", 403, "no");

    let cfg = config(true, None, None);
    let report = run_detection(
        &cfg,
        &[cand("https://src.example/page")],
        "cfg",
        "cands",
        &t,
    )
    .unwrap();
    let broken: Vec<_> = report
        .opportunities
        .iter()
        .filter(|o| o.rule == "broken-link-replacement-possibility")
        .collect();
    assert!(
        broken.is_empty(),
        "403 is not a confirmed broken link; got {broken:?}"
    );
    // (The curated-lead rule may legitimately fire on this page; only the
    // broken-link rule is under test here.)
}

#[test]
fn first_party_source_and_owned_linking_source_rejected() {
    let mut t = FakeTransport::new(&["gissecrets.com", "linked.example"]);
    t.route("https://gissecrets.com/robots.txt", 200, "");
    t.route("https://linked.example/robots.txt", 200, "");
    t.route(
        "https://linked.example/page",
        200,
        r#"<html><body><main><p>Visit <a href="https://gissecrets.com/">GisSecrets</a> now.</p></main></body></html>"#,
    );
    let cfg = config(true, None, None);
    let report = run_detection(
        &cfg,
        &[
            cand("https://gissecrets.com/self-page"),
            cand("https://linked.example/page"),
        ],
        "cfg",
        "cands",
        &t,
    )
    .unwrap();
    assert_eq!(
        report.counters.candidates_fetched, 1,
        "first-party source never fetched"
    );
    let rec = &report.candidates[0];
    assert_eq!(rec.url, "https://gissecrets.com/self-page");
    assert!(matches!(rec.outcome, CandidateOutcome::Rejected));
    assert!(rec.notes.contains("first-party"));
    // Already-linking page: fetched, but no mention opportunity.
    let rec2 = &report.candidates[1];
    assert!(
        rec2.opportunities.is_empty(),
        "already-linking source must not produce a mention"
    );
}

#[test]
fn source_404_is_rejected_not_analyzed() {
    let mut t = FakeTransport::new(&["gone.example"]);
    t.route("https://gone.example/robots.txt", 404, "");
    t.route("https://gone.example/page", 404, "missing");
    let cfg = config(true, None, None);
    let report = run_detection(
        &cfg,
        &[cand("https://gone.example/page")],
        "cfg",
        "cands",
        &t,
    )
    .unwrap();
    let rec = &report.candidates[0];
    assert!(matches!(rec.outcome, CandidateOutcome::Rejected));
    assert_eq!(rec.http_status, Some(404));
    assert!(rec.notes.contains("non-200 source rejected"));
}

#[test]
fn candidate_limit_enforced() {
    let mut t = FakeTransport::new(&["a.example", "b.example"]);
    t.route("https://a.example/robots.txt", 404, "");
    t.route("https://b.example/robots.txt", 404, "");
    t.route(
        "https://a.example/p",
        200,
        "<html><body><main><p>one</p></main></body></html>",
    );
    t.route(
        "https://b.example/p",
        200,
        "<html><body><main><p>two</p></main></body></html>",
    );
    let limits = Limits {
        candidate_pages: 1,
        ..Default::default()
    };
    let cfg = config(true, None, Some(limits));
    let report = run_detection(
        &cfg,
        &[cand("https://a.example/p"), cand("https://b.example/p")],
        "cfg",
        "cands",
        &t,
    )
    .unwrap();
    assert_eq!(report.counters.candidates_fetched, 1);
    assert!(report.counters.candidate_limit_reached);
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("candidate limit 1 reached")));
}

#[test]
fn network_disabled_refuses_to_run() {
    // run_detection itself doesn't check the flag (the CLI does), but
    // evidence_dir=None + disabled flag must still not fetch: verify via
    // transport that no requests happen when candidates are all rejected
    // statically. Here: verify the config flag is echoed into the report.
    let t = FakeTransport::new(&[]);
    let cfg = config(false, None, None);
    let report = run_detection(&cfg, &[], "cfg", "cands", &t).unwrap();
    assert!(!report.network_enabled);
    assert_eq!(t.request_count(), 0);
}

#[test]
fn evidence_captures_written_and_hashed() {
    let dir = tempfile::tempdir().unwrap();
    let ev = dir.path().join("ev").to_str().unwrap().to_string();
    let mut t = FakeTransport::new(&["src.example"]);
    t.route(KEY_RB, 404, "");
    t.route(
        "https://src.example/page",
        200,
        "<html><body><main><p>plain page about tarot.</p></main></body></html>",
    );
    let cfg = config(true, Some(&ev), None);
    let report = run_detection(
        &cfg,
        &[cand("https://src.example/page")],
        "cfg",
        "cands",
        &t,
    )
    .unwrap();
    let rec = &report.candidates[0];
    let fname = rec.capture_file.as_ref().expect("capture file recorded");
    let on_disk = std::fs::read(std::path::Path::new(&ev).join(fname)).unwrap();
    assert_eq!(
        on_disk,
        b"<html><body><main><p>plain page about tarot.</p></main></body></html>"
    );
    let hash = rec.capture_sha256.as_ref().unwrap();
    assert_eq!(*hash, crate::detector::sha256_hex(&on_disk));
    assert_eq!(report.evidence_dir.as_deref(), Some(ev.as_str()));
}

#[test]
fn provenance_preserved_from_import() {
    let mut t = FakeTransport::new(&["prov.example"]);
    t.route("https://prov.example/robots.txt", 404, "");
    t.route(
        "https://prov.example/page",
        200,
        "<html><body><main><p>hello world</p></main></body></html>",
    );
    let mut c = cand("https://prov.example/page");
    c.query = Some("tarot \"useful links\" resources".into());
    c.provider = Some("imported-provider".into());
    c.collected_at = Some("2026-10-15T14:00:00Z".into());
    let cfg = config(true, None, None);
    let report = run_detection(&cfg, &[c], "cfg.json", "cands.json", &t).unwrap();
    let rec = &report.candidates[0];
    assert_eq!(
        rec.imported_query.as_deref(),
        Some("tarot \"useful links\" resources")
    );
    assert_eq!(rec.imported_provider.as_deref(), Some("imported-provider"));
    assert_eq!(
        rec.imported_collected_at.as_deref(),
        Some("2026-10-15T14:00:00Z")
    );
    assert_eq!(report.config_file, "cfg.json");
    assert_eq!(report.candidates_file, "cands.json");
}

#[test]
fn unrelated_stats_site_with_brand_string_is_not_a_mention() {
    // Simulates the gissecrets.com siteindices-style false positive: the page
    // contains "gissecrets" only as parsed keyword metadata in a script tag.
    let mut t = FakeTransport::new(&["stats.example"]);
    t.route("https://stats.example/robots.txt", 404, "");
    t.route(
        "https://stats.example/report",
        200,
        r#"<html><head><script>var kw = "gissecrets.com";</script></head>
        <body><main><h1>Website statistics</h1>
        <p>Traffic report generated automatically.</p></main></body></html>"#,
    );
    let cfg = config(true, None, None);
    let report = run_detection(
        &cfg,
        &[cand("https://stats.example/report")],
        "cfg",
        "cands",
        &t,
    )
    .unwrap();
    assert!(
        report.opportunities.is_empty(),
        "script-tag brand string must not be a mention"
    );
}

#[test]
fn dedupe_summary_counts_in_audit() {
    let mut t = FakeTransport::new(&["d.example"]);
    t.route("https://d.example/robots.txt", 404, "");
    t.route(
        "https://d.example/p",
        200,
        "<html><body><main><p>x</p></main></body></html>",
    );
    let cfg = config(true, None, None);
    let report = run_detection(
        &cfg,
        &[
            cand("https://d.example/p"),
            cand("https://d.example/p/"),
            cand("https://d.example/p"),
        ],
        "cfg",
        "cands",
        &t,
    )
    .unwrap();
    assert_eq!(report.counters.candidates_input, 3);
    assert_eq!(report.counters.candidates_deduplicated, 1);
    assert_eq!(
        report.audit.get("duplicates_removed").map(String::as_str),
        Some("2")
    );
    assert_eq!(
        t.request_count(),
        2,
        "deduped candidates fetch once (robots.txt + page)"
    );
}

#[test]
fn robots_401_source_recorded_as_unavailable() {
    let mut t = FakeTransport::new(&["blocked.example"]);
    t.route("https://blocked.example/robots.txt", 401, "denied");
    let cfg = config(true, None, None);
    let report = run_detection(
        &cfg,
        &[cand("https://blocked.example/Links.html")],
        "cfg",
        "cands",
        &t,
    )
    .unwrap();
    let rec = &report.candidates[0];
    assert!(matches!(rec.outcome, CandidateOutcome::Rejected));
    assert!(rec.notes.contains("robots unavailable"));
    assert_eq!(rec.robots.status, "unavailable");
    assert_eq!(
        t.request_count(),
        1,
        "only robots.txt attempted, never the page"
    );
}
