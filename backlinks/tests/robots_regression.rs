use backlink_opportunities_detector::fetch::RobotsRules;

#[test]
fn whitespace_groups_and_repeated_agents_keep_all_rules() {
    let rules = RobotsRules::parse("User-agent: *\n\nDisallow: /fallback\nUser-agent: repair-agent\nDisallow: /one\nUser-agent: repair-agent\nDisallow: /two\n", "repair-agent");
    assert!(!rules.path_allowed("/one"));
    assert!(!rules.path_allowed("/two"));
    assert!(rules.path_allowed("/fallback"));
    let rules = RobotsRules::parse("User-agent: *\nUser-agent: repair-agent\nDisallow: /private\nUser-agent: rep\nAllow: /private\n", "repair-agent");
    assert!(!rules.path_allowed("/private"));
}

#[test]
fn wildcard_end_anchor_and_prefix_identity() {
    let rules = RobotsRules::parse("User-agent: *\nDisallow: /exact$\nDisallow: /a*b*c$\nDisallow: /Private\nDisallow: /x\nAllow: /x/open\n", "agent");
    for path in ["/exact", "/abc", "/a123b456c", "/Private", "/x/closed"] {
        assert!(!rules.path_allowed(path), "{path}");
    }
    for path in [
        "/other/exact",
        "/exact/more",
        "/a123b456c/more",
        "/private",
        "/x/open",
        "/outside",
    ] {
        assert!(rules.path_allowed(path), "{path}");
    }
}

#[test]
fn wildcard_matches_do_not_recurse_exponentially() {
    let pattern = format!("User-agent: *\nDisallow: /{}z$\n", "*a".repeat(50));
    let rules = RobotsRules::parse(&pattern, "agent");
    assert!(rules.path_allowed(&format!("/{}q", "a".repeat(500))));
}
