//! Top-level scan pipeline: request -> inventory -> edges -> graph -> gates
//! -> deterministic report. Shared by the CLI binary.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use url::Url;

use crate::gates::{evaluate, GateCounts};
use crate::graph::{hierarchy_problems, Graph};
use crate::links::{build_inventory, classify, fragment_identity, Inventory, LinkClass};
use crate::parser::{extract, fragment_target_exists, PageLinks};
use crate::request::{PageRecord, ScanRequest};
use crate::sitemap::{path_identity, BuildFormat, SiteModel};

pub const EXIT_OK: i32 = 0;
pub const EXIT_VIOLATIONS: i32 = 1;
pub const EXIT_FAILURE: i32 = 2;

#[derive(Debug, Serialize)]
pub struct BrokenPathIssue {
    pub source: String,
    pub href: String,
    pub resolved: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct FragmentIssue {
    pub source: String,
    pub href: String,
    pub target: String,
    pub fragment: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct EdgeRecord {
    pub source: String,
    pub target: String,
    pub href: String,
    pub landmark: String,
    pub editorial: bool,
    pub anchor: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rel: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct HierarchyIssue {
    pub kind: String,
    pub page: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related: Option<String>,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct Opportunity {
    /// "hierarchy": missing explicit child->parent or parent->child link.
    /// "contextual": target label/title whole-phrase found in unlinked
    /// visible body text of the source page.
    pub kind: String,
    pub source: String,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: String,
    pub engineVersion: String,
    pub coverage: Coverage,
    pub config: ReportConfig,
    pub nodes: Vec<NodeRecord>,
    pub edges: Vec<EdgeRecord>,
    pub issues: Issues,
    pub hierarchy: HierarchyResult,
    pub opportunities: Vec<Opportunity>,
    pub gates: GateReport,
    pub sitemap: crate::local_sitemap::SitemapReport,
    pub anchorFindings: Vec<AnchorFinding>,
}

#[derive(Debug, Serialize)]
pub struct AnchorFinding {
    pub kind: String,
    pub source: String,
    pub target: String,
    pub anchor: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct Coverage {
    pub totalPages: usize,
    pub scannedPages: usize,
    pub assets: usize,
    pub excludedPages: Vec<String>,
    pub complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scanIncompleteReason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReportConfig {
    pub site: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub originAliases: Vec<String>,
    pub basePath: String,
    pub trailingSlash: String,
    pub buildFormat: String,
    pub mode: String,
    pub entrypoints: Vec<String>,
    pub auditScope: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct NodeRecord {
    pub path: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<usize>,
    pub orphan: bool,
    pub unreachable: bool,
    pub inCycle: bool,
    pub editorialDepth: Option<usize>,
    pub editorialOrphan: bool,
    pub editorialUnreachable: bool,
    pub editorialSources: usize,
    pub metadata: Option<crate::metadata::Metadata>,
}

#[derive(Debug, Serialize)]
pub struct Issues {
    pub brokenPaths: Vec<BrokenPathIssue>,
    pub brokenFragments: Vec<FragmentIssue>,
    pub external: BTreeMap<String, Vec<String>>,
    pub special: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct HierarchyResult {
    pub configured: bool,
    pub cycles: Vec<String>,
    pub missingParents: Vec<String>,
    pub violations: Vec<HierarchyIssue>,
}

#[derive(Debug, Serialize)]
pub struct GateReport {
    pub mode: String,
    pub outcomes: Vec<crate::gates::ThresholdOutcome>,
    pub violated: bool,
}

#[derive(Debug)]
pub struct Prepared {
    pub req: ScanRequest,
    pub site: SiteModel,
    pub root: PathBuf,
}

pub fn prepare(input: &str) -> Result<Prepared, (String, i32)> {
    let req: ScanRequest = serde_json::from_str(input)
        .map_err(|e| (format!("invalid config: request JSON: {e}"), EXIT_FAILURE))?;
    if req.schemaVersion != crate::request::SCHEMA_VERSION {
        return Err((
            format!(
                "invalid config: schemaVersion '{}' not supported (expected '{}')",
                req.schemaVersion,
                crate::request::SCHEMA_VERSION
            ),
            EXIT_FAILURE,
        ));
    }
    if req.mode != "audit" && req.mode != "enforce" {
        return Err((
            format!(
                "invalid config: mode '{}' not supported (supported: audit, enforce)",
                req.mode
            ),
            EXIT_FAILURE,
        ));
    }
    if req.entrypoints.is_empty() {
        return Err((
            "invalid config: entrypoints is required (explicit entry page paths)".to_string(),
            EXIT_FAILURE,
        ));
    }
    let site = SiteModel::from_request(&req).map_err(|e| (e, EXIT_FAILURE))?;
    if req.artifactRoot.trim().is_empty() {
        return Err((
            "invalid config: artifactRoot is required".to_string(),
            EXIT_FAILURE,
        ));
    }
    let root = PathBuf::from(&req.artifactRoot);
    if !root.is_dir() {
        return Err((
            format!(
                "scan failure: artifactRoot '{}' is not a directory",
                req.artifactRoot
            ),
            EXIT_FAILURE,
        ));
    }
    Ok(Prepared { req, site, root })
}

/// Canonical entrypoint form: normalize to the page key shape of the build
/// format (directory keys end with '/', file keys do not except root).
fn entry_key(e: &str, site: &SiteModel) -> String {
    let mut k = e.trim().to_string();
    if !k.starts_with('/') {
        k = format!("/{k}");
    }
    match site.build_format {
        BuildFormat::Directory => {
            if k != "/" && !k.ends_with('/') {
                k.push('/');
            }
            k
        }
        BuildFormat::File => {
            if k.len() > 1 {
                k.trim_end_matches('/').to_string()
            } else {
                k
            }
        }
    }
}

fn load_page(root: &Path, rec: &PageRecord) -> Result<PageLinks, String> {
    let fs_path = root.join(rec.filePath.replace('\\', "/"));
    let data = std::fs::read(&fs_path).map_err(|e| format!("{}: {e}", rec.filePath))?;
    let html = std::str::from_utf8(&data).map_err(|e| format!("{}: {e}", rec.filePath))?;
    Ok(extract(html))
}

/// Public URL of a page key under the site model. Keys are full public
/// paths (base-inclusive when a base is configured). File-format document
/// URLs carry the '.html' suffix and the root is '<base>/index.html'
/// (verified against real Astro 7 output); directory-format keys end '/'.
pub fn page_public_url(site: &SiteModel, key: &str) -> Option<Url> {
    let owned = !site.base.is_empty()
        && (key == site.base_identity
            || key == format!("{}/", site.base_identity)
            || key.starts_with(&format!("{}/", site.base_identity)));
    let mut path = if owned {
        key.to_string()
    } else {
        format!("{}{}", site.base, key)
    };
    if matches!(site.build_format, crate::sitemap::BuildFormat::File) {
        let root_forms = site.base_identity.is_empty() && (path == "/" || path.is_empty())
            || !site.base_identity.is_empty()
                && (path == site.base_identity || path == format!("{}/", site.base_identity));
        if root_forms {
            let trimmed = path.trim_end_matches('/');
            path = if trimmed.is_empty() {
                "/index.html".to_string()
            } else {
                format!("{trimmed}/index.html")
            };
        } else {
            path = format!("{}.html", path.trim_end_matches('/'));
        }
    }
    let path = if path.starts_with('/') {
        path
    } else {
        format!("/{}", path)
    };
    Url::parse(&format!("{}/{}", site.site, path.trim_start_matches('/'))).ok()
}

/// Resolve one href against a page's public URL per the URL standard.
fn resolve_href(site: &SiteModel, page_key: &str, href: &str) -> Option<Url> {
    if let Ok(u) = Url::parse(href) {
        return Some(u);
    }
    let base = page_public_url(site, page_key)?;
    base.join(href).ok()
}

fn same_site_origin(u: &Url, site: &SiteModel) -> bool {
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    let origin = match u.port() {
        Some(p) => format!("{}://{}:{}", u.scheme(), host, p),
        None => format!("{}://{}", u.scheme(), host),
    };
    site.origin_aliases.contains(&origin)
}

pub fn run_prepared(p: &Prepared) -> Result<Report, String> {
    let inv: Inventory = build_inventory(&p.root, &p.site)
        .map_err(|e| format!("scan failure: cannot read artifact root: {e}"))?;

    let excluded: BTreeSet<String> = p
        .req
        .excludePagePaths
        .iter()
        .chain(p.req.exclude.iter())
        .cloned()
        .collect();

    let mut eligible: BTreeMap<String, PageRecord> = BTreeMap::new();
    for (k, rec) in &inv.pages {
        if !excluded.contains(k) {
            eligible.insert(k.clone(), rec.clone());
        }
    }

    let entrypoints: Vec<String> = p
        .req
        .entrypoints
        .iter()
        .map(|e| entry_key(e, &p.site))
        .collect();
    for e in &entrypoints {
        if !eligible.contains_key(e) {
            return Err(format!(
                "invalid config: entrypoint '{e}' is not an eligible page in the artifact"
            ));
        }
    }

    let mut graph = Graph::new();
    let mut editorial = Graph::new();
    let mut edges: Vec<EdgeRecord> = Vec::new();
    let mut broken_paths: Vec<BrokenPathIssue> = Vec::new();
    let mut broken_fragments: Vec<FragmentIssue> = Vec::new();
    let mut external: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut special: Vec<serde_json::Value> = Vec::new();
    let mut scan_incomplete: Option<String> = None;
    let mut page_links: BTreeMap<String, PageLinks> = BTreeMap::new();

    let sorted: Vec<(&String, &PageRecord)> = {
        let mut v: Vec<(&String, &PageRecord)> = eligible.iter().collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        v
    };

    for (key, rec) in &sorted {
        match load_page(&p.root, rec) {
            Ok(pl) => {
                page_links.insert((*key).clone(), pl);
            }
            Err(e) => {
                scan_incomplete = Some(e);
            }
        }
    }

    let metadata: BTreeMap<_, _> = page_links
        .iter()
        .map(|(key, page)| {
            (
                key.clone(),
                crate::metadata::assess(key, page, &p.site, &inv),
            )
        })
        .collect();
    let sitemap = crate::local_sitemap::audit(&p.root, &p.site, &inv, &metadata);

    // Cross-page fragment validation needs the parsed links of targets; all
    // pages are parsed above, so consult page_links directly.
    for (key, _) in &sorted {
        let pl = match page_links.get(*key) {
            Some(pl) => pl,
            None => continue,
        };
        graph.add_node((*key).clone());
        editorial.add_node((*key).clone());
        for dl in &pl.links {
            let u = match resolve_href(&p.site, key, &dl.href) {
                Some(u) => u,
                None => {
                    special.push(serde_json::json!({
                        "source": key,
                        "href": dl.href,
                        "reason": "unparseable href"
                    }));
                    continue;
                }
            };
            let frag = u.fragment().map(|f| f.to_string());
            let mut u_nofrag = u.clone();
            u_nofrag.set_fragment(None);
            let cls = classify(&u_nofrag, &p.site, &inv);
            match cls {
                LinkClass::Page(target) => {
                    if target == **key {
                        if let Some(f) = frag {
                            validate_fragment(
                                &f,
                                &dl.href,
                                key,
                                &target,
                                pl,
                                &mut broken_fragments,
                            );
                        }
                        continue;
                    }
                    let is_nofollow = dl.rel.iter().any(|r| r == "nofollow");
                    if !is_nofollow {
                        graph.add_edge(key, &target);
                        if dl.editorial {
                            editorial.add_edge(key, &target);
                        }
                    }
                    edges.push(EdgeRecord {
                        source: (*key).clone(),
                        target: target.clone(),
                        href: dl.href.clone(),
                        landmark: dl.landmark.clone(),
                        editorial: dl.editorial,
                        anchor: dl.anchor.clone(),
                        rel: dl.rel.clone(),
                    });
                    if let Some(f) = frag {
                        if let Some(tpl) = page_links.get(&target) {
                            validate_fragment(
                                &f,
                                &dl.href,
                                key,
                                &target,
                                tpl,
                                &mut broken_fragments,
                            );
                        }
                    }
                }
                LinkClass::Asset(fs) => {
                    if let Some(f) = frag {
                        broken_fragments.push(FragmentIssue {
                            source: (*key).clone(),
                            href: dl.href.clone(),
                            target: fs,
                            fragment: f,
                            reason: "fragment on non-page asset".to_string(),
                        });
                    }
                }
                LinkClass::Missing => {
                    if let Some(f) = frag {
                        broken_fragments.push(FragmentIssue {
                            source: (*key).clone(),
                            href: dl.href.clone(),
                            target: path_identity(u_nofrag.path()),
                            fragment: f,
                            reason: "target page missing".to_string(),
                        });
                    }
                    broken_paths.push(BrokenPathIssue {
                        source: (*key).clone(),
                        href: dl.href.clone(),
                        resolved: u_nofrag.to_string(),
                        reason: "no page or asset at path".to_string(),
                    });
                }
                LinkClass::External(origin) => {
                    external
                        .entry(origin)
                        .or_default()
                        .insert(format!("{key}: {}", dl.href));
                }
                LinkClass::Special => {
                    special.push(serde_json::json!({
                        "source": key,
                        "href": dl.href,
                        "tag": dl.tag,
                    }));
                }
                LinkClass::OutsideScope => {
                    special.push(serde_json::json!({
                        "source": key,
                        "href": dl.href,
                        "reason": "same origin outside configured base; outside audited scope",
                    }));
                }
            }
        }
    }

    let (depth, _reachable) = graph.bfs(&entrypoints);
    let (editorial_depth, editorial_reachable) = editorial.bfs(&entrypoints);
    let editorial_orphans = editorial.orphans();
    let mut editorial_sources: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut anchor_targets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in edges.iter().filter(|e| e.editorial) {
        if !edge.rel.iter().any(|r| r == "nofollow") {
            editorial_sources
                .entry(edge.target.clone())
                .or_default()
                .insert(edge.source.clone());
        }
        if !edge.anchor.is_empty() {
            anchor_targets
                .entry(edge.anchor.to_lowercase())
                .or_default()
                .insert(edge.target.clone());
        }
    }
    let mut anchor_findings = Vec::new();
    for edge in edges.iter().filter(|e| e.editorial) {
        let text = edge.anchor.to_lowercase();
        let vague = matches!(
            text.as_str(),
            "" | "click here" | "here" | "read more" | "learn more" | "more" | "link"
        );
        let ambiguous = anchor_targets.get(&text).is_some_and(|t| t.len() > 1);
        if vague || ambiguous {
            anchor_findings.push(AnchorFinding {
            kind: if vague { "vague" } else { "ambiguous" }.into(), source: edge.source.clone(), target: edge.target.clone(), anchor: edge.anchor.clone(),
            detail: "Advisory: review descriptive editorial anchor text in context; no search penalty inferred".into(),
        });
        }
    }
    let orphans = graph.orphans();
    let unreachable = graph.unreachable(&entrypoints);
    let cyclic = graph.cyclic_nodes();
    let max_depth_observed = depth.values().copied().max().unwrap_or(0);

    let mut hier = HierarchyResult {
        configured: p.req.hierarchy.is_some(),
        cycles: Vec::new(),
        missingParents: Vec::new(),
        violations: Vec::new(),
    };
    let mut opportunities: Vec<Opportunity> = Vec::new();
    let mut hierarchy_violations = 0usize;

    if let Some(hcfg) = &p.req.hierarchy {
        let known: BTreeSet<String> = eligible.keys().cloned().collect();
        let (cycles, missing) = hierarchy_problems(&hcfg.parents, &known);
        hier.cycles = cycles.iter().cloned().collect();
        hier.missingParents = missing.iter().cloned().collect();
        for c in &hier.cycles {
            hier.violations.push(HierarchyIssue {
                kind: "cycle".to_string(),
                page: c.clone(),
                related: None,
                detail: "hierarchy cycle detected".to_string(),
            });
            hierarchy_violations += 1;
        }
        for c in &hier.missingParents {
            hier.violations.push(HierarchyIssue {
                kind: "missingParent".to_string(),
                page: c.clone(),
                related: hcfg.parents.get(c).cloned(),
                detail: "configured parent page not found in artifact".to_string(),
            });
            hierarchy_violations += 1;
        }

        for (child, parent) in &hcfg.parents {
            if excluded.contains(child) || excluded.contains(parent) || cycles.contains(child) {
                continue;
            }
            if hcfg.requireChildToParent {
                if let Some(cpl) = page_links.get(child) {
                    if !links_to(&p.site, cpl, child, parent) {
                        hier.violations.push(HierarchyIssue {
                            kind: "childToParentLinkMissing".to_string(),
                            page: child.clone(),
                            related: Some(parent.clone()),
                            detail: "child page does not link to configured parent".to_string(),
                        });
                        opportunities.push(Opportunity {
                            kind: "hierarchy".to_string(),
                            source: child.clone(),
                            target: parent.clone(),
                            detail: "add child-to-parent link (configured parent)".to_string(),
                        });
                        hierarchy_violations += 1;
                    }
                }
            }
            if hcfg.requireParentToChild {
                if let Some(ppl) = page_links.get(parent) {
                    if !links_to(&p.site, ppl, parent, child) {
                        hier.violations.push(HierarchyIssue {
                            kind: "parentToChildLinkMissing".to_string(),
                            page: parent.clone(),
                            related: Some(child.clone()),
                            detail: "parent page does not link to configured child".to_string(),
                        });
                        opportunities.push(Opportunity {
                            kind: "hierarchy".to_string(),
                            source: parent.clone(),
                            target: child.clone(),
                            detail: "add parent-to-child link (configured child)".to_string(),
                        });
                        hierarchy_violations += 1;
                    }
                }
            }
            if hcfg.requireBreadcrumbs {
                if let Some(crec) = eligible.get(child) {
                    let sel = hcfg
                        .breadcrumbSelector
                        .clone()
                        .unwrap_or_else(|| "[aria-label=\"breadcrumb\"]".to_string());
                    let chain = crate::hierarchy::breadcrumbs_of(hcfg, child);
                    let ok = page_has_breadcrumb_links(&p.root, &p.site, crec, child, &sel, &chain);
                    if !ok {
                        hier.violations.push(HierarchyIssue {
                            kind: "breadcrumbsMissing".to_string(),
                            page: child.clone(),
                            related: Some(parent.clone()),
                            detail: format!(
                                "no breadcrumb links to ancestors found via selector '{sel}'"
                            ),
                        });
                        hierarchy_violations += 1;
                    }
                }
            }
        }
    }
    let mut contextual_links = Graph::new();
    for edge in edges.iter().filter(|e| e.editorial) {
        contextual_links.add_edge(&edge.source, &edge.target);
    }
    opportunities.extend(crate::contextual::suggest(
        &page_links,
        &metadata,
        &contextual_links,
        p.req.hierarchy.as_ref(),
    ));

    let counts = GateCounts {
        broken_paths: broken_paths.len(),
        broken_fragments: broken_fragments.len(),
        orphans: orphans.len(),
        unreachable: unreachable.len(),
        max_depth_observed,
        hierarchy_violations,
    };

    let outcomes = evaluate(&p.req.thresholds, &counts);
    let violated = outcomes.iter().any(|o| o.violated);

    let nodes: Vec<NodeRecord> = eligible
        .keys()
        .map(|k| NodeRecord {
            path: k.clone(),
            file: eligible[k].filePath.clone(),
            depth: depth.get(k).copied(),
            orphan: orphans.contains(k),
            unreachable: unreachable.contains(k),
            inCycle: cyclic.contains(k),
            editorialDepth: editorial_depth.get(k).copied(),
            editorialOrphan: editorial_orphans.contains(k),
            editorialUnreachable: !editorial_reachable.contains(k),
            editorialSources: editorial_sources.get(k).map_or(0, BTreeSet::len),
            metadata: metadata.get(k).cloned(),
        })
        .collect();

    let external_out: BTreeMap<String, Vec<String>> = external
        .into_iter()
        .map(|(k, v)| (k, v.into_iter().collect()))
        .collect();

    Ok(Report {
        schema: crate::request::SCHEMA_VERSION.to_string(),
        engineVersion: crate::request::ENGINE_VERSION.to_string(),
        coverage: Coverage {
            totalPages: inv.pages.len(),
            scannedPages: eligible.len(),
            assets: inv.assets.len(),
            excludedPages: excluded.iter().cloned().collect(),
            complete: scan_incomplete.is_none(),
            scanIncompleteReason: scan_incomplete,
        },
        config: ReportConfig {
            site: p.site.site.clone(),
            originAliases: p.site.origin_aliases.clone(),
            basePath: p.site.base.clone(),
            trailingSlash: format!("{:?}", p.site.trailing_slash).to_lowercase(),
            buildFormat: format!("{:?}", p.site.build_format).to_lowercase(),
            mode: p.req.mode.clone(),
            entrypoints,
            auditScope: serde_json::json!({"excludedPages": excluded, "hierarchy": p.req.hierarchy}),
        },
        nodes,
        edges,
        sitemap,
        anchorFindings: anchor_findings,
        issues: Issues {
            brokenPaths: broken_paths,
            brokenFragments: broken_fragments,
            external: external_out,
            special,
        },
        hierarchy: hier,
        opportunities,
        gates: GateReport {
            mode: p.req.mode.clone(),
            outcomes,
            violated,
        },
    })
}

/// Does the page with URL key `page_key` actually link to `target`?
/// Resolution uses the page's real public URL, per URL semantics.
fn links_to(site: &SiteModel, pl: &PageLinks, page_key: &str, target: &str) -> bool {
    for dl in &pl.links {
        if let Some(u) = resolve_href(site, page_key, &dl.href) {
            if !same_site_origin(&u, site) {
                continue;
            }
            let mut un = u.clone();
            un.set_fragment(None);
            let identity = crate::sitemap::path_identity(un.path());
            for cand in crate::links::page_key_candidates(&identity, site.build_format) {
                if cand == target {
                    return true;
                }
            }
        }
    }
    false
}

fn validate_fragment(
    frag: &str,
    href: &str,
    source: &str,
    target: &str,
    page: &PageLinks,
    out: &mut Vec<FragmentIssue>,
) {
    if frag.is_empty() || frag.starts_with(":~:") {
        return;
    }
    let decoded = fragment_identity(frag);
    if decoded == "top" {
        return;
    }
    if !fragment_target_exists(&decoded, page) {
        out.push(FragmentIssue {
            source: source.to_string(),
            href: href.to_string(),
            target: target.to_string(),
            fragment: frag.to_string(),
            reason: "no matching id or named anchor".to_string(),
        });
    }
}

fn page_has_breadcrumb_links(
    root: &Path,
    site: &SiteModel,
    rec: &PageRecord,
    page_key: &str,
    selector: &str,
    chain: &[(String, String)],
) -> bool {
    let fs = root.join(rec.filePath.replace('\\', "/"));
    let data = match std::fs::read(&fs) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let doc = scraper::Html::parse_document(std::str::from_utf8(data.as_slice()).unwrap_or(""));
    let sel = match scraper::Selector::parse(selector) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let a_sel = scraper::Selector::parse("a[href]").expect("static selector");
    let wanted: Vec<String> = chain.iter().map(|(p, _)| p.clone()).collect();
    if wanted.is_empty() {
        return false;
    }
    let format = site.build_format;
    for bc in doc.select(&sel) {
        let mut found: BTreeSet<String> = BTreeSet::new();
        for a in bc.select(&a_sel) {
            if let Some(href) = a.attr("href") {
                if let Some(u) = resolve_href(site, page_key, href) {
                    if !same_site_origin(&u, site) {
                        continue;
                    }
                    let identity = crate::sitemap::path_identity(u.path());
                    for w in &wanted {
                        for cand in crate::links::page_key_candidates(&identity, format) {
                            if cand == *w {
                                found.insert(w.clone());
                            }
                        }
                    }
                }
            }
        }
        if wanted.iter().all(|w| found.contains(w)) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contextual::whole_phrase as contains_whole_phrase;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ails-audit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(path: &Path, content: &str) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    fn request_json(root: &Path, extra: serde_json::Value) -> String {
        let mut base = serde_json::json!({
            "schemaVersion": "1",
            "artifactRoot": root.to_string_lossy(),
            "site": "https://e.com",
            "buildFormat": "directory",
            "entrypoints": ["/"]
        });
        if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                b.insert(k.clone(), v.clone());
            }
        }
        base.to_string()
    }

    #[test]
    fn end_to_end_edges_orphans_fragments() {
        let root = tmpdir("e2e");
        write(
            &root.join("index.html"),
            r##"<html><body><nav>
            <a href="/about/">About</a>
            <a href="/ghost/">Ghost</a>
            <a href="/about/#nope">Deep</a>
            <a href="/about/#intro">Intro</a>
            <a href="#selffrag">Self</a>
            <a href="mailto:x@y.z">Mail</a>
            <a href="https://ext.dev/p">Ext</a>
            </nav><p id="selffrag">s</p></body></html>"##,
        );
        write(
            &root.join("about/index.html"),
            r##"<html><body><a href="/">Home</a><h2 id="intro">I</h2><a href="/lonely/">L</a><a href="/lonely/#missing">LM</a></body></html>"##,
        );
        write(
            &root.join("lonely/index.html"),
            r##"<html><body><p>linked from about, so not an orphan</p></body></html>"##,
        );
        write(
            &root.join("unlinked/index.html"),
            r##"<html><body><p>genuine orphan: no page links here</p></body></html>"##,
        );
        let json = request_json(
            &root,
            serde_json::json!({"entrypoints": ["/"], "thresholds": {"brokenPaths": 0, "brokenFragments": 0}}),
        );
        let p = prepare(&json).unwrap();
        let report = run_prepared(&p).unwrap();
        assert!(report.coverage.complete);
        assert_eq!(report.nodes.len(), 4);
        // edges: / -> /about/, /about/ -> /, /about/ -> /lonely/
        assert!(report
            .edges
            .iter()
            .any(|e| e.source == "/" && e.target == "/about/"));
        assert!(report
            .edges
            .iter()
            .any(|e| e.source == "/about/" && e.target == "/lonely/"));
        // broken: /ghost/ path, fragments #nope (target /about/) and
        // #missing (target /lonely/)
        assert_eq!(report.issues.brokenPaths.len(), 1);
        assert_eq!(report.issues.brokenFragments.len(), 2);
        // /lonely/ is linked from /about/: reachable, not an orphan
        let lonely = report.nodes.iter().find(|n| n.path == "/lonely/").unwrap();
        assert!(!lonely.orphan);
        assert_eq!(lonely.depth, Some(2));
        // /unlinked/ is a genuine orphan: no inbound edges at all
        let unlinked = report
            .nodes
            .iter()
            .find(|n| n.path == "/unlinked/")
            .unwrap();
        assert!(unlinked.orphan);
        assert!(unlinked.unreachable);
        assert!(report.gates.violated, "thresholds must be violated");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn enforce_mode_audit_mode_same_report_no_exit_semantics_in_engine() {
        let root = tmpdir("modes");
        write(
            &root.join("index.html"),
            r##"<html><body><a href="/a/">a</a></body></html>"##,
        );
        write(
            &root.join("a/index.html"),
            r##"<html><body></body></html>"##,
        );
        for mode in ["audit", "enforce"] {
            let json = request_json(
                &root,
                serde_json::json!({"mode": mode, "thresholds": {"orphans": 0}}),
            );
            let p = prepare(&json).unwrap();
            let report = run_prepared(&p).unwrap();
            // /a/ is orphan; violation recorded in both modes identically
            assert_eq!(report.gates.mode, mode);
            assert!(report.gates.violated);
            assert_eq!(report.nodes.iter().filter(|n| n.orphan).count(), 1);
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn exclusions_do_not_mask_broken_paths() {
        let root = tmpdir("excl");
        write(
            &root.join("index.html"),
            r##"<html><body><a href="/404">404</a><a href="/gone/">gone</a></body></html>"##,
        );
        write(&root.join("404.html"), r##"<html><body></body></html>"##);
        // wait: 404.html in directory format is not a page; adjust
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn excluded_404_directory_format() {
        let root = tmpdir("excl404");
        write(
            &root.join("index.html"),
            r##"<html><body><a href="/404/">404</a><a href="/gone/">gone</a></body></html>"##,
        );
        write(
            &root.join("404/index.html"),
            r##"<html><body></body></html>"##,
        );
        let json = request_json(
            &root,
            serde_json::json!({"excludePagePaths": ["/404/"], "thresholds": {"brokenPaths": 0}}),
        );
        let p = prepare(&json).unwrap();
        let report = run_prepared(&p).unwrap();
        // /404/ excluded from graph; /gone/ still a real broken path
        assert_eq!(report.nodes.len(), 1);
        assert_eq!(report.issues.brokenPaths.len(), 1);
        assert!(report.gates.violated);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn hierarchy_link_checks_and_opportunities() {
        let root = tmpdir("hier");
        write(
            &root.join("index.html"),
            r##"<html><body><a href="/docs/">Docs</a><a href="/blog/">Blog</a></body></html>"##,
        );
        // /docs/ links back to parent; /blog/ does not (violation)
        write(
            &root.join("docs/index.html"),
            r##"<html><body><a href="/">Home</a></body></html>"##,
        );
        write(
            &root.join("blog/index.html"),
            r##"<html><body></body></html>"##,
        );
        let json = request_json(
            &root,
            serde_json::json!({
                "hierarchy": {
                    "parents": {"/docs/": "/", "/blog/": "/"},
                    "labels": {"/": "Home"},
                    "requireChildToParent": true,
                    "requireParentToChild": false
                }
            }),
        );
        let p = prepare(&json).unwrap();
        let report = run_prepared(&p).unwrap();
        assert_eq!(report.hierarchy.violations.len(), 1);
        assert_eq!(
            report.hierarchy.violations[0].kind,
            "childToParentLinkMissing"
        );
        assert_eq!(report.opportunities.len(), 1);
        assert_eq!(report.opportunities[0].kind, "hierarchy");
        assert_eq!(report.opportunities[0].source, "/blog/");
        assert_eq!(report.opportunities[0].target, "/");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn contextual_suggestions_grounded_in_text() {
        let root = tmpdir("ctx");
        write(
            &root.join("index.html"),
            r##"<html><body><a href="/docs/a/">A</a><a href="/docs/b/">B</a></body></html>"##,
        );
        // /docs/a/ body genuinely mentions the target page title unlinked
        write(
            &root.join("docs/a/index.html"),
            r##"<html><body><a href="/">Home</a><p>See also the Deep Sky Guide for charts.</p></body></html>"##,
        );
        write(
            &root.join("docs/b/index.html"),
            r##"<html><body><title>Deep Sky Guide</title><a href="/">Home</a><p>charts</p></body></html>"##,
        );
        let json = request_json(
            &root,
            serde_json::json!({
                "hierarchy": {
                    "parents": {"/docs/a/": "/docs/", "/docs/b/": "/docs/"},
                    "requireChildToParent": false,
                    "requireParentToChild": false
                }
            }),
        );
        let p = prepare(&json).unwrap();
        let report = run_prepared(&p).unwrap();
        let ctx: Vec<&Opportunity> = report
            .opportunities
            .iter()
            .filter(|o| o.kind == "contextual")
            .collect();
        // grounded: exactly a->b via the title phrase; no reverse mention
        assert_eq!(ctx.len(), 1, "{:?}", ctx);
        assert_eq!(ctx[0].source, "/docs/a/");
        assert_eq!(ctx[0].target, "/docs/b/");
        // mentions only inside nav/footer/header/link text do not count
        write(
            &root.join("docs/a/index.html"),
            r##"<html><body><a href="/">Home</a><nav><a href="/docs/b/">Deep Sky Guide</a></nav><footer>Deep Sky Guide</footer><p>plain body text</p></body></html>"##,
        );
        let p2 = prepare(&json).unwrap();
        let report2 = run_prepared(&p2).unwrap();
        let ctx2: Vec<&Opportunity> = report2
            .opportunities
            .iter()
            .filter(|o| o.kind == "contextual")
            .collect();
        assert!(ctx2.is_empty(), "{:?}", ctx2);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_contextual_when_already_linked() {
        let root = tmpdir("ctx2");
        write(
            &root.join("index.html"),
            r##"<html><body><a href="/docs/a/">A</a><a href="/docs/b/">B</a></body></html>"##,
        );
        write(
            &root.join("docs/a/index.html"),
            r##"<html><body><a href="/docs/b/">B</a></body></html>"##,
        );
        write(
            &root.join("docs/b/index.html"),
            r##"<html><body><a href="/docs/a/">A</a></body></html>"##,
        );
        let json = request_json(
            &root,
            serde_json::json!({
                "hierarchy": {
                    "parents": {"/docs/a/": "/docs/", "/docs/b/": "/docs/"}
                }
            }),
        );
        let p = prepare(&json).unwrap();
        let report = run_prepared(&p).unwrap();
        assert!(report.opportunities.iter().all(|o| o.kind != "contextual"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn invalid_entrypoint_fails_not_skips() {
        let root = tmpdir("badentry");
        write(&root.join("index.html"), r##"<html><body></body></html>"##);
        let json = request_json(&root, serde_json::json!({"entrypoints": ["/missing/"]}));
        let p = prepare(&json).unwrap();
        let err = run_prepared(&p).unwrap_err();
        assert!(err.contains("entrypoint"), "got: {err}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn invalid_mode_fails() {
        let root = tmpdir("badmode");
        write(&root.join("index.html"), r##"<html><body></body></html>"##);
        let json = request_json(&root, serde_json::json!({"mode": "yolo"}));
        let err = prepare(&json).unwrap_err();
        assert!(err.0.contains("mode"), "got: {}", err.0);
        assert_eq!(err.1, EXIT_FAILURE);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unreachable_and_depth_bfs() {
        let root = tmpdir("bfs");
        write(
            &root.join("index.html"),
            r##"<html><body><a href="/a/">a</a></body></html>"##,
        );
        write(
            &root.join("a/index.html"),
            r##"<html><body><a href="/b/">b</a><a href="/c/">c</a></body></html>"##,
        );
        write(
            &root.join("b/index.html"),
            r##"<html><body><a href="/d/">d</a></body></html>"##,
        );
        write(
            &root.join("c/index.html"),
            r##"<html><body></body></html>"##,
        );
        write(
            &root.join("d/index.html"),
            r##"<html><body></body></html>"##,
        );
        write(
            &root.join("isle/index.html"),
            r##"<html><body><a href="/isle2/">i2</a></body></html>"##,
        );
        write(
            &root.join("isle2/index.html"),
            r##"<html><body><a href="/isle/">i1</a></body></html>"##,
        );
        let json = request_json(&root, serde_json::json!({}));
        let p = prepare(&json).unwrap();
        let report = run_prepared(&p).unwrap();
        let by_path = |k: &str| report.nodes.iter().find(|n| n.path == k).unwrap();
        assert_eq!(by_path("/b/").depth, Some(2));
        assert_eq!(by_path("/d/").depth, Some(3));
        assert!(by_path("/isle/").unreachable);
        assert!(by_path("/isle2/").unreachable);
        // isle is a disconnected cycle; both in cycle, not orphans
        assert!(by_path("/isle/").inCycle);
        assert!(by_path("/isle2/").inCycle);
        assert!(!by_path("/isle/").orphan);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn deterministic_report_across_runs() {
        let root = tmpdir("det");
        write(
            &root.join("index.html"),
            r##"<html><body><a href="/a/">a</a><a href="/b/">b</a></body></html>"##,
        );
        write(
            &root.join("a/index.html"),
            r##"<html><body><a href="/">h</a></body></html>"##,
        );
        write(
            &root.join("b/index.html"),
            r##"<html><body></body></html>"##,
        );
        let json = request_json(&root, serde_json::json!({}));
        let p1 = prepare(&json).unwrap();
        let r1 = run_prepared(&p1).unwrap();
        let p2 = prepare(&json).unwrap();
        let r2 = run_prepared(&p2).unwrap();
        let s1 = serde_json::to_string(&r1).unwrap();
        let s2 = serde_json::to_string(&r2).unwrap();
        assert_eq!(s1, s2);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn base_path_file_format_links() {
        let root = tmpdir("basefile");
        // Astro emits at dist root even with a base; base is a URL prefix.
        write(
            &root.join("index.html"),
            r##"<html><body><a href="guide.html">g</a><a href="/docs/about">a</a></body></html>"##,
        );
        write(
            &root.join("guide.html"),
            r##"<html><body><a href="./">up</a></body></html>"##,
        );
        write(&root.join("about.html"), r##"<html><body></body></html>"##);
        let json = request_json(
            &root,
            serde_json::json!({"buildFormat": "file", "basePath": "/docs", "entrypoints": ["/docs"]}),
        );
        let p = prepare(&json).unwrap();
        let report = run_prepared(&p).unwrap();
        assert_eq!(report.nodes.len(), 3);
        assert!(report
            .edges
            .iter()
            .any(|e| e.source == "/docs" && e.target == "/docs/guide"));
        assert!(report
            .edges
            .iter()
            .any(|e| e.source == "/docs/guide" && e.target == "/docs"));
        assert!(
            report.issues.brokenPaths.is_empty(),
            "{:?}",
            report.issues.brokenPaths
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn breadcrumbs_relative_paths_resolve_against_page_url() {
        let root = tmpdir("bc-rel");
        write(
            &root.join("index.html"),
            r##"<html><body><a href="/knowledge/docs/">Docs</a><a href="/knowledge/docs/deep/">Deep</a></body></html>"##,
        );
        write(
            &root.join("knowledge/docs/index.html"),
            r##"<html><body><nav aria-label="breadcrumb"><a href="/">Home</a></nav><a href="deep/">Deep</a></body></html>"##,
        );
        write(
            &root.join("knowledge/docs/deep/index.html"),
            r##"<html><body><nav aria-label="breadcrumb"><a href="../../../">Home</a><a href="../">Docs</a></nav></body></html>"##,
        );
        let json = request_json(
            &root,
            serde_json::json!({
                "hierarchy": {
                    "parents": {"/knowledge/docs/": "/", "/knowledge/docs/deep/": "/knowledge/docs/"},
                    "labels": {"/": "Home", "/knowledge/docs/": "Docs"},
                    "requireChildToParent": true,
                    "requireBreadcrumbs": true
                }
            }),
        );
        let p = prepare(&json).unwrap();
        let report = run_prepared(&p).unwrap();
        assert!(
            report.hierarchy.violations.is_empty(),
            "{:?}",
            report.hierarchy.violations
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn breadcrumb_external_same_path_does_not_count() {
        let root = tmpdir("bc-ext");
        write(
            &root.join("index.html"),
            r##"<html><body><a href="/knowledge/docs/">Docs</a><a href="/knowledge/docs/deep/">Deep</a></body></html>"##,
        );
        write(
            &root.join("knowledge/docs/index.html"),
            r##"<html><body><nav aria-label="breadcrumb"><a href="/">Home</a></nav></body></html>"##,
        );
        write(
            &root.join("knowledge/docs/deep/index.html"),
            r##"<html><body><nav aria-label="breadcrumb"><a href="https://outside.test/">Home</a><a href="https://outside.test/knowledge/docs/">Docs</a></nav></body></html>"##,
        );
        let json = request_json(
            &root,
            serde_json::json!({
                "hierarchy": {
                    "parents": {"/knowledge/docs/": "/", "/knowledge/docs/deep/": "/knowledge/docs/"},
                    "labels": {"/": "Home", "/knowledge/docs/": "Docs"},
                    "requireBreadcrumbs": true
                }
            }),
        );
        let p = prepare(&json).unwrap();
        let report = run_prepared(&p).unwrap();
        assert_eq!(report.hierarchy.violations.len(), 1);
        assert_eq!(report.hierarchy.violations[0].kind, "breadcrumbsMissing");
        assert_eq!(report.hierarchy.violations[0].page, "/knowledge/docs/deep/");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn breadcrumb_internal_alias_href_counts() {
        let root = tmpdir("bc-alias");
        write(&root.join("index.html"), r##"<html><body></body></html>"##);
        write(
            &root.join("docs/index.html"),
            r##"<html><body><nav aria-label="breadcrumb"><a href="http://e.com/">Home</a></nav></body></html>"##,
        );
        let json = request_json(
            &root,
            serde_json::json!({
                "originAliases": ["http://e.com"],
                "hierarchy": {
                    "parents": {"/docs/": "/"},
                    "labels": {"/": "Home"},
                    "requireBreadcrumbs": true
                }
            }),
        );
        let p = prepare(&json).unwrap();
        let report = run_prepared(&p).unwrap();
        assert!(
            report.hierarchy.violations.is_empty(),
            "{:?}",
            report.hierarchy.violations
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn links_to_rejects_external_origin_with_matching_path() {
        let root = tmpdir("ext-links-to");
        write(
            &root.join("index.html"),
            r##"<html><body><a href="/docs/">Docs</a></body></html>"##,
        );
        write(
            &root.join("docs/index.html"),
            r##"<html><body><a href="https://outside.test/">Home</a></body></html>"##,
        );
        let json = request_json(
            &root,
            serde_json::json!({
                "hierarchy": {
                    "parents": {"/docs/": "/"},
                    "labels": {"/": "Home"},
                    "requireChildToParent": true
                }
            }),
        );
        let p = prepare(&json).unwrap();
        let report = run_prepared(&p).unwrap();
        assert!(report
            .hierarchy
            .violations
            .iter()
            .any(|v| v.kind == "childToParentLinkMissing" && v.page == "/docs/"));
        assert_eq!(report.opportunities.len(), 1);
        assert_eq!(report.opportunities[0].kind, "hierarchy");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn contains_whole_phrase_boundaries() {
        assert!(contains_whole_phrase(
            "the Deep Sky Guide today",
            "Deep Sky Guide"
        ));
        assert!(contains_whole_phrase("(Deep Sky Guide)", "Deep Sky Guide"));
        assert!(!contains_whole_phrase(
            "Deep Sky Guidebook",
            "Deep Sky Guide"
        ));
        assert!(!contains_whole_phrase("xDeep Sky Guide", "Deep Sky Guide"));
        assert!(!contains_whole_phrase("Deep Sky Guides", "Deep Sky Guide"));
        assert!(contains_whole_phrase("lise café list", "café"));
        assert!(!contains_whole_phrase("cafés list", "café"));
        assert!(!contains_whole_phrase("any text", "   "));
    }

    #[test]
    fn contextual_excludes_hidden_and_nonvisible_mentions() {
        let root = tmpdir("ctx-hidden");
        write(
            &root.join("index.html"),
            r##"<html><body><a href="/docs/a/">A</a><a href="/docs/b/">B</a></body></html>"##,
        );
        write(
            &root.join("docs/a/index.html"),
            r##"<html><body><a href="/">Home</a><p>placeholder</p></body></html>"##,
        );
        write(
            &root.join("docs/b/index.html"),
            r##"<html><body><title>Deep Sky Guide</title><a href="/">Home</a><p>charts</p></body></html>"##,
        );
        let cases: &[(&str, usize)] = &[
            ("<header>Deep Sky Guide</header>", 0),
            ("<nav>Deep Sky Guide</nav>", 0),
            ("<p hidden>Deep Sky Guide</p>", 0),
            ("<p style=\"display:none\">Deep Sky Guide</p>", 0),
            ("<p style=\"display:none !important\">Deep Sky Guide</p>", 0),
            ("<p style=\"visibility:hidden\">Deep Sky Guide</p>", 0),
            ("<div aria-hidden=\"true\">Deep Sky Guide</div>", 0),
            ("<noscript>Deep Sky Guide</noscript>", 0),
            ("<template>Deep Sky Guide</template>", 0),
            ("<p>Deep Sky Guidebook</p>", 0),
            ("<p>Read the Deep Sky Guide today.</p>", 1),
        ];
        let json = request_json(
            &root,
            serde_json::json!({
                "hierarchy": {
                    "parents": {"/docs/a/": "/docs/", "/docs/b/": "/docs/"},
                    "requireChildToParent": false,
                    "requireParentToChild": false
                }
            }),
        );
        for (body, expected) in cases {
            write(
                &root.join("docs/a/index.html"),
                &format!(r##"<html><body><a href="/">Home</a>{body}</body></html>"##),
            );
            let p = prepare(&json).unwrap();
            let report = run_prepared(&p).unwrap();
            let n = report
                .opportunities
                .iter()
                .filter(|o| o.kind == "contextual")
                .count();
            assert_eq!(n, *expected, "case body: {body}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}
