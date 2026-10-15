//! Engine version and JSON I/O schema version.
//! Bump SCHEMA_VERSION on any breaking report/request change.

pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SCHEMA_VERSION: &str = "1";

fn default_build_format() -> String {
    "directory".to_string()
}

fn default_trailing_slash() -> String {
    "ignore".to_string()
}

fn default_mode() -> String {
    "audit".to_string()
}

/// Versioned request envelope sent on stdin as JSON.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanRequest {
    pub schemaVersion: String,
    pub artifactRoot: String,
    pub site: Option<String>,
    #[serde(default)]
    pub originAliases: Vec<String>,
    #[serde(default)]
    pub basePath: String,
    #[serde(default = "default_trailing_slash")]
    pub trailingSlash: String,
    #[serde(default = "default_build_format")]
    pub buildFormat: String,
    /// audit | enforce. Both produce identical reports; enforce decides the
    /// process exit code when thresholds are violated.
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub entrypoints: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Page URL paths (in-base keys, same form as report node paths)
    /// excluded from eligibility AND graph traversal.
    #[serde(default)]
    pub excludePagePaths: Vec<String>,
    #[serde(default)]
    pub hierarchy: Option<crate::hierarchy::HierarchyConfig>,
    #[serde(default)]
    pub thresholds: Option<crate::gates::Thresholds>,
    #[serde(default)]
    pub diagnosticsPath: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageRecord {
    pub urlPath: String,
    #[serde(default)]
    pub filePath: String,
}

impl PageRecord {
    pub fn key(&self) -> &str {
        &self.urlPath
    }
}
