# Rust visible-text rules

The library exposes `parser::extract(html: &str) -> PageLinks` and `visible::anchor(el: scraper::ElementRef<'_>) -> String`. This repair does not change their signatures or the versioned JSON request/report schema.

## Anchor evidence

- Hidden anchors, or anchors with a hidden ancestor, have empty anchor evidence.
- Visible descendant text and image/area `alt` text are normalized by collapsing whitespace.
- If that evidence is empty, `aria-labelledby` resolves the first whitespace-separated ID against the same document. The first element in document order with that exact ID supplies its descendant text. Further IDs and duplicate-ID elements are ignored. Referenced text is normalized without recursively resolving label references.
- If no usable referenced text exists, the existing `aria-label`, then `title` fallback applies. A missing reference with no other label still yields empty evidence and an advisory vague-anchor finding.
- This is offline evidence extraction, not a complete browser accessibility-tree implementation. Descriptive resolved labels are assessed by the existing advisory anchor rules; gate semantics are unchanged.

## Unlinked contextual text

`visible::unlinked(&scraper::Html) -> String` and `PageLinks.unlinked` retain their existing string types. Removed visible anchors and noneditorial subtrees now insert a NUL boundary, not whitespace, so target phrases cannot match across removed visible text. This intermediate string is not serialized into the JSON report. Hidden spans and script/style/template/noscript text still disappear with a whitespace separator. Inline visible formatting remains contiguous. Contextual recommendations are advisory and never affect enforce gates.
