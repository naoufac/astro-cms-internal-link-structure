use crate::{
    audit::Opportunity, graph::Graph, hierarchy::HierarchyConfig, metadata::Metadata,
    parser::PageLinks,
};
use std::collections::BTreeMap;

pub fn suggest(
    pages: &BTreeMap<String, PageLinks>,
    metadata: &BTreeMap<String, Metadata>,
    graph: &Graph,
    hierarchy: Option<&HierarchyConfig>,
) -> Vec<Opportunity> {
    let mut phrases: BTreeMap<String, Vec<(&str, String)>> = BTreeMap::new();
    for (key, page) in pages {
        let phrase = hierarchy
            .and_then(|h| h.labels.get(key))
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&page.title);
        let phrase = crate::visible::normalize(phrase);
        if phrase.split_whitespace().count() < 2 {
            continue;
        }
        phrases
            .entry(phrase.to_lowercase())
            .or_default()
            .push((key, phrase));
    }
    let mut out = Vec::new();
    for (source, page) in pages {
        if !metadata.get(source).is_some_and(|m| m.eligible) {
            continue;
        }
        let text = page.unlinked.to_lowercase();
        for (normalized, targets) in &phrases {
            if targets.len() != 1 {
                continue;
            }
            let (target, phrase) = &targets[0];
            if source == target
                || !metadata.get(*target).is_some_and(|m| m.eligible)
                || graph.edges.get(source).is_some_and(|s| s.contains(*target))
            {
                continue;
            }
            if whole_phrase(&text, normalized) {
                out.push(Opportunity { kind: "contextual".into(), source: source.clone(), target: (*target).into(), detail: format!("Unique target phrase '{phrase}' appears in visible unlinked editorial text; review suitability") });
            }
        }
    }
    out
}

pub(crate) fn whole_phrase(text: &str, phrase: &str) -> bool {
    let phrase = phrase.trim();
    if phrase.is_empty() {
        return false;
    }
    text.match_indices(phrase).any(|(i, _)| {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        !text[..i].chars().next_back().is_some_and(word)
            && !text[i + phrase.len()..].chars().next().is_some_and(word)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_phrase_boundaries() {
        assert!(whole_phrase("a unique guide.", "unique guide"));
        assert!(!whole_phrase("a unique guides", "unique guide"));
        assert!(!whole_phrase("myunique guide", "unique guide"));
    }
}
