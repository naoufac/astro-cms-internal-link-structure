use crate::{
    links::{classify, Inventory, LinkClass},
    metadata::Metadata,
    sitemap::SiteModel,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
};
use url::Url;

#[derive(Debug, Serialize)]
pub struct SitemapIssue {
    pub kind: String,
    pub url: String,
    pub detail: String,
}
#[derive(Debug, Serialize)]
pub struct SitemapReport {
    pub files: Vec<String>,
    pub urls: usize,
    pub issues: Vec<SitemapIssue>,
    pub basis: &'static str,
}

pub fn audit(
    root: &Path,
    site: &SiteModel,
    inv: &Inventory,
    metadata: &BTreeMap<String, Metadata>,
) -> SitemapReport {
    let assets: BTreeSet<String> = inv.assets.values().cloned().collect();
    let mut queue: VecDeque<String> = assets
        .iter()
        .filter(|f| {
            f.rsplit('/')
                .next()
                .is_some_and(|n| n.starts_with("sitemap") && n.ends_with(".xml"))
        })
        .cloned()
        .collect();
    let mut seen = BTreeSet::new();
    let mut urls = BTreeMap::<String, usize>::new();
    let mut pages = BTreeSet::new();
    let mut issues = Vec::new();
    let mut issue = |kind: &str, url: String, detail: &str| {
        issues.push(SitemapIssue {
            kind: kind.into(),
            url,
            detail: detail.into(),
        })
    };
    let mut parsed_any = false;
    let canonical_root = root.canonicalize().ok();
    while let Some(file) = queue.pop_front() {
        if !seen.insert(file.clone()) {
            continue;
        }
        let path = root.join(&file);
        if !path
            .canonicalize()
            .ok()
            .zip(canonical_root.as_ref())
            .is_some_and(|(p, r)| p.starts_with(r))
        {
            issue("unsafeChild", file, "Local sitemap is outside artifactRoot");
            continue;
        }
        let xml = match std::fs::read_to_string(path) {
            Ok(x) => x,
            Err(_) => {
                issue("unavailable", file, "Cannot read local sitemap");
                continue;
            }
        };
        let doc = match roxmltree::Document::parse(&xml) {
            Ok(d) => d,
            Err(_) => {
                issue("malformed", file, "Invalid sitemap XML");
                continue;
            }
        };
        let kind = doc.root_element().tag_name().name();
        if !matches!(kind, "urlset" | "sitemapindex") {
            issue("malformed", file, "Expected urlset or sitemapindex");
            continue;
        }
        parsed_any = true;

        for entry in doc.root_element().children().filter(|n| n.is_element()) {
            let expected = if kind == "urlset" { "url" } else { "sitemap" };
            if entry.tag_name().name() != expected {
                continue;
            }
            let locs: Vec<_> = entry
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "loc")
                .collect();
            if locs.len() != 1 {
                issue(
                    "malformed",
                    file.clone(),
                    "Each entry requires exactly one loc",
                );
                continue;
            }
            let loc = locs[0].text().unwrap_or("").trim();
            let u = match Url::parse(loc) {
                Ok(u)
                    if matches!(u.scheme(), "http" | "https")
                        && crate::metadata::valid_encoding(loc)
                        && !loc.chars().any(char::is_control)
                        && u.username().is_empty()
                        && u.password().is_none()
                        && u.fragment().is_none() =>
                {
                    u
                }
                _ => {
                    issue(
                        "malformedUrl",
                        loc.into(),
                        "Sitemap loc must be an absolute HTTP(S) URL",
                    );
                    continue;
                }
            };
            if kind == "sitemapindex" {
                let origin = u.origin().ascii_serialization();
                let encoded_rel = u
                    .path()
                    .strip_prefix(&format!("{}/", site.base))
                    .unwrap_or("");
                let decoded_rel =
                    percent_encoding::percent_decode_str(encoded_rel).decode_utf8_lossy();
                let rel = decoded_rel.as_ref();
                if !site.origin_aliases.contains(&origin)
                    || u.query().is_some()
                    || !assets.contains(rel)
                {
                    issue(
                        "unavailableChild",
                        u.to_string(),
                        "Child is not an inventoried local artifact; no fetch attempted",
                    );
                } else {
                    queue.push_back(rel.into());
                }
                continue;
            }

            *urls.entry(u.to_string()).or_default() += 1;
            match classify(&u, site, inv) {
                LinkClass::Page(key) => {
                    if !pages.insert(key.clone()) {
                        issue(
                            "duplicatePage",
                            u.to_string(),
                            "Multiple sitemap entries identify the same artifact",
                        );
                    }
                    if let Some(m) = metadata.get(&key) {
                        if m.noindex {
                            issue(
                                "noindex",
                                u.to_string(),
                                "Sitemap URL declares noindex in local HTML",
                            );
                        }
                        if !matches!(m.canonicalStatus.as_str(), "absent" | "self")
                            || u.query().is_some()
                            || m.canonicalResolved
                                .as_ref()
                                .is_some_and(|c| c != u.as_str())
                        {
                            issue(
                                "noncanonical",
                                u.to_string(),
                                "Sitemap URL conflicts with local canonical signals",
                            );
                        }
                    }
                }
                LinkClass::External(_) | LinkClass::OutsideScope => issue(
                    "outsideScope",
                    u.to_string(),
                    "Sitemap URL outside configured site scope",
                ),
                _ => issue(
                    "missingArtifact",
                    u.to_string(),
                    "No local page artifact; HTTP status is unknown",
                ),
            }
        }
    }
    for (u, count) in &urls {
        if *count > 1 {
            issue("duplicateUrl", u.clone(), &format!("{count} occurrences"));
        }
    }
    if !seen.is_empty() && parsed_any {
        for (key, m) in metadata {
            if m.eligible && !pages.contains(key) {
                issue(
                    "notListed",
                    key.clone(),
                    "Eligible artifact absent from discovered local sitemaps",
                );
            }
        }
    }
    SitemapReport {
        files: seen.into_iter().collect(),
        urls: urls.values().sum(),
        issues,
        basis: "Offline local sitemap consistency only; no indexation or HTTP status verified",
    }
}
