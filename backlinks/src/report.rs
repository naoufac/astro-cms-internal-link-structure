//! Versioned configuration and report data types (JSON schemas).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const CONFIG_SCHEMA_VERSION: u32 = 1;
pub const REPORT_SCHEMA_VERSION: u32 = 1;
pub const TOOL_NAME: &str = "backlink-opportunities-detector";
pub const TOOL_VERSION: &str = "0.1.0";

/// Detection rule identifiers recorded per opportunity.
pub const RULE_MENTION: &str = "unlinked-brand-mention";
pub const RULE_RESOURCE_PAGE: &str = "curated-resource-page-lead";
pub const RULE_BROKEN_LINK: &str = "broken-link-replacement-possibility";
pub const RULE_ENGINE_VERSION: &str = "1.0.0";

/// Candidate target page on the user's own site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetPage {
    /// Canonical first-party URL (must parse as http/https).
    pub url: String,
    /// Short human label.
    pub title: String,
    /// Topic keywords describing this page.
    #[serde(default)]
    pub topics: Vec<String>,
}

/// Detector configuration, supplied as JSON on the command line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectorConfig {
    pub schema_version: u32,
    /// Origin of the site being promoted (first party), e.g. `https://gissecrets.com`.
    pub site_origin: String,
    /// All owned domains (registrable domains), including the site origin's.
    /// A page linking to ANY of these is disqualified as "already linking".
    pub owned_domains: Vec<String>,
    /// Approved brand names / aliases matched exactly with word boundaries.
    pub brands: Vec<String>,
    /// Candidate first-party target pages for placements and replacements.
    pub target_pages: Vec<TargetPage>,
    /// Domain suffixes never treated as candidate sources (first-party and
    /// platforms that block automated access anyway).
    #[serde(default)]
    pub excluded_domains: Vec<String>,
    #[serde(default)]
    pub limits: Limits,
    /// Opt-in network switch. When false the detect subcommand performs NO
    /// network I/O and only analyzes inline/imported fixture payloads.
    #[serde(default)]
    pub network: NetworkPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limits {
    /// Maximum candidate source pages fetched per run (default 25).
    #[serde(default = "default_candidate_pages")]
    pub candidate_pages: usize,
    /// Maximum unique outbound link destinations checked per run (default 100).
    #[serde(default = "default_outbound_checks")]
    pub outbound_checks: usize,
    /// Concurrent HTTP requests (default 2).
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    /// Per-origin minimum spacing between requests, milliseconds (default 1500).
    #[serde(default = "default_per_origin_delay_ms")]
    pub per_origin_delay_ms: u64,
    /// Per-request timeout, seconds (default 15).
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
    /// Decompressed response body cap, bytes (default 3 MiB).
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,
    /// Maximum redirect hops followed manually (default 5).
    #[serde(default = "default_max_redirects")]
    pub max_redirects: usize,
    /// Maximum outbound links analyzed per source page (default 100).
    #[serde(default = "default_outbound_per_page")]
    pub outbound_links_per_page: usize,
}

fn default_candidate_pages() -> usize {
    25
}
fn default_outbound_checks() -> usize {
    100
}
fn default_concurrency() -> usize {
    2
}
fn default_per_origin_delay_ms() -> u64 {
    1500
}
fn default_timeout_seconds() -> u64 {
    15
}
fn default_max_body_bytes() -> usize {
    3 * 1024 * 1024
}
fn default_max_redirects() -> usize {
    5
}
fn default_outbound_per_page() -> usize {
    100
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            candidate_pages: default_candidate_pages(),
            outbound_checks: default_outbound_checks(),
            concurrency: default_concurrency(),
            per_origin_delay_ms: default_per_origin_delay_ms(),
            timeout_seconds: default_timeout_seconds(),
            max_body_bytes: default_max_body_bytes(),
            max_redirects: default_max_redirects(),
            outbound_links_per_page: default_outbound_per_page(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkPolicy {
    /// When false (default) no network requests are made at all.
    #[serde(default)]
    pub enabled: bool,
    /// User agent sent with every request and used for robots.txt lookups.
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
    /// Directory (outside any site tree) for raw capture files and hashes.
    #[serde(default)]
    pub evidence_dir: Option<String>,
}

fn default_user_agent() -> String {
    "backlink-opportunities-detector/0.1 (+https://github.com/naoufac/astro-cms-internal-link-structure)".to_string()
}

// ---------------------------------------------------------------------------
// Search query generation
// ---------------------------------------------------------------------------

/// One generated search-query suggestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuerySuggestion {
    /// Documented template that produced this query.
    pub template: String,
    /// The literal query string to run in an external search tool.
    pub query: String,
    /// Which brand or target page this query was derived from.
    pub subject: String,
    /// Purpose bucket: `brand-mention` or `resource-page`.
    pub purpose: String,
}

#[derive(Debug, Serialize)]
pub struct QueryOutput {
    pub schema_version: u32,
    pub generated_at: String,
    /// Configured subjects used (not a guarantee of results).
    pub config_summary: QueryConfigSummary,
    pub queries: Vec<QuerySuggestion>,
    /// Fixed reminder stored with every export.
    pub usage_note: &'static str,
}

#[derive(Debug, Serialize)]
pub struct QueryConfigSummary {
    pub site_origin: String,
    pub brands: Vec<String>,
    pub target_pages: usize,
    pub topics: Vec<String>,
}

// ---------------------------------------------------------------------------
// Detection report
// ---------------------------------------------------------------------------

/// How a candidate source page was resolved during the run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateOutcome {
    /// Produced one or more opportunities.
    Opportunities,
    /// Fetched fine but nothing qualified; `notes` explains which rules ran.
    NoOpportunity,
    /// Rejected before or during fetch; `notes` carries the exact reason.
    Rejected,
    /// Infrastructure failure (DNS, TLS, timeout); `notes` carries the error.
    Error,
}

/// A single detected opportunity (any rule).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opportunity {
    /// Rule identifier, e.g. `unlinked-brand-mention`.
    pub rule: String,
    /// Rule engine version for audit reproducibility.
    pub rule_version: String,
    /// Page the evidence was found on.
    pub source_url: String,
    /// First-party page proposed for the link (from config target_pages).
    pub target_url: String,
    /// Exact verbatim snippet from the page's visible text.
    pub evidence_quote: String,
    /// Where the quote came from: `visible_text`, `heading`, or `anchor_text`.
    pub evidence_location: String,
    /// Only for broken-link rule: the dead destination URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broken_url: Option<String>,
    /// Only for broken-link rule: final URL after redirects (if reached).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broken_final_url: Option<String>,
    /// Observed HTTP status of the fetch that produced the evidence.
    pub observed_status: u16,
    /// UTC timestamp of the observation (RFC 3339).
    pub checked_at_utc: String,
    /// Redirect chain actually followed, first hop first.
    pub redirect_chain: Vec<String>,
    /// True when at least one link to an owned domain was checked and none found.
    pub existing_link_checked: bool,
    /// Always false for emitted opportunities; recorded for audit.
    pub links_to_owned_domain: bool,
    /// Whether the source itself is first-party (always false when emitted).
    pub source_is_first_party: bool,
    /// Facts directly observed from the fetch/HTML. No interpretation.
    pub observed_facts: Vec<String>,
    /// Explicitly labeled inference (topic fit, placement suitability).
    pub inferred_assessment: Vec<String>,
}

/// A candidate source page's full audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateRecord {
    pub url: String,
    /// Final URL after manual redirect handling (when fetched).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_url: Option<String>,
    pub outcome: CandidateOutcome,
    /// Rejection/error/limit details, or rule notes for no_opportunity.
    pub notes: String,
    /// HTTP status (when a response was received).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    pub robots: RobotsCheck,
    /// Raw capture reference: file name inside evidence_dir.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_file: Option<String>,
    /// SHA-256 of the captured body (lowercase hex).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_sha256: Option<String>,
    /// Provenance inherited from the import (never manufactured).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_collected_at: Option<String>,
    pub opportunities: Vec<Opportunity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RobotsCheck {
    /// How robots was resolved for this origin.
    /// `allowed` | `disallowed` | `unavailable` (treated as disallowed) | `not_applicable`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Counters for limit exhaustion and honesty reporting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunCounters {
    pub candidates_input: usize,
    pub candidates_deduplicated: usize,
    pub candidates_fetched: usize,
    pub candidates_rejected: usize,
    pub candidates_failed: usize,
    pub candidates_no_opportunity: usize,
    pub outbound_checked: usize,
    pub outbound_limit_reached: bool,
    pub candidate_limit_reached: bool,
    pub opportunities_total: usize,
}

/// Top-level run report (`detect --output report.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub schema_version: u32,
    pub tool: &'static str,
    pub tool_version: String,
    pub rule_engine_version: String,
    pub config_file: String,
    pub candidates_file: String,
    pub started_at_utc: String,
    pub finished_at_utc: String,
    /// Complete without any candidate-level error (individual candidate
    /// rejections are NOT failures; they are recorded).
    pub run_status: String,
    pub partial: bool,
    pub network_enabled: bool,
    pub counters: RunCounters,
    /// Where raw captures live (configured evidence_dir).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_dir: Option<String>,
    pub candidates: Vec<CandidateRecord>,
    pub opportunities: Vec<Opportunity>,
    /// Any run-level warnings (non-fatal).
    pub warnings: Vec<String>,
    /// Deliberate honest-zero note when nothing was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zero_result_note: Option<String>,
    /// Free-form audit info (e.g. import transform provenance).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub audit: BTreeMap<String, String>,
}

/// One imported candidate from a provider-neutral search-result file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedCandidate {
    pub url: String,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub collected_at: Option<String>,
    /// Search title/snippet are discovery hints ONLY; stored for provenance,
    /// never used as evidence of page contents.
    #[serde(default)]
    pub search_title: Option<String>,
    #[serde(default)]
    pub search_snippet: Option<String>,
    #[serde(default)]
    pub suggested_target: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DedupeSummary {
    pub input: usize,
    pub unique: usize,
    pub duplicates: Vec<String>,
}
