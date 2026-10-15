//! Gated fetching engine: robots gate, DNS vetting, manual redirects.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::time::{Duration, Instant};

use robots_txt::Robots;

use crate::neturl::{ip_is_global, safe_parse, SafeUrl};
use crate::report::Limits;
use crate::transport::HttpTransport;

/// Transport/engine failure kinds with distinct report semantics.
#[derive(Debug, Clone)]
pub enum FetchError {
    /// URL failed static safety validation (never contacted).
    UnsafeTarget(String),
    /// DNS failed or answered no public address.
    Unreachable(String),
    /// robots.txt 401/403/429/5xx or unreachable: treated as NOT allowed.
    RobotsUnavailable(String),
    /// robots.txt explicitly disallows the path for our UA.
    RobotsDisallowed(String),
    /// Too many redirects or a cycle.
    RedirectLoop(String),
    /// Redirect to a downgraded or non-public destination.
    UnsafeRedirect(String),
    /// Body exceeded the configured cap.
    BodyTooLarge,
    /// Any other failure.
    Other(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::UnsafeTarget(s) => write!(f, "unsafe target: {s}"),
            FetchError::Unreachable(s) => write!(f, "unreachable: {s}"),
            FetchError::RobotsUnavailable(s) => write!(f, "robots unavailable: {s}"),
            FetchError::RobotsDisallowed(s) => write!(f, "robots disallowed: {s}"),
            FetchError::RedirectLoop(s) => write!(f, "redirect loop: {s}"),
            FetchError::UnsafeRedirect(s) => write!(f, "unsafe redirect: {s}"),
            FetchError::BodyTooLarge => write!(f, "response body exceeded cap"),
            FetchError::Other(s) => write!(f, "error: {s}"),
        }
    }
}

impl std::error::Error for FetchError {}

/// Owned extraction of the UA-relevant robots rules (order preserved).
#[derive(Debug, Clone)]
pub struct RobotsRules {
    /// (allow, path-prefix) in file order; first matching prefix decides.
    pub rules: Vec<(bool, String)>,
}

impl RobotsRules {
    /// First-match prefix decision, mirroring robots_txt's SimpleMatcher.
    pub fn path_allowed(&self, path: &str) -> bool {
        for (allow, prefix) in &self.rules {
            if prefix.is_empty() {
                return true;
            }
            if prefix.len() <= path.len() && path[..prefix.len()].eq_ignore_ascii_case(prefix) {
                return *allow;
            }
        }
        true
    }
}

/// Robots gate result for one origin.
#[derive(Debug, Clone)]
pub enum RobotsOutcome {
    /// robots.txt present (2xx): rules extracted for the run's user agent.
    Fetched(RobotsRules),
    /// robots.txt absent (404/410): no restrictions.
    Absent,
    /// robots unavailable (401/403/429/5xx, DNS failure, unsafe redirect):
    /// conservatively treated as denied for every path.
    Unavailable(String),
}

impl RobotsOutcome {
    /// Decide one path under this outcome.
    pub fn path_allowed(&self, _ua: &str, path: &str) -> Result<(), String> {
        match self {
            RobotsOutcome::Fetched(rules) => {
                if rules.path_allowed(path) {
                    Ok(())
                } else {
                    Err(format!("robots.txt disallows {path} for this agent"))
                }
            }
            RobotsOutcome::Absent => Ok(()),
            RobotsOutcome::Unavailable(r) => Err(r.clone()),
        }
    }
}

/// Fetch and parse robots.txt for an origin under full safety rules.
pub fn check_robots(
    origin: &str,
    user_agent: &str,
    limits: &Limits,
    transport: &dyn HttpTransport,
) -> RobotsOutcome {
    let robots_url = format!("{origin}/robots.txt");
    let url = match safe_parse(&robots_url) {
        Ok(u) => u,
        Err(e) => return RobotsOutcome::Unavailable(format!("robots URL rejected: {}", e.0)),
    };
    match fetch_with_redirects(&url, limits, transport, user_agent) {
        Ok(resp) => match resp.status {
            200..=299 => {
                let text = String::from_utf8_lossy(&resp.body).to_string();
                let robots = Robots::from_str_lossy(&text);
                let section = robots.choose_section(user_agent);
                let rules = section
                    .rules
                    .iter()
                    .map(|r| (r.allow, r.path.to_string()))
                    .collect();
                RobotsOutcome::Fetched(RobotsRules { rules })
            }
            404 | 410 => RobotsOutcome::Absent,
            s => RobotsOutcome::Unavailable(format!("robots.txt HTTP {s}")),
        },
        Err(e) => RobotsOutcome::Unavailable(e.to_string()),
    }
}

/// Robots outcome for an origin, cached per run.
pub fn robots_for_origin(
    origin: &str,
    user_agent: &str,
    limits: &Limits,
    transport: &dyn HttpTransport,
    cache: &mut HashMap<String, RobotsOutcome>,
) -> RobotsOutcome {
    if let Some(c) = cache.get(origin) {
        return c.clone();
    }
    let outcome = check_robots(origin, user_agent, limits, transport);
    cache.insert(origin.to_string(), outcome.clone());
    outcome
}

/// Outcome of a complete gated fetch.
#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub status: u16,
    pub final_url: String,
    pub body: Vec<u8>,
    pub redirect_chain: Vec<String>,
    pub content_type: Option<String>,
    /// Robots decision text for the report (None when this WAS robots.txt).
    pub robots_note: Option<String>,
}

/// Filter DNS answers to publicly routable addresses.
pub fn vetted_ips(url: &SafeUrl, transport: &dyn HttpTransport) -> Vec<IpAddr> {
    transport
        .resolve(&url.host_ascii)
        .unwrap_or_default()
        .into_iter()
        .filter(|ip| ip_is_global(*ip))
        .collect()
}

/// Full gated fetch: safety parse -> robots gate (explicit UA) -> manual
/// redirect chain with per-hop safety + DNS re-validation.
pub fn gated_get(
    raw_url: &str,
    user_agent: &str,
    limits: &Limits,
    transport: &dyn HttpTransport,
    robots_cache: &mut HashMap<String, RobotsOutcome>,
) -> Result<FetchOutcome, FetchError> {
    let url = safe_parse(raw_url).map_err(|e| FetchError::UnsafeTarget(e.0))?;
    let origin = url.origin_key();
    let path = url.path_and_query();
    let is_robots = path.ends_with("/robots.txt");

    let mut robots_note = None;
    if !is_robots {
        let outcome = robots_for_origin(&origin, user_agent, limits, transport, robots_cache);
        match &outcome {
            RobotsOutcome::Unavailable(r) => return Err(FetchError::RobotsUnavailable(r.clone())),
            RobotsOutcome::Fetched(_) => {
                outcome
                    .path_allowed(user_agent, &path)
                    .map_err(FetchError::RobotsDisallowed)?;
                robots_note = Some("robots.txt consulted; path allowed".to_string());
            }
            RobotsOutcome::Absent => {
                robots_note = Some("robots.txt absent; no restrictions".to_string());
            }
        }
    }
    let mut out = fetch_with_redirects(&url, limits, transport, user_agent)?;
    out.robots_note = robots_note;
    Ok(out)
}

/// Follow redirects manually. Every hop: safety-parse, DNS-vet, dial pinned.
/// HTTPS to HTTP downgrade rejected; hops > max and cycles are errors.
pub fn fetch_with_redirects(
    url: &SafeUrl,
    limits: &Limits,
    transport: &dyn HttpTransport,
    user_agent: &str,
) -> Result<FetchOutcome, FetchError> {
    let mut current = url.clone();
    let mut chain: Vec<String> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(format!(
        "{}{}",
        current.origin_key(),
        current.path_and_query()
    ));
    loop {
        let ips = vetted_ips(&current, transport);
        let Some(ip) = ips.into_iter().next() else {
            return Err(FetchError::Unreachable(format!(
                "no public address for {}",
                current.host_ascii()
            )));
        };
        let resp = transport
            .get(
                current.scheme(),
                current.host_ascii(),
                current.port(),
                ip,
                &current.path_and_query(),
                user_agent,
            )
            .map_err(|e| FetchError::Unreachable(e.to_string()))?;
        if (300..400).contains(&resp.status) {
            let Some(loc) = resp.location else {
                return Err(FetchError::UnsafeRedirect(
                    "redirect without Location header".into(),
                ));
            };
            let next = current
                .normalized
                .join(loc.trim())
                .map_err(|e| FetchError::UnsafeRedirect(format!("bad Location: {e}")))?;
            chain.push(next.to_string());
            if chain.len() > limits.max_redirects {
                return Err(FetchError::RedirectLoop(format!(
                    "more than {} redirects",
                    limits.max_redirects
                )));
            }
            let key = format!(
                "{}://{}{}",
                next.scheme(),
                next.host_str().unwrap_or(""),
                match next.query() {
                    Some(q) => format!("{}?{q}", next.path()),
                    None => next.path().to_string(),
                }
            );
            if !visited.insert(key) {
                return Err(FetchError::RedirectLoop("cycle detected".into()));
            }
            let next_safe =
                safe_parse(next.as_str()).map_err(|e| FetchError::UnsafeRedirect(e.0))?;
            if next_safe.scheme() == "http" && current.scheme() == "https" {
                return Err(FetchError::UnsafeRedirect("HTTPS downgrade".into()));
            }
            current = next_safe;
            continue;
        }
        if resp.body.len() > limits.max_body_bytes {
            return Err(FetchError::BodyTooLarge);
        }
        return Ok(FetchOutcome {
            status: resp.status,
            final_url: current.normalized.to_string(),
            body: resp.body,
            redirect_chain: chain,
            content_type: resp.content_type,
            robots_note: None,
        });
    }
}

/// Per-origin pacing (single-threaded engine loop).
pub struct Pacer {
    last: HashMap<String, Instant>,
    delay_ms: u64,
}

impl Pacer {
    pub fn new(delay_ms: u64) -> Self {
        Pacer {
            last: HashMap::new(),
            delay_ms,
        }
    }

    /// Block until the per-origin delay has elapsed for `origin`.
    pub fn wait(&mut self, origin: &str) {
        if self.delay_ms == 0 {
            return;
        }
        if let Some(t) = self.last.get(origin) {
            let need = Duration::from_millis(self.delay_ms);
            let elapsed = t.elapsed();
            if elapsed < need {
                std::thread::sleep(need - elapsed);
            }
        }
        self.last.insert(origin.to_string(), Instant::now());
    }
}
