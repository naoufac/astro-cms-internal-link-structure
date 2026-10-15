//! Extraction of navigation edges (a[href], area[href]) and id inventory.

use std::collections::BTreeMap;

use scraper::{ElementRef, Html, Selector};

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct DomLink {
    pub tag: String,
    pub href: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rel: Vec<String>,
    pub ordinal: usize,
    pub landmark: String,
}

pub struct PageLinks {
    pub links: Vec<DomLink>,
    pub ids: BTreeMap<String, usize>,
    pub named_anchors: BTreeMap<String, usize>,
}

fn nearest_landmark(el: &ElementRef) -> String {
    let mut cur = el.parent();
    while let Some(node) = cur {
        if let Some(er) = ElementRef::wrap(node) {
            let name = er.value().name();
            if matches!(name, "nav" | "header" | "footer" | "main" | "aside") {
                return name.to_string();
            }
        }
        cur = node.parent();
    }
    "body".to_string()
}

pub fn extract(html: &str) -> PageLinks {
    let doc = Html::parse_document(html);
    let mut links = Vec::new();
    let mut ids: BTreeMap<String, usize> = BTreeMap::new();
    let mut named: BTreeMap<String, usize> = BTreeMap::new();
    let a_sel = Selector::parse("a[href], area[href]").expect("static selector");
    let id_sel = Selector::parse("[id]").expect("static selector");
    let name_sel = Selector::parse("a[name]").expect("static selector");
    for (ordinal, el) in doc.select(&a_sel).enumerate() {
        let tag = el.value().name().to_string();
        let href = el.attr("href").unwrap_or("").to_string();
        let rel: Vec<String> = el
            .attr("rel")
            .unwrap_or("")
            .split_whitespace()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        let landmark = nearest_landmark(&el);
        links.push(DomLink {
            tag,
            href,
            rel,
            ordinal,
            landmark,
        });
    }
    for el in doc.select(&id_sel) {
        if let Some(id) = el.attr("id") {
            *ids.entry(id.to_string()).or_insert(0) += 1;
        }
    }
    for el in doc.select(&name_sel) {
        if let Some(n) = el.attr("name") {
            *named.entry(n.to_string()).or_insert(0) += 1;
        }
    }
    PageLinks {
        links,
        ids,
        named_anchors: named,
    }
}

pub fn fragment_target_exists(frag_decoded: &str, page: &PageLinks) -> bool {
    page.ids.contains_key(frag_decoded) || page.named_anchors.contains_key(frag_decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_links_with_landmarks() {
        let html = r##"<html><body>
        <nav><a href="/a/">A</a></nav>
        <main><a href="b">B</a><a href="https://x.dev/e" rel="nofollow external">E</a></main>
        <a href="#top">T</a>
        </body></html>"##;
        let p = extract(html);
        assert_eq!(p.links.len(), 4);
        assert_eq!(p.links[0].landmark, "nav");
        assert_eq!(p.links[0].tag, "a");
        assert_eq!(p.links[1].landmark, "main");
        assert_eq!(p.links[2].rel, vec!["nofollow", "external"]);
        assert_eq!(p.links[2].landmark, "main");
        assert_eq!(p.links[3].href, "#top");
        assert_eq!(p.links[3].landmark, "body");
    }

    #[test]
    fn ids_and_named_anchors() {
        let html = r##"<html><body>
        <h2 id="intro">Intro</h2><p id="intro">dup</p>
        <a name="legacy"></a><a id="modern">m</a>
        </body></html>"##;
        let p = extract(html);
        assert_eq!(p.ids.get("intro"), Some(&2));
        assert_eq!(p.ids.get("modern"), Some(&1));
        assert!(p.named_anchors.contains_key("legacy"));
        assert!(fragment_target_exists("legacy", &p));
        assert!(fragment_target_exists("modern", &p));
        assert!(!fragment_target_exists("missing", &p));
    }

    #[test]
    fn area_links_collected() {
        let html = r##"<html><body><map name="m"><area href="/t/" alt="t"></map></body></html>"##;
        let p = extract(html);
        assert_eq!(p.links.len(), 1);
        assert_eq!(p.links[0].tag, "area");
    }
}
