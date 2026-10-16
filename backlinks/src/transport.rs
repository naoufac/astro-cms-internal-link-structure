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

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;

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
    timeout_seconds: u64,
    max_body_bytes: usize,
}

impl PinnedReqwestTransport {
    /// Build the production client: blocking, rustls with native roots,
    /// no proxy env honored (built with default-features=false), gzip/brotli
    /// decompression on, no cookies, no credentials, GET only.
    pub fn new(timeout_seconds: u64) -> anyhow::Result<Self> {
        Self::with_body_limit(
            timeout_seconds,
            crate::report::Limits::default().max_body_bytes,
        )
    }

    pub fn with_body_limit(timeout_seconds: u64, max_body_bytes: usize) -> anyhow::Result<Self> {
        Ok(Self {
            timeout_seconds,
            max_body_bytes,
        })
    }

    fn build_client(&self, host: &str, socket: std::net::SocketAddr) -> io::Result<Client> {
        Client::builder()
            .timeout(Duration::from_secs(self.timeout_seconds))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .dns_resolver(std::sync::Arc::new(SelectedAddress {
                host: host.to_string(),
                socket,
            }))
            .build()
            .map_err(io_err)
    }
}

struct SelectedAddress {
    host: String,
    socket: std::net::SocketAddr,
}

impl reqwest::dns::Resolve for SelectedAddress {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let result: Result<reqwest::dns::Addrs, Box<dyn std::error::Error + Send + Sync>> =
            if name.as_str() == self.host {
                Ok(Box::new(std::iter::once(self.socket)))
            } else {
                Err(io::Error::other("unvetted DNS name rejected").into())
            };
        Box::pin(std::future::ready(result))
    }
}

fn read_capped(reader: &mut impl io::Read, max: usize) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let remaining = max.saturating_sub(body.len());
        let limit = chunk.len().min(remaining.saturating_add(1));
        let n = reader.read(&mut chunk[..limit])?;
        if n == 0 {
            return Ok(body);
        }
        if n > remaining {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "decompressed response exceeded body cap",
            ));
        }
        body.extend_from_slice(&chunk[..n]);
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
        let authority = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let raw = format!("{scheme}://{authority}{path_and_query}");
        let url = reqwest::Url::parse(&raw)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("bad URL: {e}")))?;
        let validated = crate::neturl::safe_parse(&raw)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.0))?;
        if !crate::neturl::ip_is_global(ip) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "selected IP is not public",
            ));
        }
        if let Ok(literal) = validated.host_ascii().parse::<IpAddr>() {
            if literal != ip {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "literal URL differs from selected IP",
                ));
            }
        }
        let socket = std::net::SocketAddr::new(ip, port);
        let client = self.build_client(validated.host_ascii(), socket)?;
        let status;
        let location;
        let content_type;
        let body;
        {
            let mut resp = client
                .get(url)
                .header("User-Agent", user_agent)
                .header("Accept", "text/html,application/xhtml+xml;q=0.8,*/*;q=0.5")
                .send()
                .map_err(io_err)?;
            if resp.remote_addr() != Some(socket) {
                return Err(io::Error::other(format!(
                    "socket pin mismatch: selected {socket}, connected {:?}",
                    resp.remote_addr()
                )));
            }
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
            body = read_capped(&mut resp, self.max_body_bytes)?;
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
