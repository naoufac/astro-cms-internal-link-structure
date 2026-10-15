//! HTTP transport seam and the production pinned-reqwest implementation.
//!
//! The engine only ever dials a validated IP for a validated host: DNS is
//! resolved once per destination, every answer is filtered to publicly
//! routable addresses, and the chosen address is pinned into the reqwest
//! client via `resolve()` so no later request can silently rebind to a
//! different (potentially private) address.

use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::time::Duration;

use reqwest::blocking::Client;

/// Result of one HTTP response inside the engine.
#[derive(Debug, Clone)]
pub struct RawResponse {
    pub status: u16,
    /// Already-decompressed body bytes.
    pub body: Vec<u8>,
    pub location: Option<String>,
    pub content_type: Option<String>,
}

/// Minimal HTTP transport seam (GET only, no credentials, no cookies).
pub trait HttpTransport: Send + Sync {
    /// Resolve `host` to candidate addresses. Implementations MUST apply no
    /// filtering here; the engine filters to public addresses itself.
    fn resolve(&self, host: &str) -> io::Result<Vec<IpAddr>>;

    /// GET against a vetted (scheme, host, port, ip) tuple. Implementations
    /// dial `ip` for `host`: never a second DNS lookup.
    fn get(
        &self,
        scheme: &str,
        host: &str,
        port: u16,
        ip: IpAddr,
        path_and_query: &str,
        user_agent: &str,
    ) -> io::Result<RawResponse>;
}

fn io_err(e: reqwest::Error) -> io::Error {
    match e.status() {
        Some(_) => io::Error::other(format!("http error: {e}")),
        None => io::Error::other(format!("transport error: {e}")),
    }
}

/// Production transport. Redirects are disabled at the client level; the
/// engine follows them manually with full re-validation per hop.
pub struct PinnedReqwestTransport {
    client: std::sync::Mutex<Client>,
    timeout_seconds: u64,
    pins: std::sync::Mutex<std::collections::HashMap<String, std::net::SocketAddr>>,
}

impl PinnedReqwestTransport {
    /// Build the production client: blocking, rustls with native roots,
    /// no proxy env honored (built with default-features=false), gzip/brotli
    /// decompression on, no cookies, no credentials, GET only.
    pub fn new(timeout_seconds: u64) -> anyhow::Result<Self> {
        let client = Self::build_client(timeout_seconds)?;
        Ok(PinnedReqwestTransport {
            client: std::sync::Mutex::new(client),
            timeout_seconds,
            pins: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    fn build_client(timeout_seconds: u64) -> anyhow::Result<Client> {
        Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {e}"))
    }
}

impl HttpTransport for PinnedReqwestTransport {
    fn resolve(&self, host: &str) -> io::Result<Vec<IpAddr>> {
        // Use the system resolver via std, keeping resolution independent of
        // any proxy stack. Returns raw answers; engine filters to public.
        std::net::ToSocketAddrs::to_socket_addrs(&(host, 443))
            .map(|it| it.map(|sa| sa.ip()).collect())
            .map_err(|e| io::Error::new(e.kind(), format!("DNS lookup failed for {host}: {e}")))
    }

    fn get(
        &self,
        scheme: &str,
        host: &str,
        port: u16,
        ip: IpAddr,
        path_and_query: &str,
        user_agent: &str,
    ) -> io::Result<RawResponse> {
        let authority = format!("{host}:{port}");
        let raw = format!("{scheme}://{authority}{path_and_query}");
        let url = reqwest::Url::parse(&raw)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("bad URL: {e}")))?;
        // Pin the validated IP: all requests for host:port go to ip.
        // Pin host:port -> validated IP. reqwest pins live on the client
        // builder, so when a new authority appears we rebuild the shared
        // client with the full pin set. The pin maps the authority (host and
        // port) to the exact vetted SocketAddr, so every request dials that
        // address and DNS rebinding is impossible.
        let socket = std::net::SocketAddr::new(ip, port);
        let needs_rebuild = {
            let pins = self.pins.lock().unwrap();
            pins.get(&authority) != Some(&socket)
        };
        if needs_rebuild {
            let mut pins = self.pins.lock().unwrap();
            pins.insert(authority.clone(), socket);
            let mut b = Client::builder()
                .timeout(Duration::from_secs(self.timeout_seconds))
                .redirect(reqwest::redirect::Policy::none());
            for (a, sa) in pins.iter() {
                b = b.resolve_to_addrs(a, &[*sa]);
            }
            let rebuilt = b
                .build()
                .map_err(|e| io::Error::other(format!("client rebuild: {e}")))?;
            *self.client.lock().unwrap() = rebuilt;
        }
        let status;
        let location;
        let content_type;
        let body;
        {
            let client = self.client.lock().unwrap();
            let mut resp = client
                .get(url)
                .header("User-Agent", user_agent)
                .header("Accept", "text/html,application/xhtml+xml;q=0.8,*/*;q=0.5")
                .send()
                .map_err(io_err)?;
            status = resp.status().as_u16();
            location = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_ascii_lowercase());
            body = {
                let mut buf = Vec::new();
                use std::io::Read as _;
                resp.read_to_end(&mut buf)
                    .map_err(|e| io::Error::other(format!("body read failed: {e}")))?;
                buf
            };
        }
        Ok(RawResponse {
            status,
            body,
            location,
            content_type,
        })
    }
}

/// Test-only in-memory transport with deterministic responses. Production
/// code never constructs this; there is no localhost bypass anywhere.
#[doc(hidden)]
pub mod fake {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    pub struct FakeTransport {
        /// Key: `https://host/path?query` (default scheme https for tests).
        pub routes: HashMap<String, RawResponse>,
        /// Optional redirect overrides resolved before `routes`.
        pub redirects: HashMap<String, (u16, String)>,
        pub default_status: u16,
        pub requests: Arc<AtomicUsize>,
        /// DNS answers per host. Missing host = NXDOMAIN (no answers).
        pub dns: HashMap<String, Vec<IpAddr>>,
        /// Record of dialed IPs, for pin-behavior assertions.
        pub dialed: Arc<Mutex<Vec<String>>>,
    }

    impl FakeTransport {
        pub fn new(hosts: &[&str]) -> Self {
            FakeTransport {
                routes: HashMap::new(),
                redirects: HashMap::new(),
                default_status: 200,
                requests: Arc::new(AtomicUsize::new(0)),
                dns: hosts
                    .iter()
                    .map(|h| ((*h).to_string(), vec!["93.184.216.34".parse().unwrap()]))
                    .collect(),
                dialed: Arc::new(Mutex::new(Vec::new())),
            }
        }

        pub fn route(&mut self, key: &str, status: u16, body: &str) {
            self.routes.insert(
                key.to_string(),
                RawResponse {
                    status,
                    body: body.as_bytes().to_vec(),
                    location: None,
                    content_type: Some("text/html".into()),
                },
            );
        }

        pub fn route_redirect(&mut self, key: &str, status: u16, location: &str) {
            self.redirects
                .insert(key.to_string(), (status, location.to_string()));
        }

        pub fn request_count(&self) -> usize {
            self.requests.load(Ordering::SeqCst)
        }
    }

    impl HttpTransport for FakeTransport {
        fn resolve(&self, host: &str) -> io::Result<Vec<IpAddr>> {
            Ok(self.dns.get(host).cloned().unwrap_or_default())
        }

        fn get(
            &self,
            _scheme: &str,
            host: &str,
            _port: u16,
            ip: IpAddr,
            path_and_query: &str,
            _user_agent: &str,
        ) -> io::Result<RawResponse> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            self.dialed
                .lock()
                .unwrap()
                .push(format!("{host} -> {ip}{path_and_query}"));
            let key = format!("https://{host}{path_and_query}");
            if let Some((status, loc)) = self.redirects.get(&key) {
                return Ok(RawResponse {
                    status: *status,
                    body: Vec::new(),
                    location: Some(loc.clone()),
                    content_type: None,
                });
            }
            Ok(self.routes.get(&key).cloned().unwrap_or(RawResponse {
                status: self.default_status,
                body: b"default".to_vec(),
                location: None,
                content_type: Some("text/html".into()),
            }))
        }
    }
}
