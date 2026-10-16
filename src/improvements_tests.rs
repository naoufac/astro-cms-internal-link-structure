use crate::cli::run_request;
use serde_json::{json, Value};

#[test]
fn offline_editorial_metadata_and_contextual_regression() {
    let root = std::env::temp_dir().join(format!("ails-improvements-{}", std::process::id()));
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::create_dir_all(root.join("private")).unwrap();
    std::fs::write(root.join("index.html"), r#"<title>Home</title><main><p>Unique Target Guide is relevant.</p><nav><main><a href='/target/'>menu</a></main></nav><div id='_mig-drawer' role='dialog' aria-label='Site menu'><a href='/target/'>drawer</a></div><div hidden><a href='/target/'>hidden</a></div><a href='/private/'><span hidden>wrong</span><img alt='Private guide'></a></main>"#).unwrap();
    std::fs::write(root.join("target/index.html"), "<title>Unique Target Guide</title><link rel='canonical' href='/target/'><main><a href='/'>click here</a></main>").unwrap();
    std::fs::write(
        root.join("private/index.html"),
        "<title>Private Guide</title><meta name='googlebot' content='noindex'>",
    )
    .unwrap();
    std::fs::write(root.join("sitemap.xml"), "<urlset xmlns='http://www.sitemaps.org/schemas/sitemap/0.9'><url><loc>https://e.com/private/</loc></url><url><loc>https://e.com/missing/</loc></url></urlset>").unwrap();
    let report = run_request(&json!({"schemaVersion":"1","artifactRoot":root,"site":"https://e.com","entrypoints":["/"]}).to_string()).unwrap();
    let r: Value = serde_json::to_value(report).unwrap();
    assert_eq!(r["edges"][0]["editorial"], false);
    assert_eq!(r["edges"][3]["anchor"], "Private guide");
    assert_eq!(r["nodes"][2]["metadata"]["canonicalStatus"], "self");
    assert_eq!(r["nodes"][1]["metadata"]["eligible"], false);
    assert!(r["nodes"][2]["editorialUnreachable"].as_bool().unwrap());
    assert!(r["opportunities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["kind"] == "contextual" && o["target"] == "/target/"));
    assert!(r["sitemap"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["kind"] == "noindex"));
    assert!(r["sitemap"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["kind"] == "missingArtifact"));
    assert!(r["anchorFindings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["kind"] == "vague"));
    std::fs::remove_dir_all(root).unwrap();
}

fn run_fixture(name: &str, files: &[(&str, &str)]) -> Value {
    let root = std::env::temp_dir().join(format!("ails-improvement-{name}-{}", std::process::id()));
    for (file, html) in files {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, html).unwrap();
    }
    let r = run_request(&json!({"schemaVersion":"1","artifactRoot":root,"site":"https://e.com","entrypoints":["/"]}).to_string()).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    serde_json::to_value(r).unwrap()
}

#[test]
fn sitemap_namespaced_index_local_children_escaping_and_invalid_entries() {
    let r = run_fixture("xml", &[
        ("index.html", "<title>Home</title><link rel='canonical' href='/'>"),
        ("private/index.html", "<meta name='robots' content='noindex'>"),
        ("other/index.html", "<link rel='canonical' href='https://outside.example/'>"),
        ("sitemap.xml", "<s:sitemapindex xmlns:s='http://www.sitemaps.org/schemas/sitemap/0.9'><s:sitemap><s:loc>https://e.com/child%20map.xml</s:loc></s:sitemap><s:sitemap><s:loc>https://e.com/../outside.xml</s:loc></s:sitemap><s:sitemap><s:loc>https://outside.example/sitemap.xml</s:loc></s:sitemap></s:sitemapindex>"),
        ("child map.xml", "<s:urlset xmlns:s='http://www.sitemaps.org/schemas/sitemap/0.9'><s:url><s:loc>https://e.com/</s:loc></s:url><s:url><s:loc>https://e.com/</s:loc></s:url><s:url><s:loc>https://e.com/private/</s:loc></s:url><s:url><s:loc>https://e.com/other/</s:loc></s:url><s:url><s:loc>https://e.com/missing/?a=1&amp;b=2</s:loc></s:url><s:url><s:loc>/relative/</s:loc></s:url><s:url/></s:urlset>"),
    ]);
    let issues = r["sitemap"]["issues"].as_array().unwrap();
    for kind in [
        "duplicateUrl",
        "duplicatePage",
        "noindex",
        "noncanonical",
        "missingArtifact",
        "malformedUrl",
        "malformed",
        "unavailableChild",
    ] {
        assert!(
            issues.iter().any(|i| i["kind"] == kind),
            "{kind}: {issues:?}"
        );
    }
    assert!(issues
        .iter()
        .any(|i| i["url"] == "https://e.com/missing/?a=1&b=2"));
    assert_eq!(r["sitemap"]["files"].as_array().unwrap().len(), 2);
    assert_eq!(r["sitemap"]["urls"], 5);
}

#[test]
fn malformed_sitemap_empty_and_missing_canonical_are_honest() {
    let r = run_fixture(
        "bad-xml",
        &[
            ("index.html", "<title>Home</title>"),
            ("sitemap.xml", "<urlset><url></urlset>"),
        ],
    );
    assert_eq!(r["sitemap"]["issues"][0]["kind"], "malformed");
    assert_eq!(r["nodes"][0]["metadata"]["eligible"], true);
    let r = run_fixture(
        "empty-xml",
        &[("index.html", ""), ("sitemap.xml", "<urlset/>")],
    );
    assert_eq!(r["sitemap"]["urls"], 0);
    assert!(r["sitemap"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["kind"] == "notListed"));
}

#[test]
fn role_menu_menubar_and_drawer_class_links_are_not_editorial() {
    let p = crate::parser::extract(
        "<main><div role='menu'><a href='/a'>Menu item</a></div><div role='menubar'><a href='/b'>Menu bar</a></div><div class='menu-wrapper _mig-drawer'><a href='/c'>Drawer</a></div></main>",
    );
    assert_eq!(p.links.len(), 3);
    assert!(
        p.links.iter().all(|l| !l.editorial),
        "{:?}",
        p.links
            .iter()
            .map(|l| (l.href.as_str(), l.editorial))
            .collect::<Vec<_>>()
    );
}

#[test]
fn sitemap_canonical_alias_contradiction_is_reported() {
    let r = run_fixture(
        "canonical-alias",
        &[
            (
                "index.html",
                "<title>Home</title><link rel='canonical' href='/'>",
            ),
            (
                "target/index.html",
                "<title>Target topic</title><link rel='canonical' href='/target/'>",
            ),
            (
                "sitemap.xml",
                "<urlset xmlns='http://www.sitemaps.org/schemas/sitemap/0.9'><url><loc>https://e.com/</loc></url><url><loc>https://e.com/target</loc></url></urlset>",
            ),
        ],
    );
    assert!(
        r["sitemap"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["url"] == "https://e.com/target"
                && matches!(
                    i["kind"].as_str(),
                    Some("canonicalMismatch") | Some("noncanonical")
                )),
        "{:?}",
        r["sitemap"]["issues"]
    );
}

#[test]
fn malformed_sitemap_reports_uncertainty_without_missing_listings() {
    let r = run_fixture(
        "malformed-uncertain",
        &[
            ("index.html", "<title>Home</title>"),
            (
                "sitemap.xml",
                "<urlset><url><loc>https://e.com/</loc></url>",
            ),
        ],
    );
    let issues = r["sitemap"]["issues"].as_array().unwrap();
    assert!(
        issues.iter().any(|i| i["kind"] == "malformed"),
        "{issues:?}"
    );
    assert!(
        !issues.iter().any(|i| i["kind"] == "notListed"),
        "{issues:?}"
    );
}

#[test]
fn contextual_excludes_ambiguous_ineligible_self_and_existing_nofollow() {
    let r = run_fixture("ctx-exclusions", &[
        ("index.html", "<title>Source Guide</title><main><p>Source Guide. Duplicate Guide. Private Guide. Foreign Guide. Linked Guide. Unique Guidebook.</p><a href='/linked/' rel='nofollow'>Linked</a></main>"),
        ("one/index.html", "<title>Duplicate Guide</title>"),
        ("two/index.html", "<title>Duplicate Guide</title>"),
        ("private/index.html", "<title>Private Guide</title><meta name='robots' content='noindex'>"),
        ("foreign/index.html", "<title>Foreign Guide</title><link rel='canonical' href='https://else.example/'>"),
        ("linked/index.html", "<title>Linked Guide</title>"),
        ("unique/index.html", "<title>Unique Guide</title>"),
    ]);
    assert!(r["opportunities"].as_array().unwrap().is_empty());
}

#[test]
fn contextual_results_not_silently_capped() {
    let root = std::env::temp_dir().join(format!("ails-context-cap-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut text = String::new();
    for i in 0..205 {
        std::fs::create_dir_all(root.join(format!("p{i}"))).unwrap();
        std::fs::write(
            root.join(format!("p{i}/index.html")),
            format!("<title>Unique Guide {i}</title>"),
        )
        .unwrap();
        text.push_str(&format!("<p>Unique Guide {i}.</p>"));
    }
    std::fs::write(root.join("index.html"), text).unwrap();
    let r = run_request(&json!({"schemaVersion":"1","artifactRoot":root,"site":"https://e.com","entrypoints":["/"]}).to_string()).unwrap();
    assert_eq!(
        r.opportunities
            .iter()
            .filter(|o| o.kind == "contextual")
            .count(),
        205
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn editorial_all_ancestors_and_anchor_text_evidence() {
    let p = crate::parser::extract("<main><header><main><a href='/a'>header</a></main></header><aside><a href='/b'>aside</a></aside><div style='display: none !important'><a href='/c'>hidden</a></div><a href='/d'>Visible<span aria-hidden='true'>wrong</span> words<img alt='diagram'></a><a href='/e' title='Fallback'></a><map><area href='/f' alt='Map destination'></map><div role='dialog' aria-label='Site menu'><a href='/g'>drawer</a></div></main>");
    assert!(p.links[..3].iter().all(|l| !l.editorial));
    assert_eq!(p.links[2].anchor, "");
    assert_eq!(p.links[3].anchor, "Visible words diagram");
    assert_eq!(p.links[4].anchor, "Fallback");
    assert_eq!(p.links[5].anchor, "Map destination");
    assert!(!p.links[6].editorial);
}

#[test]
fn contextual_never_bridges_removed_visible_link_spans() {
    let r = run_fixture("removed-links", &[
        ("index.html", "<main><p>Split <a href='/other/'>unrelated</a> Phrase</p><p>Hidden <span style='display:none'>noise</span> Phrase</p><p>Inline <em>Phrase</em></p></main>"),
        ("other/index.html", "<title>Other</title>"),
        ("split/index.html", "<title>Split Phrase</title>"),
        ("hidden/index.html", "<title>Hidden Phrase</title>"),
        ("inline/index.html", "<title>Inline Phrase</title>"),
    ]);
    let opportunities = r["opportunities"].as_array().unwrap();
    assert!(
        !opportunities.iter().any(|o| o["target"] == "/split/"),
        "invented phrase across existing visible link: {opportunities:?}"
    );
    for target in ["/hidden/", "/inline/"] {
        assert!(opportunities.iter().any(|o| o["target"] == target));
    }
}

#[test]
fn aria_labelledby_resolves_first_reference_and_first_document_id() {
    let r = run_fixture("labelledby", &[
        ("index.html", "<main><a href='/target/' aria-labelledby='label'><svg></svg></a><span id='label'>Descriptive destination</span><span id='label'>Wrong duplicate</span><span id='second'>Wrong second reference</span><a href='/target/' aria-labelledby='label second'></a><a href='/target/' aria-labelledby='missing second'></a><a href='/target/' aria-labelledby='missing'></a></main>"),
        ("target/index.html", "<title>Target</title>"),
    ]);
    let edges = r["edges"].as_array().unwrap();
    assert_eq!(edges[0]["anchor"], "Descriptive destination");
    assert_eq!(edges[1]["anchor"], "Descriptive destination");
    assert_eq!(edges[2]["anchor"], "");
    assert_eq!(edges[3]["anchor"], "");
    let findings = r["anchorFindings"].as_array().unwrap();
    assert!(!findings
        .iter()
        .any(|f| f["kind"] == "vague" && f["anchor"] == "Descriptive destination"));
    assert!(findings
        .iter()
        .any(|f| f["kind"] == "vague" && f["anchor"] == ""));
}
