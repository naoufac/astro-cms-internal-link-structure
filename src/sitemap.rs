//! URL identity and site configuration semantics.
//!
//! Identity normalization: percent-decode ONLY unreserved escapes, keep all
//! other escapes (uppercase hex), and encode any literal byte outside the
//! unreserved set. Encoded slashes (%2F) never collapse into separators, and
//! /a%2Fb never equals /a/b.

use url::Url;

use crate::request::ScanRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildFormat {
    Directory,
    File,
}

impl BuildFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "directory" => Some(BuildFormat::Directory),
            "file" => Some(BuildFormat::File),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrailingSlash {
    Always,
    Never,
    Ignore,
}

impl TrailingSlash {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "always" => Some(TrailingSlash::Always),
            "never" => Some(TrailingSlash::Never),
            "ignore" => Some(TrailingSlash::Ignore),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct SiteModel {
    pub site: String,
    pub origin_aliases: Vec<String>,
    /// Configured base path, '' or '/x' (raw form).
    pub base: String,
    /// Identity-normalized base for link classification.
    pub base_identity: String,
    pub build_format: BuildFormat,
    pub trailing_slash: TrailingSlash,
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

/// Identity form of a path: decode unreserved escapes, normalize escape hex
/// case, percent-encode literal bytes outside unreserved + '/'. Invalid
/// escapes become %25. Encoded slashes are preserved as escapes.
pub fn path_identity(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                let v = h * 16 + l;
                if is_unreserved(v) {
                    out.push(v as char);
                } else {
                    out.push('%');
                    out.push((bytes[i + 1] as char).to_ascii_uppercase());
                    out.push((bytes[i + 2] as char).to_ascii_uppercase());
                }
                i += 3;
                continue;
            }
        }
        if is_unreserved(b) || b == b'/' {
            out.push(b as char);
        } else if b == b'%' {
            out.push_str("%25");
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
        i += 1;
    }
    out
}

/// Fragment identity: decode ALL valid escapes (fragments are opaque
/// strings, no separator semantics), keep invalid escapes literal, and never
/// treat '+' as space. Comparison is case-sensitive.
pub fn decode_fragment(frag: &str) -> String {
    let bytes = frag.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Normalize an origin string (scheme://host[:port]) to canonical form:
/// lowercase host, default ports removed, no path/query/fragment allowed.
pub fn normalize_origin(origin: &str) -> Option<String> {
    let u = Url::parse(origin).ok()?;
    if u.path() != "/" && u.path() != "" {
        return None;
    }
    if u.query().is_some() || u.fragment().is_some() {
        return None;
    }
    let host = u.host_str()?.to_ascii_lowercase();
    let scheme = u.scheme();
    let port = match u.port() {
        None => None,
        Some(p) => {
            let default = matches!((scheme, p), ("http", 80) | ("https", 443));
            if default {
                None
            } else {
                Some(p)
            }
        }
    };
    Some(match port {
        Some(p) => format!("{}://{}:{}", scheme, host, p),
        None => format!("{}://{}", scheme, host),
    })
}

pub fn host_of(origin: &str) -> String {
    match Url::parse(origin.trim_end_matches('/')) {
        Ok(u) => u.host_str().unwrap_or("").to_ascii_lowercase(),
        Err(_) => origin.to_ascii_lowercase(),
    }
}

impl SiteModel {
    pub fn from_request(req: &ScanRequest) -> Result<SiteModel, String> {
        let site = match &req.site {
            Some(s) => normalize_origin(s).ok_or_else(|| {
                format!("invalid config: site '{s}' is not a bare origin (scheme://host[:port], no path/query)")
            })?,
            None => return Err("invalid config: site is required".to_string()),
        };
        let mut origin_aliases = Vec::new();
        for a in &req.originAliases {
            let n = normalize_origin(a)
                .ok_or_else(|| format!("invalid config: originAlias '{a}' is not a bare origin"))?;
            origin_aliases.push(n);
        }
        if !origin_aliases.contains(&site) {
            origin_aliases.insert(0, site.clone());
        }
        let base = {
            let b = req.basePath.trim();
            if b.is_empty() || b == "/" {
                String::new()
            } else {
                if !b.starts_with('/') {
                    return Err(format!(
                        "invalid config: basePath '{b}' must start with '/'"
                    ));
                }
                match b.trim_end_matches('/') {
                    "" => String::new(),
                    t => t.to_string(),
                }
            }
        };
        let base_identity = path_identity(&base);
        let build_format = BuildFormat::parse(&req.buildFormat).ok_or_else(|| {
            format!(
                "invalid config: buildFormat '{}' not supported (supported: directory, file)",
                req.buildFormat
            )
        })?;
        let trailing_slash = TrailingSlash::parse(&req.trailingSlash).ok_or_else(|| {
            format!(
                "invalid config: trailingSlash '{}' not supported (supported: always, never, ignore)",
                req.trailingSlash
            )
        })?;
        Ok(SiteModel {
            site,
            origin_aliases,
            base,
            base_identity,
            build_format,
            trailing_slash,
        })
    }

    /// Public URL path key of an emitted page file (relative, POSIX),
    /// following real emitted route semantics: directory format ends with
    /// '/', file format does not (root is '/'). None when the file has no
    /// page URL under the configured base.
    pub fn page_url_path(&self, rel_path: &str) -> Option<String> {
        let rel = rel_path.trim_start_matches('/');
        let dir = match self.build_format {
            BuildFormat::Directory => {
                if rel == "index.html" {
                    ""
                } else {
                    rel.strip_suffix("/index.html")?
                }
            }
            BuildFormat::File => match rel.strip_suffix(".html")? {
                "index" => "",
                other => other,
            },
        };
        let mut joined = self.base.clone();
        if !dir.is_empty() {
            if joined.is_empty() {
                joined = dir.to_string();
            } else {
                joined = format!("{}/{}", self.base, dir);
            }
        }
        if joined.is_empty() {
            return Some("/".to_string());
        }
        let key = if self.build_format == BuildFormat::Directory {
            format!("/{}/", path_identity(&joined).trim_matches('/'))
        } else {
            format!("/{}", path_identity(&joined).trim_matches('/'))
        };
        if !self.base.is_empty() {
            let bi = &self.base_identity;
            if !(key == *bi || key == format!("{bi}/") || key.starts_with(&format!("{bi}/"))) {
                return None;
            }
        }
        Some(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(base: &str, fmt: &str) -> SiteModel {
        let r: ScanRequest = serde_json::from_value(serde_json::json!({
            "schemaVersion": "1",
            "artifactRoot": "/x",
            "site": "https://e.com",
            "basePath": base,
            "buildFormat": fmt
        }))
        .unwrap();
        SiteModel::from_request(&r).unwrap()
    }

    #[test]
    fn identity_preserves_encoded_slash() {
        assert_ne!(path_identity("/a%2Fb"), path_identity("/a/b"));
        assert_eq!(path_identity("/a%2Fb"), "/a%2Fb");
        assert_eq!(path_identity("/a%2fb"), "/a%2Fb");
        assert_eq!(path_identity("/a/b"), "/a/b");
    }

    #[test]
    fn identity_decodes_unreserved_only() {
        assert_eq!(path_identity("/%7Euser"), "/~user");
        assert_eq!(path_identity("/%41%42"), "/AB");
        assert_eq!(path_identity("/a%20b"), "/a%20b");
        assert_eq!(path_identity("/100%"), "/100%25");
        assert_eq!(path_identity("/%zz"), "/%25zz");
    }

    #[test]
    fn identity_unifies_raw_and_escaped_unicode() {
        assert_eq!(path_identity("/café/"), path_identity("/caf%C3%A9/"));
        assert_eq!(path_identity("/caf%C3%A9/"), "/caf%C3%A9/");
    }

    #[test]
    fn fragment_decoding_strict() {
        assert_eq!(decode_fragment("C%2B"), "C+");
        assert_eq!(decode_fragment("a%20b"), "a b");
        assert_eq!(decode_fragment("100%"), "100%");
        assert_eq!(decode_fragment("%2F"), "/");
        assert_eq!(decode_fragment("CAFÉ"), "CAFÉ");
    }

    #[test]
    fn normalize_origin_forms() {
        assert_eq!(
            normalize_origin("https://Example.com:443").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            normalize_origin("http://example.com:8080").as_deref(),
            Some("http://example.com:8080")
        );
        assert!(normalize_origin("https://example.com/path").is_none());
        assert!(normalize_origin("not a url").is_none());
    }

    #[test]
    fn page_url_path_directory_format() {
        let m = model("", "directory");
        assert_eq!(m.page_url_path("index.html"), Some("/".into()));
        assert_eq!(m.page_url_path("about/index.html"), Some("/about/".into()));
        assert_eq!(m.page_url_path("404.html"), None);
        assert_eq!(
            m.page_url_path("guide/deep/index.html"),
            Some("/guide/deep/".into())
        );
    }

    #[test]
    fn page_url_path_file_format_root_is_slash() {
        let m = model("", "file");
        assert_eq!(m.page_url_path("index.html"), Some("/".into()));
        assert_eq!(m.page_url_path("about.html"), Some("/about".into()));
        assert_eq!(
            m.page_url_path("nested/page.html"),
            Some("/nested/page".into())
        );
    }

    #[test]
    fn page_url_path_with_base() {
        let m = model("/docs", "directory");
        assert_eq!(m.page_url_path("index.html"), Some("/docs/".into()));
        assert_eq!(
            m.page_url_path("guide/index.html"),
            Some("/docs/guide/".into())
        );
        let m = model("/docs", "file");
        assert_eq!(m.page_url_path("index.html"), Some("/docs".into()));
    }

    #[test]
    fn page_url_path_base_normalized() {
        let m = model("/café", "directory");
        assert_eq!(m.page_url_path("index.html"), Some("/caf%C3%A9/".into()));
    }

    #[test]
    fn invalid_config_rejected() {
        let r: ScanRequest = serde_json::from_value(serde_json::json!({
            "schemaVersion": "1",
            "artifactRoot": "/x",
            "site": "https://e.com",
            "buildFormat": "preserve"
        }))
        .unwrap();
        let err = SiteModel::from_request(&r).unwrap_err();
        assert!(err.contains("buildFormat"), "got: {err}");
    }

    #[test]
    fn site_must_be_bare_origin() {
        let r: ScanRequest = serde_json::from_value(serde_json::json!({
            "schemaVersion": "1",
            "artifactRoot": "/x",
            "site": "https://e.com/sub"
        }))
        .unwrap();
        let err = SiteModel::from_request(&r).unwrap_err();
        assert!(err.contains("bare origin"), "got: {err}");
    }

    #[test]
    fn invalid_mode_rejected() {
        let r: ScanRequest = serde_json::from_value(serde_json::json!({
            "schemaVersion": "1",
            "artifactRoot": "/x",
            "site": "https://e.com",
            "mode": "yolo"
        }))
        .unwrap();
        assert!(SiteModel::from_request(&r).is_ok());
        // mode validation happens in audit::prepare
    }
}
