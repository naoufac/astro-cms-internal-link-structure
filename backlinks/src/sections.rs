use crate::{
    detector::heading_is_resource_style,
    neturl::{safe_parse, SafeUrl},
    text::{element_text, has_excluded_ancestor, heading_level, is_in_excluded},
};
use scraper::{ElementRef, Html};
use std::collections::HashSet;

pub(crate) fn resource_group(html: &str, source: &SafeUrl) -> Option<(String, usize)> {
    let doc = Html::parse_document(html);
    let elements: Vec<_> = doc
        .root_element()
        .descendants()
        .filter_map(ElementRef::wrap)
        .collect();
    for (index, heading) in elements.iter().enumerate() {
        let Some(level) = heading_level(heading.value().name()) else {
            continue;
        };
        if is_in_excluded(*heading) || has_excluded_ancestor(*heading) {
            continue;
        }
        let text = element_text(heading);
        if !heading_is_resource_style(&text) {
            continue;
        }
        let mut destinations = HashSet::new();
        for el in &elements[index + 1..] {
            if is_in_excluded(*el) || has_excluded_ancestor(*el) {
                continue;
            }
            if heading_level(el.value().name()).is_some_and(|next| next <= level) {
                break;
            }
            if el.value().name() != "a" {
                continue;
            }
            let Some(href) = el.attr("href") else {
                continue;
            };
            let Ok(abs) = source.normalized.join(href) else {
                continue;
            };
            let Ok(url) = safe_parse(abs.as_str()) else {
                continue;
            };
            if url.host_ascii() != source.host_ascii() {
                destinations.insert(url.normalized.to_string());
            }
        }
        if destinations.len() >= 3 {
            return Some((text, destinations.len()));
        }
    }
    None
}
