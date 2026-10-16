use crate::{
    audit::page_public_url,
    links::{classify, Inventory, LinkClass},
    parser::PageLinks,
    sitemap::SiteModel,
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct Metadata {
    pub title: String,
    pub canonicals: Vec<String>,
    pub canonicalStatus: String,
    pub canonicalResolved: Option<String>,
    pub robots: BTreeMap<String, Vec<String>>,
    pub noindex: bool,
    pub eligible: bool,
    pub basis: &'static str,
}

pub(crate) fn valid_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.iter().enumerate().all(|(i, b)| {
        *b != b'%'
            || (bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit))
    })
}

pub fn assess(key: &str, page: &PageLinks, site: &SiteModel, inv: &Inventory) -> Metadata {
    let noindex = page.robots.values().flatten().any(|s| {
        s.split(|c: char| c == ',' || c.is_whitespace())
            .any(|d| matches!(d.to_ascii_lowercase().as_str(), "noindex" | "none"))
    });
    let mut resolved = None;
    let status = match page.canonicals.as_slice() {
        [] => "absent",
        [href] => {
            let u = page_public_url(site, key).and_then(|base| base.join(href.trim()).ok());
            match u {
                Some(u)
                    if !href.trim().is_empty()
                        && !href.chars().any(char::is_control)
                        && valid_encoding(href)
                        && matches!(u.scheme(), "http" | "https")
                        && u.username().is_empty()
                        && u.password().is_none()
                        && u.fragment().is_none() =>
                {
                    resolved = Some(u.to_string());
                    match classify(&u, site, inv) {
                        LinkClass::Page(target) if target == key && u.query().is_none() => "self",
                        LinkClass::External(_) => "external",
                        _ => "noncanonical",
                    }
                }
                _ => "malformed",
            }
        }
        _ => "multiple",
    };
    Metadata {
        title: page.title.clone(),
        canonicals: page.canonicals.clone(),
        canonicalStatus: status.to_string(),
        canonicalResolved: resolved,
        robots: page.robots.clone(),
        noindex,
        eligible: !noindex && matches!(status, "absent" | "self"),
        basis:
            "Offline artifact signals only, not Google's selected canonical or verified indexation",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_and_robots_variants() {
        let req = serde_json::from_value(serde_json::json!({"schemaVersion":"1","artifactRoot":"/unused","site":"https://e.com"})).unwrap();
        let site = SiteModel::from_request(&req).unwrap();
        let mut inv = Inventory::default();
        inv.pages.insert("/".into(), Default::default());
        for (html, status, eligible) in [
            ("", "absent", true),
            ("<link rel='canonical' href='/'>", "self", true),
            ("<link rel='canonical' href='http://['>", "malformed", false),
            ("<link rel='canonical' href=''>", "malformed", false),
            (
                "<link rel='canonical' href='https://other.com/'>",
                "external",
                false,
            ),
            (
                "<link rel='canonical' href='/else/'>",
                "noncanonical",
                false,
            ),
            (
                "<link rel='canonical' href='/'><link rel='canonical' href='/'>",
                "multiple",
                false,
            ),
            (
                "<meta name='ROBOTS' content='INDEX, noindex'>",
                "absent",
                false,
            ),
            ("<meta name='googlebot' content='none'>", "absent", false),
        ] {
            let m = assess("/", &crate::parser::extract(html), &site, &inv);
            assert_eq!(m.canonicalStatus, status, "{html}");
            assert_eq!(m.eligible, eligible, "{html}");
        }
    }
}
