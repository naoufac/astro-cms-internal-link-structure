//! Provider-neutral import format parsing and transformation helpers.
//!
//! Imported candidates carry provenance (query/provider/date). Search titles
//! and snippets are discovery hints ONLY and are never treated as evidence.

use crate::report::{DedupeSummary, ImportedCandidate};
use serde_json::Value;

/// Parse an imported-candidates JSON file. Accepts:
/// 1. `[ {...}, ... ]`: already provider-neutral array;
/// 2. provider-shaped `[{query, response: {data: {web: [{url, title, description}]}}}]`
///    (raw provider export; provider recorded as "imported-provider");
/// 3. `{candidates: [...]}` wrapper.
pub fn parse_candidates(text: &str) -> anyhow::Result<(Vec<ImportedCandidate>, DedupeSummary)> {
    let v: Value = serde_json::from_str(text).map_err(|e| anyhow::anyhow!("invalid JSON: {e}"))?;
    let mut out: Vec<ImportedCandidate> = Vec::new();

    match &v {
        Value::Array(items) => {
            for item in items {
                if is_provider_export(item) {
                    out.extend(from_provider_export(item)?);
                } else {
                    out.push(from_neutral(item)?);
                }
            }
        }
        Value::Object(o) => {
            if let Some(cands) = o.get("candidates").and_then(|c| c.as_array()) {
                for item in cands {
                    if is_provider_export(item) {
                        out.extend(from_provider_export(item)?);
                    } else {
                        out.push(from_neutral(item)?);
                    }
                }
            } else {
                anyhow::bail!("object input must have a 'candidates' array");
            }
        }
        _ => anyhow::bail!("unsupported import shape"),
    }

    // Dedupe by normalized URL, preserving first provenance.
    let mut seen = std::collections::HashSet::new();
    let mut duplicates = Vec::new();
    let mut unique = Vec::new();
    for c in out {
        let key = crate::neturl::safe_parse(&c.url)
            .map(|u| crate::neturl::dedupe_key(&u))
            .unwrap_or_else(|_| c.url.clone());
        if seen.insert(key.clone()) {
            unique.push(c);
        } else {
            duplicates.push(key);
        }
    }
    let input = unique.len() + duplicates.len();
    let unique_count = unique.len();
    Ok((
        unique,
        DedupeSummary {
            input,
            unique: unique_count,
            duplicates,
        },
    ))
}

fn is_provider_export(v: &Value) -> bool {
    v.get("query").is_some() && v.get("response").is_some()
}

fn from_provider_export(item: &Value) -> anyhow::Result<Vec<ImportedCandidate>> {
    if item["response"]["success"] == false
        || !item["response"]["error"].is_null()
        || !item["response"]["data"]["error"].is_null()
    {
        anyhow::bail!("provider acquisition failed; import is not an empty successful search");
    }
    let query = item
        .get("query")
        .and_then(|q| q.as_str())
        .unwrap_or("")
        .to_string();
    let mut out = Vec::new();
    let web = item["response"]["data"]["web"].as_array().ok_or_else(|| {
        anyhow::anyhow!("malformed provider export: response.data.web must be an array")
    })?;
    for w in web {
        out.push(ImportedCandidate {
            url: w
                .get("url")
                .and_then(|u| u.as_str())
                .filter(|u| !u.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("provider result missing nonempty url"))?
                .to_string(),
            query: Some(query.clone()),
            provider: Some("imported-provider".to_string()),
            collected_at: None,
            search_title: w.get("title").and_then(|t| t.as_str()).map(String::from),
            search_snippet: w
                .get("description")
                .and_then(|d| d.as_str())
                .map(String::from),
            suggested_target: None,
            backlink_hints: vec![],
        });
    }
    Ok(out)
}

fn from_neutral(item: &Value) -> anyhow::Result<ImportedCandidate> {
    let url = item
        .get("url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| anyhow::anyhow!("candidate missing 'url'"))?;
    Ok(ImportedCandidate {
        backlink_hints: vec![],
        url: url.to_string(),
        query: item.get("query").and_then(|q| q.as_str()).map(String::from),
        provider: item
            .get("provider")
            .and_then(|p| p.as_str())
            .map(String::from),
        collected_at: item
            .get("collected_at")
            .and_then(|c| c.as_str())
            .map(String::from),
        search_title: item
            .get("search_title")
            .and_then(|t| t.as_str())
            .map(String::from),
        search_snippet: item
            .get("search_snippet")
            .and_then(|s| s.as_str())
            .map(String::from),
        suggested_target: item
            .get("suggested_target")
            .and_then(|s| s.as_str())
            .map(String::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_neutral_array_and_dedupes() {
        let text = r#"[
            {"url": "https://a.example/x", "query": "q1", "provider": "p", "collected_at": "2026-10-15"},
            {"url": "https://a.example/x/", "query": "q1", "provider": "p", "collected_at": "2026-10-15"},
            {"url": "https://b.example/y", "query": "q2", "provider": "p"}
        ]"#;
        let (cands, dedupe) = parse_candidates(text).unwrap();
        assert_eq!(cands.len(), 2);
        assert_eq!(dedupe.duplicates.len(), 1);
        assert_eq!(cands[0].query.as_deref(), Some("q1"));
        assert_eq!(cands[0].provider.as_deref(), Some("p"));
    }

    #[test]
    fn parses_provider_export_shape() {
        let text = r#"[{
            "query": "tarot resources",
            "response": {"success": true, "data": {"web": [
                {"url": "https://c.example/r", "title": "T", "description": "D"}
            ]}}}
        ]"#;
        let (cands, _) = parse_candidates(text).unwrap();
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].url, "https://c.example/r");
        assert_eq!(cands[0].query.as_deref(), Some("tarot resources"));
        assert_eq!(cands[0].search_title.as_deref(), Some("T"));
    }

    #[test]
    fn rejects_non_json_and_bad_shapes() {
        assert!(parse_candidates("not json").is_err());
        assert!(parse_candidates("{\"foo\": 1}").is_err());
        assert!(parse_candidates("[{\"nope\": 1}]").is_err());
    }
}
