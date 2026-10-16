use scraper::{ElementRef, Html, Node};

pub fn hidden(el: ElementRef<'_>) -> bool {
    let v = el.value();
    if v.attr("hidden").is_some()
        || v.attr("aria-hidden")
            .is_some_and(|s| s.eq_ignore_ascii_case("true"))
    {
        return true;
    }
    v.attr("style")
        .unwrap_or("")
        .to_ascii_lowercase()
        .split(';')
        .any(|d| {
            let Some((p, v)) = d.split_once(':') else {
                return false;
            };
            let v = v.trim().trim_end_matches("!important").trim();
            matches!(
                (p.trim(), v),
                ("display", "none") | ("visibility", "hidden" | "collapse")
            )
        })
}

pub fn noneditorial(el: ElementRef<'_>) -> bool {
    hidden(el)
        || matches!(
            el.value().name(),
            "nav"
                | "header"
                | "footer"
                | "aside"
                | "script"
                | "style"
                | "template"
                | "noscript"
                | "head"
        )
        || el.attr("id") == Some("_mig-drawer")
        || el
            .attr("class")
            .is_some_and(|c| c.split_whitespace().any(|t| t == "_mig-drawer"))
        || el.attr("role").is_some_and(|r| {
            matches!(
                r,
                "navigation" | "banner" | "contentinfo" | "complementary" | "menu" | "menubar"
            )
        })
        || (el.attr("role") == Some("dialog")
            && el
                .attr("aria-label")
                .is_some_and(|s| s.eq_ignore_ascii_case("Site menu")))
}

pub fn editorial(el: ElementRef<'_>) -> bool {
    !noneditorial(el)
        && !el
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(noneditorial)
}

fn walk(el: ElementRef<'_>, out: &mut String, unlinked: bool) {
    if hidden(el)
        || matches!(
            el.value().name(),
            "script" | "style" | "template" | "noscript"
        )
    {
        out.push(' ');
        return;
    }
    if unlinked && (noneditorial(el) || matches!(el.value().name(), "a" | "area")) {
        out.push('\0');
        return;
    }
    if !unlinked && matches!(el.value().name(), "img" | "area") {
        out.push(' ');
        out.push_str(el.attr("alt").unwrap_or(""));
        out.push(' ');
    }
    for child in el.children() {
        match child.value() {
            Node::Text(t) => out.push_str(&t.text),
            Node::Element(_) => {
                if let Some(e) = ElementRef::wrap(child) {
                    let block = matches!(
                        e.value().name(),
                        "p" | "div" | "section" | "article" | "li" | "br" | "h1" | "h2" | "h3"
                    );
                    if block {
                        out.push(' ');
                    }
                    walk(e, out, unlinked);
                    if block {
                        out.push(' ');
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn anchor(el: ElementRef<'_>) -> String {
    if hidden(el) || el.ancestors().filter_map(ElementRef::wrap).any(hidden) {
        return String::new();
    }
    let mut out = String::new();
    walk(el, &mut out, false);
    if out.trim().is_empty() {
        if let Some(id) = el
            .attr("aria-labelledby")
            .and_then(|s| s.split_whitespace().next())
        {
            if let Some(target) = el
                .ancestors()
                .last()
                .into_iter()
                .flat_map(|root| root.descendants())
                .filter_map(ElementRef::wrap)
                .find(|e| e.attr("id") == Some(id))
            {
                out = target.text().collect::<String>();
            }
        }
    }
    if out.trim().is_empty() {
        out = el
            .attr("aria-label")
            .or_else(|| el.attr("title"))
            .unwrap_or("")
            .to_string();
    }
    normalize(&out)
}

pub fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn unlinked(doc: &Html) -> String {
    let mut out = String::new();
    walk(doc.root_element(), &mut out, true);
    normalize(&out)
}
