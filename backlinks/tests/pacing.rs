use backlink_opportunities_detector::{
    report::{DetectorConfig, ImportedCandidate},
    runner::run_detection,
    transport::{fake::FakeTransport, HttpTransport, RawResponse},
};
use std::{
    io,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

struct TimedTransport {
    inner: FakeTransport,
    starts: Mutex<Vec<Instant>>,
}
impl HttpTransport for TimedTransport {
    fn resolve(&self, host: &str) -> io::Result<Vec<IpAddr>> {
        self.inner.resolve(host)
    }
    fn get(
        &self,
        scheme: &str,
        host: &str,
        port: u16,
        ip: IpAddr,
        path: &str,
        ua: &str,
    ) -> io::Result<RawResponse> {
        self.starts.lock().unwrap().push(Instant::now());
        self.inner.get(scheme, host, port, ip, path, ua)
    }
}
#[test]
fn robots_pages_and_redirect_hops_share_origin_pacing() {
    let config: DetectorConfig = serde_json::from_value(serde_json::json!({
        "schema_version":1, "site_origin":"https://owned.example", "owned_domains":["owned.example"],
        "brands":["Brand Name"], "target_pages":[],
        "limits":{"per_origin_delay_ms":20}, "network":{"enabled":true,"user_agent":"fixture"}
    })).unwrap();
    let mut inner = FakeTransport::new(&["source.example"]);
    inner.route("https://source.example/robots.txt", 404, "");
    inner.route_redirect("https://source.example/p", 302, "/final");
    inner.route(
        "https://source.example/final",
        200,
        "<main>Brand Name.</main>",
    );
    let transport = TimedTransport {
        inner,
        starts: Mutex::new(Vec::new()),
    };
    let candidate: ImportedCandidate =
        serde_json::from_value(serde_json::json!({"url":"https://source.example/p"})).unwrap();
    run_detection(&config, &[candidate], "fixture", "fixture", &transport).unwrap();
    let starts = transport.starts.lock().unwrap();
    assert_eq!(starts.len(), 3);
    for pair in starts.windows(2) {
        assert!(pair[1].duration_since(pair[0]) >= Duration::from_millis(20));
    }
}
