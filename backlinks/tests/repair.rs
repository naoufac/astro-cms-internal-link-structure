use backlink_opportunities_detector::{
    competitors, detector,
    evidence::Evidence,
    fetch::{self, RobotsRules},
    import::parse_candidates,
    neturl::safe_parse,
    report::{CandidateOutcome, DetectorConfig, ImportedCandidate, RunReport},
    runner::run_detection,
    text,
    transport::fake::FakeTransport,
};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::process::Command;

fn config() -> DetectorConfig {
    serde_json::from_value(json!({
        "schema_version":1,"site_origin":"https://owned.example",
        "owned_domains":["owned.example"],"brands":["Brand Name"],
        "target_pages":[{"url":"https://owned.example/tarot","title":"Tarot","topics":["tarot"]}],
        "competitor_domains":["one.example","two.example"],
        "limits":{"per_origin_delay_ms":0},
        "network":{"enabled":true,"user_agent":"repair-agent"}
    }))
    .unwrap()
}
fn cand(url: &str) -> ImportedCandidate {
    serde_json::from_value(json!({"url":url})).unwrap()
}
fn transport(body: &str) -> FakeTransport {
    let mut t = FakeTransport::new(&[
        "source.example",
        "owned.example",
        "reddit.com",
        "dead.example",
        "one.example",
        "two.example",
    ]);
    for host in [
        "source.example",
        "owned.example",
        "reddit.com",
        "dead.example",
        "one.example",
        "two.example",
    ] {
        t.route(&format!("https://{host}/robots.txt"), 404, "");
    }
    t.route("https://source.example/p", 200, body);
    t
}
fn run(cfg: &DetectorConfig, t: &FakeTransport) -> RunReport {
    run_detection(
        cfg,
        &[cand("https://source.example/p")],
        "fixture",
        "fixture",
        t,
    )
    .unwrap()
}
const MENTION: &str = "<main><p>Brand Name tarot.</p></main>";

#[test]
fn b08_inaccessible_outbound_status_marks_partial() {
    for status in [401, 403, 429, 500, 503] {
        let mut t =
            transport("<main><p>Cooking.</p><a href='https://dead.example/a'>Old guide</a></main>");
        t.route("https://dead.example/a", status, "unavailable");
        let r = run(&config(), &t);
        assert!(r.partial, "HTTP {status}");
        assert!(!r.warnings.is_empty());
        assert_eq!(r.run_status, "partial");
        assert!(r.opportunities.is_empty());
    }
}

#[test]
fn b03_blank_lines_and_mixed_agent_groups_preserve_rules() {
    for robots in [
        "User-agent: *\n\nDisallow: /private\n",
        "User-agent: *\nUser-agent: repair-agent\nDisallow: /private\n\nUser-agent: rep\nAllow: /private\n",
    ] {
        let mut t = transport(MENTION);
        t.route("https://source.example/robots.txt", 200, robots);
        t.route("https://source.example/private", 200, MENTION);
        let r = run_detection(&config(), &[cand("https://source.example/private")], "fixture", "fixture", &t).unwrap();
        assert!(r.opportunities.is_empty(), "{robots}");
        assert_eq!(r.candidates[0].robots.status, "disallowed");
    }
}
const GROUP: &str = "<main><h2>Tarot resources</h2><ul><li><a href='https://dead.example/a'>Tarot A</a></li><li><a href='https://dead.example/b'>B</a></li><li><a href='https://dead.example/c'>C</a></li></ul></main>";

#[test]
fn b01_production_pin_key_and_streaming_regression() {
    let source = include_str!("../src/transport.rs");
    assert!(
        !source.contains("b.resolve_to_addrs(a,"),
        "production override currently uses host:port rather than DNS name"
    );
    assert!(
        !source.contains("resp.read_to_end("),
        "production allocates uncapped decompressed response"
    );
}
#[test]
fn b01_fake_oversized_redirect_body_is_rejected() {
    let mut t = transport("");
    t.route("https://source.example/p", 302, &"x".repeat(200));
    t.routes
        .get_mut("https://source.example/p")
        .unwrap()
        .location = Some("/final".into());
    let limits = backlink_opportunities_detector::report::Limits {
        max_body_bytes: 100,
        ..Default::default()
    };
    let err = fetch::fetch_with_redirects(
        &safe_parse("https://source.example/p").unwrap(),
        &limits,
        &t,
        "fixture",
    )
    .unwrap_err();
    assert!(matches!(err, fetch::FetchError::BodyTooLarge), "{err}");
}
#[test]
fn b02_same_origin_multihop_redirect_honors_robots() {
    let mut t = transport(MENTION);
    t.route(
        "https://source.example/robots.txt",
        200,
        "User-agent: *\nDisallow: /secret\n",
    );
    t.route_redirect("https://source.example/p", 302, "/middle");
    t.route_redirect("https://source.example/middle", 302, "/secret");
    t.route("https://source.example/secret", 200, MENTION);
    let r = run(&config(), &t);
    assert!(r.opportunities.is_empty());
    assert!(r.candidates[0].notes.contains("robots disallowed"));
    assert_eq!(r.candidates[0].robots.status, "disallowed");
    assert!(!t
        .dialed
        .lock()
        .unwrap()
        .iter()
        .any(|d| d.ends_with("/secret")));
}
#[test]
fn b02_final_owned_platform_and_excluded_sources_rejected() {
    for (host, reason) in [
        ("owned.example", "owned"),
        ("reddit.com", "platform"),
        ("dead.example", "excluded"),
    ] {
        let mut cfg = config();
        cfg.excluded_domains = vec!["dead.example".into()];
        let mut t = transport(MENTION);
        t.route_redirect("https://source.example/p", 301, "/middle");
        t.route_redirect(
            "https://source.example/middle",
            302,
            &format!("https://{host}/private"),
        );
        t.route(&format!("https://{host}/private"), 200, MENTION);
        let r = run(&cfg, &t);
        assert!(r.opportunities.is_empty(), "redirect to {host}");
        assert!(
            r.candidates[0].notes.contains(reason),
            "{:?}",
            r.candidates[0]
        );
        assert!(matches!(
            r.candidates[0].outcome,
            CandidateOutcome::Rejected
        ));
    }
}
#[test]
fn b02_redirect_owned_disallowed_never_fetches_final_page() {
    let mut t = transport(MENTION);
    t.route_redirect(
        "https://source.example/p",
        302,
        "https://owned.example/private",
    );
    t.route(
        "https://owned.example/robots.txt",
        200,
        "User-agent: *\nDisallow: /\n",
    );
    t.route("https://owned.example/private", 200, MENTION);
    let r = run(&config(), &t);
    assert!(r.opportunities.is_empty());
    assert!(!t
        .dialed
        .lock()
        .unwrap()
        .iter()
        .any(|d| d.ends_with("/private")));
    assert_eq!(r.candidates[0].robots.status, "disallowed");
}
#[test]
fn b03_longest_wildcard_case_and_end_anchor_rules() {
    for (rules, path, allowed) in [
        (vec![(true, "/"), (false, "/private")], "/private", false),
        (vec![(false, "/"), (true, "/public")], "/public/x", true),
        (vec![(false, "/same"), (true, "/same")], "/same", true),
        (vec![(false, "/*?token=")], "/p?token=secret", false),
        (vec![(false, "/Private")], "/private", true),
        (vec![(false, "/*.pdf$")], "/docs/a.pdf", false),
        (vec![(false, "/*.pdf$")], "/docs/a.pdf?x=1", true),
        (vec![(false, "")], "/any", true),
    ] {
        let r = RobotsRules {
            rules: rules.into_iter().map(|(a, p)| (a, p.into())).collect(),
        };
        assert_eq!(r.path_allowed(path), allowed, "{r:?} for {path}");
    }
}
#[test]
fn b03_path_decisions_are_reported_and_groups_combined() {
    for (robots, path, allowed) in [
        (
            "User-agent: *\nAllow: /\nDisallow: /private\n",
            "/private",
            false,
        ),
        ("User-agent: *\nDisallow: /*?token=\n", "/p?token=x", false),
        ("User-agent: *\nDisallow: /Private\n", "/private", true),
        (
            "User-agent: repair-agent\nAllow: /\n\nUser-agent: repair-agent\nDisallow: /private\n",
            "/private",
            false,
        ),
        (
            "User-agent: *\nDisallow: /\n\nUser-agent: repair-agent\nAllow: /\n",
            "/private",
            true,
        ),
    ] {
        let mut t = transport(MENTION);
        t.route("https://source.example/robots.txt", 200, robots);
        t.route(&format!("https://source.example{path}"), 200, MENTION);
        let r = run_detection(
            &config(),
            &[cand(&format!("https://source.example{path}"))],
            "fixture",
            "fixture",
            &t,
        )
        .unwrap();
        assert_eq!(!r.opportunities.is_empty(), allowed, "{robots}");
        assert_eq!(
            r.candidates[0].robots.status,
            if allowed { "allowed" } else { "disallowed" }
        );
    }
}
#[test]
fn b04_owned_link_excludes_all_rules_and_outbound_work() {
    for body in [format!("{GROUP}<footer><a href='https://www.owned.example/'>Owned</a></footer>"),"<main><p>Tarot Brand Name.</p><a href='https://dead.example/a'>tarot</a><a href='https://blog.owned.example/'>Owned</a></main>".into()] {
        let mut t=transport(&body);
        t.route("https://dead.example/a",404,"gone");
        let r=run(&config(),&t);
        assert!(r.opportunities.is_empty());
        assert_eq!(r.counters.outbound_checked,0);
        assert_eq!(t.request_count(),2);
        let src=safe_parse("https://source.example/p").unwrap();
        assert!(detector::page_links_to_owned_domain(&body,&src,&HashSet::from(["owned.example".into()])));
    }
}
#[test]
fn b05_script_only_topic_never_matches_broken_link() {
    let body="<main><p>Cooking recipes.<script>var topic='tarot';</script></p><a href='https://dead.example/a'>Privacy policy</a></main>";
    let mut t = transport(body);
    t.route("https://dead.example/a", 404, "gone");
    let r = run(&config(), &t);
    assert!(r.opportunities.is_empty());
}
#[test]
fn b05_broken_match_records_actual_visible_sentence() {
    let mut t=transport("<main><p>Our tarot guide is useful.</p><a href='https://dead.example/a'>Old guide</a></main>");
    t.route("https://dead.example/a", 410, "gone");
    let r = run(&config(), &t);
    let opp = r
        .opportunities
        .iter()
        .find(|o| o.rule == "broken-link-replacement-possibility")
        .unwrap();
    assert!(
        opp.observed_facts
            .iter()
            .any(|s| s.contains("Our tarot guide is useful.")),
        "{opp:?}"
    );
}
#[test]
fn b06_inline_brand_preserves_verbatim_continuity() {
    let html = "<main><p>Brand <em>Name</em> tarot.</p><p>Br<em>and</em> Name again.</p></main>";
    assert_eq!(
        text::sentence_containing(html, "Brand Name").as_deref(),
        Some("Brand Name tarot.")
    );
    assert!(text::full_visible_text(html).contains("brand name again."));
    assert_eq!(run(&config(), &transport(html)).opportunities.len(), 1);
}
#[test]
fn b06_nested_script_and_hidden_descendants_excluded() {
    for html in [
        "<main><p>Cooking <script>Brand Name</script>.</p></main>",
        "<main><p hidden>Brand Name</p></main>",
        "<main><section aria-hidden='true'><p>Brand Name</p></section></main>",
        "<main><p>Cooking<span style='display:none'>Brand Name</span>.</p></main>",
        "<main><p style='visibility: hidden'>Brand Name</p></main>",
    ] {
        assert!(
            text::sentence_containing(html, "Brand Name").is_none(),
            "{html}"
        );
        assert!(run(&config(), &transport(html)).opportunities.is_empty());
    }
}
#[test]
fn b06_bare_main_and_mixed_blocks_are_retained() {
    let html = "<main>Brand <em>Name</em> tarot.<p>Other text.</p>More visible words.</main>";
    assert_eq!(
        text::sentence_containing(html, "Brand Name").as_deref(),
        Some("Brand Name tarot.")
    );
    assert!(text::full_visible_text(html).contains("more visible words."));
    assert_eq!(run(&config(), &transport(html)).opportunities.len(), 1);
}
#[test]
fn b07_reopened_captures_are_immutable_and_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let mut references = Vec::new();
    for body in [
        b"first".as_slice(),
        b"second".as_slice(),
        b"second".as_slice(),
    ] {
        let mut ev = Evidence::open(dir.path().to_str()).unwrap().unwrap();
        let result = ev.save("page", "https://source.example/p", body, "html");
        assert!(!format!("{result:?}").starts_with("Err"));
        references.push(ev.files[0].clone());
    }
    assert_eq!(
        references
            .iter()
            .map(|r| &r.0)
            .collect::<HashSet<_>>()
            .len(),
        3
    );
    for (name, _, hash) in references {
        assert_eq!(
            detector::sha256_hex(&std::fs::read(dir.path().join(name)).unwrap()),
            hash
        );
    }
}
#[test]
fn b07_save_failure_is_loud() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("captures");
    let mut ev = Evidence::open(path.to_str()).unwrap().unwrap();
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, b"not a directory").unwrap();
    let result = ev.save("page", "https://source.example/p", b"body", "html");
    assert!(
        format!("{result:?}").starts_with("Err"),
        "save failure silently returned {result:?}"
    );
}
#[test]
fn b07_evidence_refuses_project_paths_before_creating_them() {
    let cwd = std::env::current_dir().unwrap();
    assert!(Evidence::open(cwd.to_str()).is_err());
    assert!(Evidence::open(cwd.join("src").to_str()).is_err());
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("site");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    assert!(Evidence::open(repo.join("captures").to_str()).is_err());
    assert!(!repo.join("captures").exists());
}
#[test]
fn b07_new_directory_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("captures");
    Evidence::open(path.to_str()).unwrap();
    assert_eq!(
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o700
    );
}
#[test]
fn b07_cli_never_overwrites_user_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.json");
    std::fs::write(&path, b"keep me").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_backlink-opportunities-detector"))
        .args([
            "generate-queries",
            "--config",
            "examples/gissecrets-config.json",
            "--output",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read(path).unwrap(), b"keep me");
}
#[test]
fn b08_outbound_cap_is_partial_with_warning() {
    let mut cfg = config();
    cfg.limits.outbound_checks = 1;
    let t = transport(
        "<p>Tarot.</p><a href='https://dead.example/a'>A</a><a href='https://dead.example/b'>B</a>",
    );
    let r = run(&cfg, &t);
    assert!(r.counters.outbound_limit_reached);
    assert!(r.partial);
    assert!(!r.warnings.is_empty());
}
#[test]
fn b08_per_page_cap_is_partial_and_exact_limit_is_not() {
    let mut cfg = config();
    cfg.limits.outbound_links_per_page = 1;
    for (body,partial) in [("<p>Cooking</p><a href='https://dead.example/a'>A</a><a href='https://dead.example/b'>B</a>",true),("<p>Cooking</p><a href='https://dead.example/a'>A</a>",false)] {
        let r=run(&cfg,&transport(body));
        assert_eq!(r.partial,partial); assert_eq!(!r.warnings.is_empty(),partial);
    }
}
#[test]
fn b08_unavailable_source_is_partial_and_failed_counted() {
    let t = FakeTransport::new(&[]);
    let r = run(&config(), &t);
    assert!(r.partial);
    assert!(!r.warnings.is_empty());
    assert_eq!(r.counters.candidates_failed, 1);
    assert_eq!(t.request_count(), 0);
}
#[test]
fn b08_failed_and_malformed_provider_are_not_empty_success() {
    for input in [
        r#"[{"query":"tarot","response":{"success":false,"error":"unavailable"}}]"#,
        r#"[{"query":"tarot","response":{"success":true,"data":{}}}]"#,
        r#"[{"query":"tarot","response":{"data":{"web":[{"title":"no URL"}]}}}]"#,
    ] {
        assert!(parse_candidates(input).is_err(), "{input}");
    }
    assert!(parse_candidates(
        r#"[{"query":"tarot","response":{"success":true,"data":{"web":[]}}}]"#
    )
    .unwrap()
    .0
    .is_empty());
}
#[test]
fn b08_private_input_has_rejection_record_and_count() {
    let t = FakeTransport::new(&[]);
    let r = run_detection(
        &config(),
        &[cand("http://127.0.0.1/private")],
        "fixture",
        "fixture",
        &t,
    )
    .unwrap();
    assert_eq!(r.candidates.len(), 1);
    assert_eq!(r.counters.candidates_rejected, 1);
    assert!(matches!(
        r.candidates[0].outcome,
        CandidateOutcome::Rejected
    ));
    assert_eq!(t.request_count(), 0);
}
#[test]
fn b09_import_and_outbound_preserve_query_identity() {
    let (_, s) = parse_candidates(
        r#"[{"url":"https://source.example/p?q=a/"},{"url":"https://source.example/p?q=a"}]"#,
    )
    .unwrap();
    assert_eq!(s.unique, 2);
    let links=detector::extract_outbound_links("<a href='https://dead.example/p?q=alive'>A</a><a href='https://dead.example/p?q=dead'>D</a>",&safe_parse("https://source.example/p").unwrap(),&HashSet::new(),10);
    assert_eq!(links.len(), 2);
}
#[test]
fn b10_exclusions_share_canonical_www_case_idna_identity() {
    for (excluded, source) in [
        ("www.source.example", "source.example"),
        ("WWW.SOURCE.example", "source.example"),
        ("www.BÜCHER.example", "xn--bcher-kva.example"),
    ] {
        let mut cfg = config();
        cfg.excluded_domains = vec![excluded.into()];
        competitors::validate(&cfg).unwrap();
        let mut t = transport(MENTION);
        t.dns
            .insert(source.into(), vec!["93.184.216.34".parse().unwrap()]);
        t.route(&format!("https://{source}/robots.txt"), 404, "");
        t.route(&format!("https://{source}/p"), 200, MENTION);
        let r = run_detection(
            &cfg,
            &[cand(&format!("https://{source}/p"))],
            "fixture",
            "fixture",
            &t,
        )
        .unwrap();
        assert!(r.opportunities.is_empty(), "{excluded}");
        assert_eq!(t.request_count(), 0);
    }
}
#[test]
fn b11_footer_links_and_unrelated_heading_sections_are_not_groups() {
    for html in ["<main><h2>Tarot resources</h2><p>Reading tarot.</p></main><footer><a href='https://dead.example/a'>A</a><a href='https://dead.example/b'>B</a><a href='https://dead.example/c'>C</a></footer>","<main><h2>Tarot resources</h2><p>Reading tarot.</p><h2>Social</h2><a href='https://dead.example/a'>A</a><a href='https://dead.example/b'>B</a><a href='https://dead.example/c'>C</a></main>","<main><h2>Tarot outsourcing</h2><a href='https://dead.example/a'>A</a><a href='https://dead.example/b'>B</a><a href='https://dead.example/c'>C</a></main>"] {
        let r=run(&config(),&transport(html));
        assert!(r.opportunities.iter().all(|o|o.rule!="curated-resource-page-lead"),"{html}");
    }
    assert!(run(&config(), &transport(GROUP))
        .opportunities
        .iter()
        .any(|o| o.rule == "curated-resource-page-lead"));
}
#[test]
fn b12_documented_competitor_journey_parses_without_network() {
    let readme = include_str!("../README.md");
    for required in [
        "competitor-backlink-gap-lead",
        "--backlinks",
        "competitor_domains",
        "source_url",
        "competitor_url",
        "provenance",
    ] {
        assert!(readme.contains(required), "missing {required}");
    }
    let cfg: DetectorConfig =
        serde_json::from_str(&std::fs::read_to_string("examples/competitor-config.json").unwrap())
            .unwrap();
    assert!(!cfg.network.enabled);
    assert_eq!(competitors::validate(&cfg).unwrap().len(), 2);
    assert_eq!(
        competitors::parse_backlinks(
            &std::fs::read_to_string("examples/competitor-backlinks.json").unwrap(),
            &cfg
        )
        .unwrap()
        .len(),
        1
    );
    let out = Command::new(env!("CARGO_BIN_EXE_backlink-opportunities-detector"))
        .args([
            "detect",
            "--config",
            "examples/competitor-config.json",
            "--backlinks",
            "examples/competitor-backlinks.json",
            "--output",
            "unused.json",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("network.enabled is false"));
}
#[test]
fn b13_unknown_duplicate_missing_flags_exit_two() {
    for (args, needle) in [
        (
            vec![
                "generate-queries",
                "--config",
                "examples/gissecrets-config.json",
                "--typo",
                "true",
            ],
            "unknown",
        ),
        (
            vec!["generate-queries", "--config", "x", "--config", "y"],
            "duplicate",
        ),
        (vec!["detect"], "--config"),
        (vec!["detect", "--config", "x"], "--output"),
        (
            vec!["detect", "--config", "x", "--output", "y"],
            "exactly one",
        ),
        (vec!["wat"], "unknown subcommand"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_backlink-opportunities-detector"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stderr).contains(needle));
    }
}
#[test]
fn b13_documented_query_workflow_and_help_still_parse() {
    let out = Command::new(env!("CARGO_BIN_EXE_backlink-opportunities-detector"))
        .args([
            "generate-queries",
            "--config",
            "examples/gissecrets-config.json",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["queries"].is_array()
    );
    let out = Command::new(env!("CARGO_BIN_EXE_backlink-opportunities-detector"))
        .arg("--help")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--backlinks"));
}

#[test]
fn b03_gated_wildcard_no_socket_for_disallowed_path() {
    let mut t = transport(MENTION);
    t.route(
        "https://source.example/robots.txt",
        200,
        "User-agent: *\nDisallow: /*?token=\n",
    );
    let err = fetch::gated_get(
        "https://source.example/p?token=x",
        "repair-agent",
        &config().limits,
        &t,
        &mut HashMap::new(),
    )
    .unwrap_err();
    assert!(matches!(err, fetch::FetchError::RobotsDisallowed(_)));
    assert_eq!(t.request_count(), 1);
}
