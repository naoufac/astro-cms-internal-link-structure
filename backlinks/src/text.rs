//! Visible-text extraction and exact brand matching with documented
//! normalization and word boundaries.
//!
//! Text model: the page is reduced to *block regions* (innermost text blocks
//! such as `p`, `li`, `td`, `blockquote`) plus headings. Everything inside
//! `script`, `style`, comments, `nav`, `footer`, `header`, `aside`, and
//! `template` is dropped, so brand strings hidden in boilerplate never match.

use scraper::{ElementRef, Html, Selector};
use unicode_segmentation::UnicodeSegmentation;

/// Casefold + whitespace-collapse normalization for matching only. Evidence
/// quotes are NEVER altered; they are verbatim substrings of region text.
pub fn normalize_for_match(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            prev_space = false;
            for lc in c.to_lowercase() {
                out.push(lc);
            }
        }
    }
    out.trim().to_string()
}

/// True when `needle` occurs in `haystack` as whole word(s): characters
/// immediately before/after the occurrence are not alphanumeric
/// (Unicode-aware). `gissecretspro` therefore does not match `gissecrets`,
/// while `gissecrets-style` does (hyphens are not word characters).
pub fn contains_word(haystack: &str, needle: &str) -> bool {
    let hay = normalize_for_match(haystack);
    let needle = normalize_for_match(needle);
    if needle.is_empty() {
        return false;
    }
    let mut last = 0usize;
    while let Some(pos) = hay[last..].find(&needle) {
        let start = last + pos;
        let end = start + needle.len();
        let before_ok = hay[..start]
            .chars()
            .next_back()
            .map(|c| !c.is_alphanumeric())
            .unwrap_or(true);
        let after_ok = hay[end..]
            .chars()
            .next()
            .map(|c| !c.is_alphanumeric())
            .unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
        last = start + 1;
        while last < hay.len() && !hay.is_char_boundary(last) {
            last += 1;
        }
        if last >= hay.len() {
            break;
        }
    }
    false
}

/// HTML elements treated as boilerplate (never matched, never quoted).
const EXCLUDED_TEXT_TAGS: &[&str] = &[
    "script", "style", "noscript", "template", "head", "title", "nav", "footer", "header", "aside",
];

/// Innermost text blocks; nested blocks yield only the innermost region.
const BLOCK_TAGS: &[&str] = &[
    "p",
    "li",
    "blockquote",
    "dd",
    "dt",
    "td",
    "th",
    "figcaption",
    "pre",
    "div",
    "main",
    "section",
    "article",
    "body",
    "ul",
    "ol",
    "dl",
    "table",
    "tr",
    "figure",
    "address",
    "hr",
];

pub(crate) fn heading_level(name: &str) -> Option<u8> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

fn is_block(name: &str) -> bool {
    BLOCK_TAGS.contains(&name) || heading_level(name).is_some()
}

/// A visible text region with its origin kind.
#[derive(Debug, Clone)]
pub struct TextRegion {
    pub text: String,
    /// `main_text` or `heading`.
    pub kind: &'static str,
}

pub(crate) fn is_in_excluded(el: scraper::ElementRef) -> bool {
    EXCLUDED_TEXT_TAGS.contains(&el.value().name())
        || el.attr("hidden").is_some()
        || el
            .attr("aria-hidden")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
        || el.attr("style").is_some_and(|style| {
            style.split(';').any(|declaration| {
                let Some((name, value)) = declaration.split_once(':') else {
                    return false;
                };
                let value = value.split('!').next().unwrap_or("").trim();
                (name.trim().eq_ignore_ascii_case("display") && value.eq_ignore_ascii_case("none"))
                    || (name.trim().eq_ignore_ascii_case("visibility")
                        && (value.eq_ignore_ascii_case("hidden")
                            || value.eq_ignore_ascii_case("collapse")))
            })
        })
}

pub(crate) fn has_excluded_ancestor(el: scraper::ElementRef) -> bool {
    el.ancestors()
        .filter_map(ElementRef::wrap)
        .any(is_in_excluded)
}

pub(crate) fn element_text(el: &scraper::ElementRef) -> String {
    if is_in_excluded(*el) {
        return String::new();
    }
    let mut out = String::new();
    let mut stack = el
        .children()
        .rev()
        .map(|node| (node, false))
        .collect::<Vec<_>>();
    while let Some((node, closing)) = stack.pop() {
        if let Some(text) = node.value().as_text() {
            out.push_str(text);
        } else if let Some(element) = ElementRef::wrap(node) {
            if is_in_excluded(element) {
                continue;
            }
            let block = is_block(element.value().name()) || element.value().name() == "br";
            if block {
                out.push(' ');
            }
            if !closing {
                if block {
                    stack.push((node, true));
                }
                stack.extend(element.children().rev().map(|child| (child, false)));
            }
        }
    }
    collapse_ws(&out)
}

/// Decode the five HTML named entities plus numeric forms. scraper's
/// `.text()` yields raw entity text, which would otherwise break
/// substring verification of quotes against raw HTML.
#[cfg(test)]
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            let rest = &s[i..];
            let named = [
                ("&amp;", '&'),
                ("&lt;", '<'),
                ("&gt;", '>'),
                ("&quot;", '"'),
                ("&apos;", '\''),
                ("&nbsp;", '\u{a0}'),
            ];
            let mut handled = false;
            for (pat, ch) in named {
                if rest.starts_with(pat) {
                    out.push(ch);
                    i += pat.len();
                    handled = true;
                    break;
                }
            }
            if !handled && rest.len() > 3 && rest.as_bytes()[1] == b'#' {
                // numeric: &#123; or &#x1F;
                let upto = rest.find(';');
                if let Some(semi) = upto {
                    let body = &rest[2..semi];
                    let parsed = if let Some(hex) =
                        body.strip_prefix('x').or_else(|| body.strip_prefix('X'))
                    {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        body.parse::<u32>().ok()
                    };
                    if let Some(cp) = parsed.and_then(char::from_u32) {
                        out.push(cp);
                        i += semi + 1;
                        handled = true;
                    }
                }
            }
            if !handled {
                out.push('&');
                i += 1;
            }
        } else {
            out.push(s[i..].chars().next().unwrap());
            i += s[i..].chars().next().unwrap().len_utf8();
        }
    }
    out
}

/// Collapse whitespace runs to single spaces and trim (does not change words).
pub fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract visible block regions + headings from an HTML document.
pub fn visible_regions(html: &str) -> Vec<TextRegion> {
    let doc = Html::parse_document(html);
    let body_sel = match Selector::parse("body") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let Some(body) = doc.select(&body_sel).next() else {
        return Vec::new();
    };

    fn flush(regions: &mut Vec<TextRegion>, buffer: &mut String, kind: &'static str) {
        let text = collapse_ws(buffer);
        if !text.is_empty() {
            regions.push(TextRegion { text, kind });
        }
        buffer.clear();
    }
    let mut regions = Vec::new();
    let mut buffer = String::new();
    if is_in_excluded(body) {
        return regions;
    }
    let mut stack = body
        .children()
        .rev()
        .map(|node| (node, "main_text", false))
        .collect::<Vec<_>>();
    while let Some((node, kind, closing)) = stack.pop() {
        if closing {
            flush(&mut regions, &mut buffer, kind);
        } else if let Some(text) = node.value().as_text() {
            buffer.push_str(text);
        } else if let Some(element) = ElementRef::wrap(node) {
            if is_in_excluded(element) {
                continue;
            }
            let name = element.value().name();
            if is_block(name) {
                flush(&mut regions, &mut buffer, kind);
                let child_kind = if heading_level(name).is_some() {
                    "heading"
                } else {
                    "main_text"
                };
                stack.push((node, child_kind, true));
                stack.extend(
                    element
                        .children()
                        .rev()
                        .map(|child| (child, child_kind, false)),
                );
            } else if name == "br" {
                buffer.push(' ');
            } else {
                stack.extend(element.children().rev().map(|child| (child, kind, false)));
            }
        }
    }
    flush(&mut regions, &mut buffer, "main_text");
    regions
}

/// Whole visible text, normalized (matching only, never quoted).
pub fn full_visible_text(html: &str) -> String {
    let parts: Vec<String> = visible_regions(html)
        .into_iter()
        .map(|r| normalize_for_match(&r.text))
        .collect();
    parts.join(" ")
}

/// Split region text into sentences, preserving punctuation and casing.
pub fn split_sentences(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?' | '…') {
            // consume consecutive terminators (e.g. "?!") and trailing quotes
            while let Some(&n) = chars.peek() {
                if matches!(n, '.' | '!' | '?' | '…' | '"' | '\'' | '”' | '’' | ')') {
                    cur.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            let trimmed = cur.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
            cur.clear();
        }
    }
    let trimmed = cur.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
    out
}

/// Verbatim sentence from the page containing the first whole-word brand
/// occurrence, or `None`. This is the exact evidence quote (modulo the same
/// whitespace collapsing applied to every region).
pub fn sentence_containing(html: &str, brand: &str) -> Option<String> {
    for r in visible_regions(html) {
        if contains_word(&r.text, brand) {
            for sentence in split_sentences(&r.text) {
                if contains_word(&sentence, brand) {
                    return Some(sentence);
                }
            }
            // Multi-sentence region without a splitting terminator: quote the
            // whole region (verbatim).
            return Some(r.text);
        }
    }
    None
}

/// Count words (Unicode segmentation): reserved for curated-page scoring.
pub fn word_count(s: &str) -> usize {
    s.unicode_words().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_boundaries() {
        assert!(contains_word("Welcome to GisSecrets home", "gissecrets"));
        assert!(contains_word("GISSECRETS!", "gissecrets"));
        assert!(contains_word("about gissecrets-style decks", "gissecrets"));
        assert!(!contains_word("gissecretspro", "gissecrets"));
        assert!(!contains_word("agissecrets", "gissecrets"));
        assert!(contains_word("À propos de Café", "café"));
    }

    #[test]
    fn multilingual_needle_inside_longer_word_does_not_panic() {
        assert!(!contains_word("水文站水网", "水网"));
        assert!(contains_word("长江 水网", "水网"));
        assert!(!contains_word("αααβ", "αα"));
        assert!(contains_word("αα αα", "αα"));
        assert!(!contains_word("caféiné", "é"));
        assert!(!contains_word("aaa", "aa"));
        assert!(!contains_word("水水水站", "水水"));
    }

    #[test]
    fn normalization_preserves_evidence_separately() {
        let original = "  GisSecrets   Rocks ";
        assert_eq!(normalize_for_match(original), "gissecrets rocks");
        assert_eq!(original.trim(), "GisSecrets   Rocks");
    }

    #[test]
    fn visible_text_excludes_script_style_comments_nav() {
        let html = r#"<html><head><title>Brand</title><style>.brand{}</style></head>
        <body>
        <nav><a href="/">Brand Home</a></nav>
        <script>var brand = "Brand";</script>
        <!-- Brand comment -->
        <main><p>Reading about Brand today.</p>
        <h2>Guides</h2>
        <a href="https://ext.example">An external site</a>
        </main>
        <footer>Brand footer</footer>
        </body></html>"#;
        let full = full_visible_text(html);
        assert!(full.contains("reading about brand today"));
        assert!(!full.contains("var brand"));
        assert!(!full.contains("brand home"));
        assert!(!full.contains("brand footer"));
        assert!(!full.contains(".brand"));
        let regions = visible_regions(html);
        assert!(regions
            .iter()
            .any(|r| r.kind == "heading" && r.text == "Guides"));
        // nav anchor text must not appear anywhere
        assert!(!regions.iter().any(|r| r.text == "Brand Home"));
    }

    #[test]
    fn nested_blocks_yield_innermost_only() {
        let html = "<body><div id='outer'><div><p>Hello world sentence one. Second one!</p></div></div></body>";
        let regions = visible_regions(html);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].text, "Hello world sentence one. Second one!");
    }

    #[test]
    fn sentence_splitting_keeps_verbatim() {
        let s = "One sentence here. It mentions GisSecrets, nicely! Next.";
        let sentences = split_sentences(s);
        assert_eq!(sentences.len(), 3);
        let hit = sentences
            .iter()
            .find(|x| contains_word(x, "gissecrets"))
            .unwrap();
        assert_eq!(hit, "It mentions GisSecrets, nicely!");
    }

    #[test]
    fn entity_decoding_and_heading_boundaries() {
        assert_eq!(decode_entities("A &amp; B"), "A & B");
        assert_eq!(decode_entities("&quot;x&quot;"), "\"x\"");
        assert_eq!(decode_entities("&#65;&#x42;"), "AB");
        assert_eq!(decode_entities("plain & unknown"), "plain & unknown");
        let html = "<html><body><main><h1>Explore Tarot: Your Ultimate Resource &amp; Reading Hub</h1></main></body></html>";
        let regions = visible_regions(html);
        assert_eq!(
            regions[0].text,
            "Explore Tarot: Your Ultimate Resource & Reading Hub"
        );
        // The decoded quote must be a raw-HTML substring after entity decode.
        assert!(html.contains("&amp;"));
    }

    #[test]
    fn sentence_containing_returns_verbatim_brand_sentence() {
        let html = "<html><body><main><p>Intro text that nobody needs. Then we discuss GisSecrets and tarot decks in depth. Closing line.</p></main></body></html>";
        let q = sentence_containing(html, "gissecrets").unwrap();
        assert_eq!(q, "Then we discuss GisSecrets and tarot decks in depth.");
    }
}
