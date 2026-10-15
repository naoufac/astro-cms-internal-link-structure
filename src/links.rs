//! Inventory and URL classification (origin scopes, assets, external, special schemes).

use std::collections::BTreeMap;
use std::path::Path;

use url::Url;

use crate::request::PageRecord;
use crate::sitemap::{decode_fragment, path_identity, SiteModel};

#[derive(Debug, Default)]
pub struct Inventory {
    /// Page URL path key (as produced by page_url_path) -> record.
    pub pages: BTreeMap<String, PageRecord>,
    /// Host + identity path (no trailing slash except root) -> fs path.
    pub assets: BTreeMap<(String, String), String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkClass {
    Page(String),
    Asset(String),
    Missing,
    External(String),
    Special,
    OutsideScope,
}

fn origin_of(u: &Url) -> String {
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    match u.port() {
        Some(p) => format!("{}://{}:{}", u.scheme(), host, p),
        None => format!("{}://{}", u.scheme(), host),
    }
}

/// Page-key forms a resolved URL path may match, per build format.
/// File format keys drop the '.html' suffix ('/index.html' is the root);
/// directory format keys end with '/'. Both written forms are accepted so
/// hosting variants (with or without '.html', with or without the trailing
/// slash) resolve to the same page node. Encoded slashes never collapse.
pub fn page_key_candidates(identity: &str, format: crate::sitemap::BuildFormat) -> Vec<String> {
    let mut out = vec![identity.to_string()];
    match format {
        crate::sitemap::BuildFormat::File => {
            if let Some(stripped) = identity.strip_suffix('/') {
                if !stripped.is_empty() {
                    out.push(stripped.to_string());
                }
            }
            if let Some(stripped) = identity.strip_suffix(".html") {
                out.push(if stripped.is_empty() {
                    "/".to_string()
                } else {
                    stripped.to_string()
                });
            }
            if let Some(prefix) = identity.strip_suffix("/index.html") {
                out.push(if prefix.is_empty() {
                    "/".to_string()
                } else {
                    prefix.to_string()
                });
            }
        }
        crate::sitemap::BuildFormat::Directory => {
            if identity != "/" {
                if identity.ends_with('/') {
                    if let Some(stripped) = identity.strip_suffix('/') {
                        if !stripped.is_empty() {
                            out.push(stripped.to_string());
                        }
                    }
                } else {
                    out.push(format!("{identity}/"));
                }
            }
        }
    }
    out
}

/// Classify a fragment-less URL already resolved against a page's public URL.
pub fn classify(u: &Url, site: &SiteModel, inv: &Inventory) -> LinkClass {
    if !matches!(u.scheme(), "http" | "https") {
        return LinkClass::Special;
    }
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    if !site.origin_aliases.contains(&origin_of(u)) {
        return LinkClass::External(origin_of(u));
    }
    let identity = path_identity(u.path());
    if !site.base.is_empty() {
        let bi = &site.base_identity;
        if !(identity == *bi
            || identity == format!("{bi}/")
            || identity.starts_with(&format!("{bi}/")))
        {
            return LinkClass::OutsideScope;
        }
    }
    for key in page_key_candidates(&identity, site.build_format) {
        if inv.pages.contains_key(&key) {
            return LinkClass::Page(key);
        }
    }
    for probe in asset_probes(&identity, site) {
        if let Some(fs) = inv.assets.get(&(host.clone(), probe)) {
            return LinkClass::Asset(fs.clone());
        }
    }
    LinkClass::Missing
}

/// Asset lookup keys for an identity path: the identity itself and, when a
/// base is configured, the identity with the base prefix removed (hosts may
/// serve base-prefixed asset URLs from base-less files).
fn asset_probes(identity: &str, site: &SiteModel) -> Vec<String> {
    let mut out = Vec::new();
    let push = |p: &str, out: &mut Vec<String>| {
        let key = p.trim_matches('/');
        if !key.is_empty() && !out.iter().any(|k| k == key) {
            out.push(key.to_string());
        }
    };
    push(identity, &mut out);
    if !site.base.is_empty() {
        let with_slash = format!("{}/", site.base_identity);
        if let Some(rest) = identity
            .strip_prefix(&with_slash)
            .or_else(|| identity.strip_prefix(&site.base_identity))
        {
            push(&format!("/{rest}"), &mut out);
        }
    }
    out
}

/// Fragment identity for comparison against id attributes.
pub fn fragment_identity(frag: &str) -> String {
    decode_fragment(frag)
}

pub fn build_inventory(root: &Path, site: &SiteModel) -> std::io::Result<Inventory> {
    let directory_format = matches!(site.build_format, crate::sitemap::BuildFormat::Directory);
    let mut inv = Inventory::default();
    for p in crate::scan::find_pages(root, directory_format)? {
        if let Some(url_path) = site.page_url_path(&p.filePath) {
            inv.pages.insert(url_path, p);
        }
    }
    let host = crate::sitemap::host_of(&site.site);
    for f in crate::scan::find_asset_files(root, directory_format)? {
        let identity = path_identity(&format!("/{f}"));
        let key = identity.trim_start_matches('/').to_string();
        inv.assets.insert((host.clone(), key), f);
    }
    Ok(inv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ails-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn model(base: &str, fmt: &str) -> SiteModel {
        let r: crate::request::ScanRequest = serde_json::from_value(serde_json::json!({
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
    fn inventory_and_classification_directory() {
        let root = tmpdir("inv-dir");
        fs::create_dir_all(root.join("about")).unwrap();
        fs::write(root.join("index.html"), "<html></html>").unwrap();
        fs::write(root.join("about/index.html"), "<html></html>").unwrap();
        fs::write(root.join("logo.svg"), "svg").unwrap();
        let site = model("", "directory");
        let inv = build_inventory(&root, &site).unwrap();
        assert!(inv.pages.contains_key("/"));
        assert!(inv.pages.contains_key("/about/"));
        assert_eq!(inv.pages.len(), 2);

        let base = Url::parse("https://e.com/").unwrap();
        assert_eq!(
            classify(&base.join("/about/").unwrap(), &site, &inv),
            LinkClass::Page("/about/".into())
        );
        assert_eq!(
            classify(&base.join("/about").unwrap(), &site, &inv),
            LinkClass::Page("/about/".into())
        );
        assert_eq!(
            classify(&base.join("/logo.svg").unwrap(), &site, &inv),
            LinkClass::Asset("logo.svg".into())
        );
        assert_eq!(
            classify(&base.join("/nope/").unwrap(), &site, &inv),
            LinkClass::Missing
        );
        assert_eq!(
            classify(&Url::parse("https://other.dev/x").unwrap(), &site, &inv),
            LinkClass::External("https://other.dev".into())
        );
        assert_eq!(
            classify(&Url::parse("mailto:a@b.c").unwrap(), &site, &inv),
            LinkClass::Special
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn classification_file_format() {
        let root = tmpdir("inv-file");
        fs::write(root.join("about.html"), "<html></html>").unwrap();
        fs::write(root.join("index.html"), "<html></html>").unwrap();
        let site = model("", "file");
        let inv = build_inventory(&root, &site).unwrap();
        assert!(inv.pages.contains_key("/about"));
        assert!(inv.pages.contains_key("/"));
        let base = Url::parse("https://e.com/").unwrap();
        assert_eq!(
            classify(&base.join("/about").unwrap(), &site, &inv),
            LinkClass::Page("/about".into())
        );
        assert_eq!(
            classify(&base.join("/about/").unwrap(), &site, &inv),
            LinkClass::Page("/about".into())
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn origin_alias_and_outside_base() {
        let root = tmpdir("inv-alias");
        fs::write(root.join("index.html"), "<html></html>").unwrap();
        let r: crate::request::ScanRequest = serde_json::from_value(serde_json::json!({
            "schemaVersion": "1",
            "artifactRoot": "/x",
            "site": "https://e.com",
            "originAliases": ["http://e.com", "https://www.e.com"],
            "basePath": "/docs"
        }))
        .unwrap();
        let site = SiteModel::from_request(&r).unwrap();
        let inv = build_inventory(&root, &site).unwrap();
        assert!(inv.pages.contains_key("/docs/"));
        let u = Url::parse("http://e.com/docs/").unwrap();
        assert_eq!(classify(&u, &site, &inv), LinkClass::Page("/docs/".into()));
        let outside = Url::parse("https://e.com/other/").unwrap();
        assert_eq!(classify(&outside, &site, &inv), LinkClass::OutsideScope);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn encoded_slash_never_collapses() {
        let root = tmpdir("inv-enc");
        fs::create_dir_all(root.join("a")).unwrap();
        fs::write(root.join("a/index.html"), "<html></html>").unwrap();
        let site = model("", "directory");
        let inv = build_inventory(&root, &site).unwrap();
        let base = Url::parse("https://e.com/").unwrap();
        // /a%2Fb/ is identity /a%2Fb/ which is NOT the page /a/
        let u = base.join("/a%2Fb/").unwrap();
        assert_eq!(classify(&u, &site, &inv), LinkClass::Missing);
        let u2 = base.join("/a/").unwrap();
        assert_eq!(classify(&u2, &site, &inv), LinkClass::Page("/a/".into()));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn percent_encoded_page_matches_raw_page() {
        let root = tmpdir("inv-uni");
        fs::create_dir_all(root.join("café")).unwrap();
        fs::write(root.join("café/index.html"), "<html></html>").unwrap();
        let site = model("", "directory");
        let inv = build_inventory(&root, &site).unwrap();
        assert!(inv.pages.contains_key("/caf%C3%A9/"));
        let base = Url::parse("https://e.com/").unwrap();
        // Astro emits /caf%C3%A9/ links; both raw and escaped resolve to one page
        let u = base.join("/caf%C3%A9/").unwrap();
        assert_eq!(
            classify(&u, &site, &inv),
            LinkClass::Page("/caf%C3%A9/".into())
        );
        let u2 = base.join("/café/").unwrap();
        assert_eq!(
            classify(&u2, &site, &inv),
            LinkClass::Page("/caf%C3%A9/".into())
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
