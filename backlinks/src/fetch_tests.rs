//! Gated-fetch engine tests using the injected FakeTransport.
//! Production code has no localhost path; these tests never touch sockets.

use std::collections::HashMap;
use std::net::IpAddr;

use crate::fetch::{fetch_with_redirects, gated_get, vetted_ips, FetchError};
use crate::neturl::safe_parse;
use crate::report::Limits;
use crate::transport::fake::FakeTransport;

fn limits() -> Limits {
    Limits::default()
}

fn https_key(host_path: &str) -> String {
    format!("https://{host_path}")
}

#[test]
fn robots_disallow_blocks_only_disallowed_path() {
    let mut t = FakeTransport::new(&["src.example"]);
    t.route(
        &https_key("src.example/robots.txt"),
        200,
        "User-agent: *\nDisallow: /secret\n",
    );
    t.route(&https_key("src.example/secret"), 200, "<html>page</html>");
    t.route(&https_key("src.example/open"), 200, "<html>open</html>");
    let mut cache = HashMap::new();
    let err = gated_get(
        "https://src.example/secret",
        "agent",
        &limits(),
        &t,
        &mut cache,
    )
    .unwrap_err();
    assert!(
        matches!(err, FetchError::RobotsDisallowed(_)),
        "expected disallow, got {err}"
    );
    let ok = gated_get(
        "https://src.example/open",
        "agent",
        &limits(),
        &t,
        &mut cache,
    )
    .unwrap();
    assert_eq!(ok.status, 200);
    assert_eq!(
        ok.robots_note.as_deref(),
        Some("robots.txt consulted; path allowed")
    );
}

#[test]
fn robots_unavailable_on_401_403_429_5xx() {
    for status in [401u16, 403, 429, 500, 503] {
        let mut t = FakeTransport::new(&["blocked.example"]);
        t.route(&https_key("blocked.example/robots.txt"), status, "denied");
        let mut cache = HashMap::new();
        let err = gated_get(
            "https://blocked.example/page",
            "agent",
            &limits(),
            &t,
            &mut cache,
        )
        .unwrap_err();
        assert!(
            matches!(err, FetchError::RobotsUnavailable(_)),
            "status {status}: expected unavailable, got {err}"
        );
    }
}

#[test]
fn robots_404_means_no_restrictions() {
    let mut t = FakeTransport::new(&["norf.example"]);
    t.route(&https_key("norf.example/robots.txt"), 404, "none");
    let mut cache = HashMap::new();
    let ok = gated_get(
        "https://norf.example/page",
        "agent",
        &limits(),
        &t,
        &mut cache,
    )
    .unwrap();
    assert_eq!(ok.status, 200);
}

#[test]
fn robots_dns_failure_is_unavailable_not_allowed() {
    let t = FakeTransport::new(&[]); // nothing resolves
    let mut cache = HashMap::new();
    let err = gated_get(
        "https://ghost.example/page",
        "agent",
        &limits(),
        &t,
        &mut cache,
    )
    .unwrap_err();
    assert!(
        matches!(err, FetchError::RobotsUnavailable(_)),
        "expected robots-unavailable on DNS failure, got {err}"
    );
}

#[test]
fn robots_redirect_to_private_is_unavailable() {
    let mut t = FakeTransport::new(&["rot.example"]);
    t.route_redirect(
        &https_key("rot.example/robots.txt"),
        302,
        "http://169.254.169.254/latest/meta-data/",
    );
    let mut cache = HashMap::new();
    let err = gated_get(
        "https://rot.example/page",
        "agent",
        &limits(),
        &t,
        &mut cache,
    )
    .unwrap_err();
    assert!(
        matches!(err, FetchError::RobotsUnavailable(_)),
        "unsafe robots redirect must be unavailable, got {err}"
    );
}

#[test]
fn redirect_chain_followed() {
    let mut t = FakeTransport::new(&["a.example", "b.example"]);
    t.route_redirect(
        &https_key("a.example/start"),
        301,
        "https://b.example/final",
    );
    t.route(&https_key("b.example/final"), 200, "arrived");
    let url = safe_parse("https://a.example/start").unwrap();
    let out = fetch_with_redirects(&url, &limits(), &t, "agent").unwrap();
    assert_eq!(out.status, 200);
    assert_eq!(out.final_url, "https://b.example/final");
    assert_eq!(out.redirect_chain, vec!["https://b.example/final"]);
}

#[test]
fn redirect_to_private_rejected() {
    let mut t = FakeTransport::new(&["a.example"]);
    t.route_redirect(
        &https_key("a.example/evil"),
        302,
        "http://169.254.169.254/latest/meta-data/",
    );
    let url = safe_parse("https://a.example/evil").unwrap();
    let err = fetch_with_redirects(&url, &limits(), &t, "agent").unwrap_err();
    assert!(matches!(err, FetchError::UnsafeRedirect(_)), "got {err}");
}

#[test]
fn https_downgrade_rejected() {
    let mut t = FakeTransport::new(&["a.example"]);
    t.route_redirect(&https_key("a.example/down"), 301, "http://a.example/plain");
    let url = safe_parse("https://a.example/down").unwrap();
    let err = fetch_with_redirects(&url, &limits(), &t, "agent").unwrap_err();
    assert!(matches!(err, FetchError::UnsafeRedirect(_)));
}

#[test]
fn redirect_loop_detected() {
    let mut t = FakeTransport::new(&["loop.example"]);
    t.route_redirect(&https_key("loop.example/1"), 302, "https://loop.example/2");
    t.route_redirect(&https_key("loop.example/2"), 302, "https://loop.example/1");
    let url = safe_parse("https://loop.example/1").unwrap();
    let err = fetch_with_redirects(&url, &limits(), &t, "agent").unwrap_err();
    assert!(matches!(err, FetchError::RedirectLoop(_)));
}

#[test]
fn redirect_target_without_public_dns_is_unreachable() {
    let mut t = FakeTransport::new(&["a.example"]); // b.example: no DNS
    t.route_redirect(&https_key("a.example/r"), 302, "https://b.example/x");
    let url = safe_parse("https://a.example/r").unwrap();
    let err = fetch_with_redirects(&url, &limits(), &t, "agent").unwrap_err();
    assert!(matches!(err, FetchError::Unreachable(_)));
}

#[test]
fn mixed_dns_answers_filtered_to_public() {
    let mut t = FakeTransport::new(&[]);
    t.dns.insert(
        "dual.example".to_string(),
        vec![
            "10.0.0.5".parse::<IpAddr>().unwrap(), // private: dropped
            "93.184.216.34".parse().unwrap(),      // public: kept
        ],
    );
    let url = safe_parse("https://dual.example/x").unwrap();
    let ips = vetted_ips(&url, &t);
    assert_eq!(ips, vec!["93.184.216.34".parse::<IpAddr>().unwrap()]);
}

#[test]
fn all_private_dns_answers_means_unreachable() {
    let mut t = FakeTransport::new(&[]);
    t.dns.insert(
        "dark.example".to_string(),
        vec!["127.0.0.1".parse().unwrap(), "192.168.0.9".parse().unwrap()],
    );
    let url = safe_parse("https://dark.example/x").unwrap();
    let err = fetch_with_redirects(&url, &limits(), &t, "agent").unwrap_err();
    assert!(matches!(err, FetchError::Unreachable(_)));
}

#[test]
fn oversized_body_rejected() {
    let mut t = FakeTransport::new(&["big.example"]);
    t.route(&https_key("big.example/blob"), 200, &"x".repeat(1000));
    let url = safe_parse("https://big.example/blob").unwrap();
    let tiny = Limits {
        max_body_bytes: 100,
        ..Default::default()
    };
    let err = fetch_with_redirects(&url, &tiny, &t, "agent").unwrap_err();
    assert!(matches!(err, FetchError::BodyTooLarge));
}

#[test]
fn unsafe_target_never_reaches_transport() {
    let t = FakeTransport::new(&[]);
    let mut cache = HashMap::new();
    let err = gated_get(
        "http://127.0.0.1:8080/x",
        "agent",
        &limits(),
        &t,
        &mut cache,
    )
    .unwrap_err();
    assert!(matches!(err, FetchError::UnsafeTarget(_)), "got {err}");
    assert_eq!(t.request_count(), 0, "transport must not be dialed");
}

#[test]
fn user_agent_propagates_to_transport() {
    use crate::transport::{HttpTransport, RawResponse};
    struct UaCapture {
        sent: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }
    impl HttpTransport for UaCapture {
        fn resolve(&self, _h: &str) -> std::io::Result<Vec<IpAddr>> {
            Ok(vec!["93.184.216.34".parse().unwrap()])
        }
        fn get(
            &self,
            _s: &str,
            _h: &str,
            _p: u16,
            _ip: IpAddr,
            _pq: &str,
            ua: &str,
        ) -> std::io::Result<RawResponse> {
            self.sent.lock().unwrap().push(ua.to_string());
            Ok(RawResponse {
                status: 200,
                body: b"ok".to_vec(),
                location: None,
                content_type: None,
            })
        }
    }
    let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let t = UaCapture { sent: sent.clone() };
    let url = safe_parse("https://ua.example/x").unwrap();
    let out = fetch_with_redirects(&url, &limits(), &t, "BacklinkDetector/1.0").unwrap();
    assert_eq!(out.status, 200);
    assert_eq!(*sent.lock().unwrap(), vec!["BacklinkDetector/1.0"]);
}
