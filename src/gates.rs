//! Gates: thresholds, violation counting, enforce vs audit decisions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thresholds {
    /// Max allowed broken internal page paths (incl. assets outside page graph).
    #[serde(default)]
    pub brokenPaths: Option<usize>,
    #[serde(default)]
    pub brokenFragments: Option<usize>,
    #[serde(default)]
    pub orphans: Option<usize>,
    #[serde(default)]
    pub unreachable: Option<usize>,
    /// Fail when any reachable page depth strictly exceeds this.
    #[serde(default)]
    pub maxDepth: Option<usize>,
    #[serde(default)]
    pub hierarchyViolations: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThresholdOutcome {
    pub name: String,
    pub observed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    pub violated: bool,
}

pub fn evaluate(thresholds: &Option<Thresholds>, counts: &GateCounts) -> Vec<ThresholdOutcome> {
    let t = thresholds.clone().unwrap_or_default();
    let mk = |name: &str, observed: usize, limit: Option<usize>| ThresholdOutcome {
        name: name.to_string(),
        observed,
        limit,
        violated: limit.map(|l| observed > l).unwrap_or(false),
    };
    vec![
        mk("brokenPaths", counts.broken_paths, t.brokenPaths),
        mk(
            "brokenFragments",
            counts.broken_fragments,
            t.brokenFragments,
        ),
        mk("orphans", counts.orphans, t.orphans),
        mk("unreachable", counts.unreachable, t.unreachable),
        mk("maxDepth", counts.max_depth_observed, t.maxDepth),
        mk(
            "hierarchyViolations",
            counts.hierarchy_violations,
            t.hierarchyViolations,
        ),
    ]
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GateCounts {
    pub broken_paths: usize,
    pub broken_fragments: usize,
    pub orphans: usize,
    pub unreachable: usize,
    pub max_depth_observed: usize,
    pub hierarchy_violations: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_threshold_key_is_rejected() {
        let bad: Result<Thresholds, _> = serde_json::from_str(r#"{"brokenPathz": 0}"#);
        assert!(bad.is_err());
        let good: Result<Thresholds, _> = serde_json::from_str(r#"{"brokenPaths": 0}"#);
        assert!(good.is_ok());
    }

    #[test]
    fn violated_when_observed_exceeds_limit() {
        let t = Thresholds {
            brokenPaths: Some(2),
            ..Default::default()
        };
        let counts = GateCounts {
            broken_paths: 3,
            ..Default::default()
        };
        let out = evaluate(&Some(t), &counts);
        let bp = out.iter().find(|o| o.name == "brokenPaths").unwrap();
        assert!(bp.violated);
        assert_eq!(bp.observed, 3);
        assert_eq!(bp.limit, Some(2));
    }

    #[test]
    fn no_threshold_no_violation() {
        let counts = GateCounts {
            broken_paths: 99,
            ..Default::default()
        };
        let out = evaluate(&None, &counts);
        assert!(out.iter().all(|o| !o.violated));
    }

    #[test]
    fn at_limit_passes() {
        let t = Thresholds {
            orphans: Some(1),
            ..Default::default()
        };
        let counts = GateCounts {
            orphans: 1,
            ..Default::default()
        };
        let out = evaluate(&Some(t), &counts);
        assert!(!out.iter().any(|o| o.violated));
    }
}
