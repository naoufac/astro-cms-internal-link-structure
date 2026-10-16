#[derive(Debug, Clone)]
pub struct RobotsRules {
    pub rules: Vec<(bool, String)>,
}

struct Section {
    useragents: Vec<String>,
    rules: Vec<(bool, String)>,
}

impl RobotsRules {
    pub fn parse(text: &str, user_agent: &str) -> Self {
        let ua = user_agent.to_ascii_lowercase();
        let mut sections: Vec<Section> = Vec::new();
        let mut current = Section {
            useragents: Vec::new(),
            rules: Vec::new(),
        };
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim().to_ascii_lowercase().as_str() {
                "user-agent" if !value.is_empty() => {
                    if !current.rules.is_empty() {
                        sections.push(current);
                        current = Section {
                            useragents: Vec::new(),
                            rules: Vec::new(),
                        };
                    }
                    current.useragents.push(value.to_ascii_lowercase());
                }
                "allow" | "disallow" if !current.useragents.is_empty() => {
                    current
                        .rules
                        .push((key.trim().eq_ignore_ascii_case("allow"), value.to_string()));
                }
                _ => {}
            }
        }
        sections.push(current);
        let specificity = |section: &Section| {
            section
                .useragents
                .iter()
                .filter(|name| name.as_str() != "*" && ua.contains(name.as_str()))
                .map(String::len)
                .max()
        };
        let longest = sections.iter().filter_map(specificity).max();
        let rules = sections
            .iter()
            .filter(|section| match longest {
                Some(length) => specificity(section) == Some(length),
                None => section.useragents.iter().any(|name| name == "*"),
            })
            .flat_map(|section| section.rules.iter().cloned())
            .collect();
        Self { rules }
    }

    pub fn path_allowed(&self, path: &str) -> bool {
        let path = comparable(path);
        self.rules
            .iter()
            .filter(|(_, rule)| !rule.is_empty())
            .filter_map(|(allow, rule)| {
                let rule = comparable(rule);
                matches(&rule, &path).then(|| {
                    let specificity = rule
                        .strip_suffix(b"$")
                        .unwrap_or(&rule)
                        .iter()
                        .filter(|b| **b != b'*')
                        .count();
                    (specificity, *allow)
                })
            })
            .max()
            .map(|(_, allow)| allow)
            .unwrap_or(true)
    }
}

fn comparable(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                let n = (a * 16 + b) as u8;
                if n.is_ascii_alphanumeric() || b"-._~".contains(&n) {
                    out.push(n);
                } else {
                    out.extend_from_slice(format!("%{n:02X}").as_bytes());
                }
                i += 3;
                continue;
            }
        }
        if byte >= 128 {
            out.extend_from_slice(format!("%{byte:02X}").as_bytes());
        } else {
            out.push(byte);
        }
        i += 1;
    }
    out
}

fn matches(pattern: &[u8], path: &[u8]) -> bool {
    let anchored = pattern.last() == Some(&b'$');
    let pattern = if anchored {
        &pattern[..pattern.len() - 1]
    } else {
        pattern
    };
    let (mut p, mut t) = (0usize, 0usize);
    let mut star = None;
    let mut retry = 0usize;
    loop {
        if p == pattern.len() && (!anchored || t == path.len()) {
            return true;
        }
        if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            retry = t;
            p += 1;
        } else if p < pattern.len() && t < path.len() && pattern[p] == path[t] {
            p += 1;
            t += 1;
        } else if let Some(position) = star {
            if retry == path.len() {
                return false;
            }
            retry += 1;
            t = retry;
            p = position + 1;
        } else {
            return false;
        }
    }
}
