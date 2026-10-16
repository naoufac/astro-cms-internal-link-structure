//! Hierarchy configuration, validation and data-only helpers.

use std::collections::BTreeMap;

fn default_label() -> String {
    String::new()
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct HierarchyConfig {
    /// Explicit child page path -> parent page path. Authoritative.
    pub parents: BTreeMap<String, String>,
    /// Labels per page path, supplied by the consumer. Never invented.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// Require an actual child->parent link in child HTML.
    #[serde(default)]
    pub requireChildToParent: bool,
    /// Require an actual parent->child link in parent HTML.
    #[serde(default)]
    pub requireParentToChild: bool,
    /// Require a breadcrumb trail (ancestor links) in child HTML.
    #[serde(default)]
    pub requireBreadcrumbs: bool,
    /// Explicit semantic selector that must contain the breadcrumb links.
    #[serde(default)]
    pub breadcrumbSelector: Option<String>,
    /// Label used by breadcrumbsOf for the site root when supplied.
    #[serde(default = "default_label")]
    pub rootLabel: String,
}

/// Pure data helper: parent of a page (path + supplied label).
pub fn parent_of<'a>(cfg: &'a HierarchyConfig, child: &str) -> Option<(&'a str, &'a str)> {
    let parent = cfg.parents.get(child)?;
    let label = cfg.labels.get(parent).map(|s| s.as_str()).unwrap_or("");
    Some((parent.as_str(), label))
}

/// Pure data helper: children of a page (paths + supplied labels), sorted.
pub fn children_of<'a>(cfg: &'a HierarchyConfig, parent: &str) -> Vec<(&'a str, &'a str)> {
    let mut out: Vec<(&str, &str)> = cfg
        .parents
        .iter()
        .filter(|(_, p)| p.as_str() == parent)
        .map(|(c, _)| {
            let label = cfg.labels.get(c).map(|s| s.as_str()).unwrap_or("");
            (c.as_str(), label)
        })
        .collect();
    out.sort();
    out
}

/// Pure data helper: breadcrumb chain from root to the page (exclusive of
/// the page itself), each entry (path, supplied label).
pub fn breadcrumbs_of(cfg: &HierarchyConfig, page: &str) -> Vec<(String, String)> {
    let mut chain: Vec<(String, String)> = Vec::new();
    let mut visited: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut cur = Some(page.to_string());
    while let Some(c) = cur {
        if !visited.insert(c.clone()) {
            break;
        }
        match cfg.parents.get(&c) {
            Some(p) => {
                let label = cfg.labels.get(p).cloned().unwrap_or_default();
                chain.push((p.clone(), label));
                cur = Some(p.clone());
            }
            None => break,
        }
    }
    chain.reverse();
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> HierarchyConfig {
        serde_json::from_value(serde_json::json!({
            "parents": {
                "/docs/": "/",
                "/docs/api/": "/docs/",
                "/blog/": "/"
            },
            "labels": {
                "/": "Home",
                "/docs/": "Docs"
            }
        }))
        .unwrap()
    }

    #[test]
    fn parent_of_returns_supplied_labels_only() {
        let c = cfg();
        assert_eq!(parent_of(&c, "/docs/api/"), Some(("/docs/", "Docs")));
        assert_eq!(parent_of(&c, "/blog/"), Some(("/", "Home")));
        assert_eq!(parent_of(&c, "/"), None);
    }

    #[test]
    fn children_of_sorted() {
        let c = cfg();
        let kids = children_of(&c, "/");
        assert_eq!(kids, vec![("/blog/", ""), ("/docs/", "Docs")]);
    }

    #[test]
    fn breadcrumbs_root_to_page() {
        let c = cfg();
        let bc = breadcrumbs_of(&c, "/docs/api/");
        assert_eq!(
            bc,
            vec![
                ("/".to_string(), "Home".to_string()),
                ("/docs/".to_string(), "Docs".to_string())
            ]
        );
    }

    #[test]
    fn cycle_does_not_hang_breadcrumbs() {
        let c: HierarchyConfig = serde_json::from_value(serde_json::json!({
            "parents": { "/a/": "/b/", "/b/": "/a/" }
        }))
        .unwrap();
        let bc = breadcrumbs_of(&c, "/a/");
        assert!(bc.len() <= 2);
    }

    #[test]
    fn unknown_fields_rejected() {
        let r: Result<HierarchyConfig, _> = serde_json::from_value(serde_json::json!({
            "parents": {}, "bogus": true
        }));
        assert!(r.is_err());
    }
}
