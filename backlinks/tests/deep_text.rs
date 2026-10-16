use backlink_opportunities_detector::text::visible_regions;
use std::process::Command;

#[test]
fn deeply_nested_remote_html_does_not_abort_the_audit() {
    if std::env::var_os("BACKLINK_DEEP_DOM_CHILD").is_some() {
        let html = format!(
            "<main>{}Brand Name.{}<script>Hidden Brand.</script></main>",
            "<span>".repeat(12000),
            "</span>".repeat(12000)
        );
        let regions = visible_regions(&html);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].text, "Brand Name.");
        let anchor_html = format!("<a href='https://outside.example/'>{html}</a>");
        let source =
            backlink_opportunities_detector::neturl::safe_parse("https://source.example/").unwrap();
        let links = backlink_opportunities_detector::detector::extract_outbound_links(
            &anchor_html,
            &source,
            &std::collections::HashSet::new(),
            10,
        );
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].anchor_text, "Brand Name.");
        return;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "deeply_nested_remote_html_does_not_abort_the_audit",
            "--nocapture",
        ])
        .env("BACKLINK_DEEP_DOM_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "deep DOM child failed: {} {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}
