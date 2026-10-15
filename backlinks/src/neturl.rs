//! URL parsing, normalization, and public-address safety classification.
//!
//! Safety rules enforced here (see README "Network safety"):
//! - only `http`/`https` schemes on standard ports (80/443, or empty);
//! - no userinfo/credentials;
//! - host must be a canonical dotted-quad IPv4 literal, an IPv6 literal, or a
//!   domain: and in every case must classify as a *publicly routable*
//!   address (domains are re-validated at DNS resolution time in `fetch.rs`);
//! - the url crate canonicalizes nonstandard IPv4 text forms (decimal, hex,
//!   octal, short) per WHATWG, so classification always sees dotted-quad;
//!   as a belt-and-braces measure any host made solely of hex digits, dots,
//!   and `x`/`X` that is not a canonical global dotted-quad is rejected.
//!
//! IPv6 policy is a conservative ALLOWLIST: only IANA global unicast
//! `2000::/3` may pass, minus documented special-use carve-outs (IETF
//! protocol assignments incl. Teredo, 6to4, NAT64, documentation, ORCHID).
//! Everything else: loopback, unspecified, discards `100::/64`, ULA,
//! link-local, multicast, IPv4-mapped private, NAT64 embedded private: is
//! rejected. Standard-library `IpAddr::is_global` is unstable on the pinned
//! toolchain and is deliberately NOT used.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use idna::domain_to_ascii;
use url::Url;

/// Reason a URL was rejected before any network activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectReason(pub String);

fn reject<T>(msg: impl Into<String>) -> Result<T, RejectReason> {
    Err(RejectReason(msg.into()))
}

/// Canonicalize a host to its ASCII (punycode) form; already-ASCII hosts pass
/// through unchanged apart from trailing-dot removal.
pub fn host_to_ascii(host: &str) -> Result<String, RejectReason> {
    domain_to_ascii(host.trim().trim_end_matches('.'))
        .map(|s| s.to_ascii_lowercase())
        .map_err(|_| RejectReason("host is not a valid domain".into()))
}

/// True when the IPv4 address is globally routable. Mirrors the IANA
/// IPv4 special-purpose registry.
pub fn ipv4_is_global(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    match o {
        [0, _, _, _] => false,                      // this-network
        [10, _, _, _] => false,                     // private
        [100, 64..=127, _, _] => false,             // CGNAT (RFC 6598)
        [127, _, _, _] => false,                    // loopback
        [169, 254, _, _] => false,                  // link-local
        [172, 16..=31, _, _] => false,              // private
        [192, 0, 0, _] => false,                    // IETF protocol assignments
        [192, 0, 2, _] => false,                    // TEST-NET-1
        [192, 168, _, _] => false,                  // private
        [198, 18, _, _] | [198, 19, _, _] => false, // benchmarking
        [198, 51, 100, _] => false,                 // TEST-NET-2
        [203, 0, 113, _] => false,                  // TEST-NET-3
        [224..=255, _, _, _] => false,              // multicast / reserved / broadcast
        _ => true,
    }
}

/// True when the IPv6 address is in a publicly routable range.
///
/// Conservative allowlist: `2000::/3` (global unicast) minus special-use
/// carve-outs. Deliberately rejects deprecated/transition encapsulations
/// outright (6to4, NAT64, Teredo) rather than trying to vet their embedded
/// IPv4: they are never genuine public IPv6 destinations.
pub fn ipv6_is_global(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    // Unspecified :: and loopback ::1
    if s == [0, 0, 0, 0, 0, 0, 0, 0] || s == [0, 0, 0, 0, 0, 0, 0, 1] {
        return false;
    }
    // IPv4-mapped ::ffff:0:0/96 and (deprecated) IPv4-compatible ::/96:
    // classify by the embedded IPv4 rules. Other 0000::/8 forms are not
    // global unicast and fall through to the 2000::/3 gate below.
    if s[0] == 0
        && s[1] == 0
        && s[2] == 0
        && s[3] == 0
        && s[4] == 0
        && (s[5] == 0 || s[5] == 0xffff)
    {
        let v4 = Ipv4Addr::new(
            (s[6] >> 8) as u8,
            (s[6] & 0xff) as u8,
            (s[7] >> 8) as u8,
            (s[7] & 0xff) as u8,
        );
        return ipv4_is_global(v4);
    }
    // NAT64 64:ff9b::/96 (and local-use 64:ff9b:1::/48 via the /3 gate below):
    // translation prefixes, never real public destinations.
    if s[0] == 0x0064 && s[1] == 0xff9b {
        return false;
    }
    // 6to4 2002::/16: deprecated (RFC 7526), embedded IPv4 not trusted.
    if s[0] == 0x2002 {
        return false;
    }
    // Documentation 2001:db8::/32.
    if s[0] == 0x2001 && s[1] == 0x0db8 {
        return false;
    }
    // IETF protocol assignments 2001::/24: Teredo 2001::/32, benchmarking
    // 2001:2::/48, AMT 2001:3::/32, ORCHID 2001:10::/28 + 2001:20::/28, etc.
    if s[0] == 0x2001 && (s[1] & 0xff00) == 0 {
        return false;
    }
    // Global unicast 2000::/3.
    (s[0] & 0xe000) == 0x2000
}

/// True when the IP is globally routable. Only such addresses may ever be
/// contacted.
pub fn ip_is_global(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_is_global(v4),
        IpAddr::V6(v6) => ipv6_is_global(v6),
    }
}

/// A URL that has passed every static safety check.
#[derive(Debug, Clone)]
pub struct SafeUrl {
    /// Normalized URL (ASCII host, standard port elided, no fragment).
    pub normalized: Url,
    /// Host in ASCII (punycode) lowercase form.
    pub host_ascii: String,
}

impl SafeUrl {
    pub fn scheme(&self) -> &str {
        self.normalized.scheme()
    }
    pub fn host_ascii(&self) -> &str {
        &self.host_ascii
    }
    /// Registrable-origin key: `scheme://host` (ports are standard-only).
    pub fn origin_key(&self) -> String {
        format!("{}://{}", self.normalized.scheme(), self.host_ascii)
    }
    pub fn port(&self) -> u16 {
        self.normalized
            .port()
            .unwrap_or(if self.scheme() == "https" { 443 } else { 80 })
    }
    /// Path + query, used as a page key.
    pub fn path_and_query(&self) -> String {
        match self.normalized.query() {
            Some(q) => format!("{}?{q}", self.normalized.path()),
            None => self.normalized.path().to_string(),
        }
    }
    pub fn as_str(&self) -> &str {
        self.normalized.as_str()
    }
}

/// Page-dedupe key: normalized URL with trailing slash collapsed (except the
/// bare origin root), so `/x` and `/x/` count as one page.
pub fn dedupe_key(u: &SafeUrl) -> String {
    let mut s = u.normalized.as_str().to_string();
    if s.len() > u.origin_key().len() && s.ends_with('/') {
        s.pop();
    }
    s
}

/// True when the host string consists solely of hex digits, dots, and
/// `x`/`X` (i.e. could be a nonstandard IPv4 text form).
fn looks_like_ipv4_text(host: &str) -> bool {
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '.' || c == 'x' || c == 'X')
}

/// Validate and normalize a URL without any network access.
///
/// Fails closed: credentials, non-standard schemes/ports, bracketed
/// non-IPv6 hosts, malformed percent-encoding, and any host that does not
/// classify as publicly routable.
pub fn safe_parse(raw: &str) -> Result<SafeUrl, RejectReason> {
    let raw = raw.trim();
    if raw.is_empty() {
        return reject("empty URL");
    }
    if raw.chars().any(|c| c.is_control()) {
        return reject("URL contains control characters");
    }
    let base = Url::parse(raw).map_err(|e| RejectReason(format!("URL parse error: {e}")))?;

    let scheme = base.scheme().to_string();
    if scheme != "http" && scheme != "https" {
        return reject(format!("scheme '{scheme}' not allowed; only http/https"));
    }
    if !base.username().is_empty() || base.password().is_some() {
        return reject("credentials in URL are not allowed");
    }
    if let Some(p) = base.port() {
        let expected = if scheme == "https" { 443 } else { 80 };
        if p != expected {
            return reject(format!("port {p} not allowed; only the scheme default"));
        }
    }
    let host = base
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| RejectReason("URL has no host".into()))?;

    // The url crate keeps IPv6 literals bracketed in host_str(); validate
    // directly against the IPv6 allowlist.
    if let Some(stripped) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        let v6: Ipv6Addr = stripped
            .parse()
            .map_err(|_| RejectReason("bracketed host is not a valid IPv6 literal".into()))?;
        if !ipv6_is_global(v6) {
            return reject(format!("IPv6 literal {stripped} is not a public address"));
        }
        let ascii = host_to_ascii(stripped)?;
        return finish(base, scheme, ascii);
    }

    // Domains and IPv4 literals: canonicalize to ASCII.
    let ascii = host_to_ascii(host)?;
    if ascii.is_empty() {
        return reject("empty host after IDNA");
    }

    // Canonical dotted-quad? The url crate has already canonicalized any
    // nonstandard IPv4 text form (hex/octal/decimal/short) per WHATWG, so a
    // strict 4-part parse suffices here.
    if let Ok(v4) = ascii.parse::<Ipv4Addr>() {
        if !ipv4_is_global(v4) {
            return reject(format!("IPv4 literal {v4} is not a public address"));
        }
        return finish(base, scheme, ascii);
    }

    // Belt-and-braces: a host that *looks* like a nonstandard IPv4 encoding
    // (hex/octal/decimal text) but was not canonicalized to dotted-quad is
    // rejected outright rather than resolved as a domain.
    if looks_like_ipv4_text(&ascii) {
        return reject(format!(
            "host '{ascii}' looks like a nonstandard IPv4 encoding and is not allowed"
        ));
    }

    // Regular domain: require a dot (no single-label or intranet hosts).
    if !ascii.contains('.') {
        return reject(format!("host '{ascii}' has no public suffix"));
    }
    finish(base, scheme, ascii)
}

fn finish(mut base: Url, _scheme: String, ascii: String) -> Result<SafeUrl, RejectReason> {
    // Reject malformed percent-encoding anywhere (url crate keeps stray '%').
    let bytes = base.as_str().as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex_ok = i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit();
            if !hex_ok {
                return reject("URL contains malformed percent-encoding");
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    base.set_fragment(None);
    Ok(SafeUrl {
        normalized: base,
        host_ascii: ascii,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_public_http_and_https() {
        for url in [
            "https://example.com/page",
            "http://example.com/",
            "https://sub.example.co.uk/x?a=1#frag",
        ] {
            assert!(safe_parse(url).is_ok(), "{url} should be accepted");
        }
        let s = safe_parse("https://Example.COM:443/A#frag").unwrap();
        assert_eq!(s.host_ascii(), "example.com");
        assert!(!s.normalized.as_str().contains(":443"));
        assert!(!s.normalized.as_str().contains("#frag"));
    }

    #[test]
    fn url_crate_canonicalizes_nonstandard_ipv4_forms_before_classification() {
        // WHATWG IPv4 parsing canonicalizes every nonstandard textual form to
        // dotted-quad; the global-address check then rejects non-public ones.
        // (input, expected canonical host)
        let cases = [
            ("http://2130706433/x", "127.0.0.1"),                // decimal
            ("http://0x7f000001/x", "127.0.0.1"),                // hex
            ("http://0177.0.0.1/x", "127.0.0.1"),                // octal short
            ("http://127.1/x", "127.0.0.1"),                     // short
            ("http://127.0.1/x", "127.0.0.1"),                   // partial
            ("http://0177.0000.0000.0001/x", "127.0.0.1"),       // octal octets
            ("http://0xA9FEB2C4/x", "169.254.178.196"),          // link-local hex
            ("http://0xC0A80001/x", "192.168.0.1"),              // private hex
            ("http://0xff.0xff.0xff.0xff/x", "255.255.255.255"), // reserved
        ];
        for (raw, want) in cases {
            let parsed = Url::parse(raw).expect("url crate should parse");
            let h = parsed.host_str().unwrap();
            assert_eq!(h, want, "canonicalization for {raw}");
            assert!(safe_parse(raw).is_err(), "{raw} must be rejected");
        }
        // Mixed hex/octal octets are a parse error outright.
        assert!(Url::parse("http://0xA9.FE.0.1/x").is_err());
        // Hex-only text that is NOT an IPv4 form stays a domain; it is only
        // fetchable if DNS resolves it to a public address (checked live).
        let dom = safe_parse("http://deadbeef/x");
        assert!(dom.is_err(), "single-label host must be rejected");
    }

    #[test]
    fn hex_only_lookalike_hosts_rejected_even_if_url_crate_diverges() {
        for raw in [
            "http://0x7f.1/x",
            "http://deadbeef/x",
            "http://0xff.0xff.0xff.0xff/x",
        ] {
            assert!(safe_parse(raw).is_err(), "{raw} must be rejected");
        }
    }

    #[test]
    fn rejects_private_and_reserved_targets() {
        for url in [
            "http://localhost/x",
            "http://127.0.0.1/x",
            "http://10.0.0.1/x",
            "http://172.16.0.1/x",
            "http://172.31.255.255/x",
            "http://192.168.1.1/x",
            "http://169.254.169.254/latest/meta-data/", // cloud metadata
            "http://100.64.0.1/x",                      // CGNAT
            "http://198.18.0.1/x",                      // benchmarking
            "http://224.0.0.1/x",                       // multicast
            "http://0.0.0.0/x",
            "https://192.0.2.10/x",  // TEST-NET-1
            "https://203.0.113.9/x", // TEST-NET-3
        ] {
            assert!(safe_parse(url).is_err(), "{url} must be rejected");
        }
    }

    #[test]
    fn rejects_unsafe_ipv6_forms() {
        for url in [
            "http://[::1]/x",
            "http://[::]/x",
            "http://[fe80::1]/x",                                 // link-local
            "http://[fc00::1]/x",                                 // ULA
            "http://[fd12:3456:789a::1]/x",                       // ULA
            "http://[ff02::1]/x",                                 // multicast
            "http://[100::1]/x",                                  // discard-only range
            "http://[::ffff:127.0.0.1]/x",                        // IPv4-mapped loopback
            "http://[::ffff:10.0.0.1]/x",                         // IPv4-mapped private
            "http://[::ffff:169.254.169.254]/x",                  // mapped metadata
            "http://[64:ff9b::a9fe:a9fe]/x",                      // NAT64 embedded 169.254.169.254
            "http://[64:ff9b::8068:8068]/x",                      // NAT64 embedded 128.104.128.104
            "http://[2002:7f00:1::1]/x",                          // 6to4 loopback
            "http://[2002:a9fe:a9fe::1]/x",                       // 6to4 metadata
            "http://[2001:0000:4136:e378:8000:63bf:3fff:fdd2]/x", // Teredo
            "http://[2001:2::1]/x",                               // benchmarking
            "http://[2001:db8::1]/x",                             // documentation
            "http://[2001:10::1]/x",                              // ORCHID
        ] {
            assert!(safe_parse(url).is_err(), "{url} must be rejected");
        }
    }

    #[test]
    fn accepts_real_public_ipv6_literal() {
        // Google Public DNS and Cloudflare DNS literals are global unicast.
        assert!(safe_parse("http://[2001:4860:4860::8888]/x").is_ok());
        assert!(safe_parse("http://[2606:4700:4700::1111]/x").is_ok());
    }

    #[test]
    fn rejects_credentials_schemes_and_ports() {
        assert!(safe_parse("https://user:pass@example.com/x").is_err());
        assert!(safe_parse("https://user@example.com/x").is_err());
        assert!(safe_parse("ftp://example.com/x").is_err());
        assert!(safe_parse("file:///etc/passwd").is_err());
        assert!(safe_parse("https://example.com:8443/x").is_err());
        assert!(safe_parse("http://example.com:8080/x").is_err());
        assert!(safe_parse("javascript:alert(1)").is_err());
        assert!(safe_parse("").is_err());
        assert!(safe_parse("https://example.com:80/x").is_err());
        assert!(safe_parse("http://example.com:443/x").is_err());
    }

    #[test]
    fn rejects_hostname_smuggling() {
        assert!(safe_parse("http://[notip]/x").is_err());
        assert!(safe_parse("http://exa%mple.com/x").is_err());
        assert!(safe_parse("http://intranet/x").is_err());
        // Contains the word localhost but is a registrable public domain:
        // accepted at the URL layer; DNS-layer checks still apply.
        let s = safe_parse("http://localhost.example.com/x").unwrap();
        assert_eq!(s.host_ascii(), "localhost.example.com");
    }

    #[test]
    fn canonicalizes_unicode_host() {
        let s = safe_parse("https://BÜCHER.Example.ORG/path").unwrap();
        assert_eq!(s.host_ascii(), "xn--bcher-kva.example.org");
        let s = safe_parse("https://EXAMPLE.com./p").unwrap();
        assert_eq!(s.host_ascii(), "example.com");
    }
}
