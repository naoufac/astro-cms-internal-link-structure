//! CLI: JSON-in, JSON-out, native argument handling (no shell).

use std::collections::HashMap;
use std::process::ExitCode;

use backlink_opportunities_detector::{
    competitors::parse_backlinks, import::parse_candidates, queries::generate_queries,
    report::DetectorConfig, runner::run_detection, transport::PinnedReqwestTransport,
};

const USAGE: &str = r#"backlink-opportunities-detector: evidence-backed backlink opportunity detection

SUBCOMMANDS:
  generate-queries --config <config.json> [--output <queries.json>]
      Generate documented search-query suggestions from configured brands and
      topics. Execute them in your OWN search tool; nothing is fetched here.

  detect --config <config.json> --candidates <results.json> --output <report.json>
  detect --config <config.json> --backlinks <backlinks.json> --output <report.json>
      Run the detector over imported candidates. Network is opt-in via
      config.network.enabled; evidence captures go to config.network.evidence_dir.

Exit codes: 0 = report written (individual exclusions are part of the report);
2 = usage error; 1 = hard failure (unreadable/invalid inputs).
"#;

fn parse_args(args: &[String]) -> Result<(String, HashMap<String, String>), String> {
    let mut it = args.iter();
    let sub = it
        .next()
        .ok_or_else(|| format!("missing subcommand\n\n{USAGE}"))?
        .clone();
    if sub == "--help" || sub == "-h" || sub == "help" {
        return Err(format!("help\n\n{USAGE}"));
    }
    let allowed: &[&str] = match sub.as_str() {
        "generate-queries" => &["config", "output"],
        "detect" => &["config", "candidates", "backlinks", "output"],
        _ => return Err(format!("unknown subcommand '{sub}'\n\n{USAGE}")),
    };
    let mut map = HashMap::new();
    while let Some(a) = it.next() {
        let key = a.trim_start_matches("--").to_string();
        if *a == key {
            return Err(format!("unexpected positional argument '{a}'\n\n{USAGE}"));
        }
        if !allowed.contains(&key.as_str()) {
            return Err(format!("unknown flag '{a}'\n\n{USAGE}"));
        }
        if map.contains_key(&key) {
            return Err(format!("duplicate flag '{a}'\n\n{USAGE}"));
        }
        let val = it
            .next()
            .ok_or_else(|| format!("missing value for --{key}\n\n{USAGE}"))?;
        if val.starts_with("--") || val.is_empty() {
            return Err(format!("missing value for --{key}\n\n{USAGE}"));
        }
        map.insert(key, val.clone());
    }
    if !map.contains_key("config") {
        return Err(format!("--config <config.json> is required\n\n{USAGE}"));
    }
    if sub == "detect" {
        if !map.contains_key("output") {
            return Err(format!("--output <report.json> is required\n\n{USAGE}"));
        }
        if map.contains_key("candidates") == map.contains_key("backlinks") {
            return Err(format!(
                "Supply exactly one of --candidates or --backlinks\n\n{USAGE}"
            ));
        }
    }
    Ok((sub, map))
}

fn write_new_output(path: &str, content: &str) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            format!("cannot create new output '{path}' (existing files are never overwritten): {e}")
        })?;
    file.write_all(content.as_bytes())
        .map_err(|e| format!("cannot write '{path}': {e}"))
}

fn read_config(path: &str) -> Result<DetectorConfig, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read config '{path}': {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("invalid config JSON in '{path}': {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (sub, opts) = match parse_args(&args) {
        Ok(x) => x,
        Err(msg) => {
            eprintln!("{msg}");
            return if msg.starts_with("help") {
                ExitCode::from(0)
            } else {
                ExitCode::from(2)
            };
        }
    };

    let result = match sub.as_str() {
        "generate-queries" => run_generate_queries(&opts),
        "detect" => run_detect(&opts),
        other => Err(format!("unknown subcommand '{other}'\n\n{USAGE}")),
    };

    match result {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::from(1)
        }
    }
}

fn run_generate_queries(opts: &HashMap<String, String>) -> Result<ExitCode, String> {
    let cfg_path = opts
        .get("config")
        .ok_or_else(|| "--config <config.json> is required".to_string())?;
    let config = read_config(cfg_path)?;
    let out = generate_queries(&config);
    let json = serde_json::to_string_pretty(&out).map_err(|e| format!("serialize queries: {e}"))?;
    match opts.get("output") {
        Some(path) => {
            if let Some(parent) = std::path::Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        format!("cannot create output dir '{}': {e}", parent.display())
                    })?;
                }
            }
            write_new_output(path, &(json + "\n"))?;
            println!("queries written: {path}");
        }
        None => println!("{json}"),
    }
    Ok(ExitCode::from(0))
}

fn run_detect(opts: &HashMap<String, String>) -> Result<ExitCode, String> {
    let cfg_path = opts
        .get("config")
        .ok_or_else(|| "--config <config.json> is required".to_string())?;
    if opts.contains_key("candidates") == opts.contains_key("backlinks") {
        return Err("Supply exactly one of --candidates or --backlinks".into());
    }
    let cand_path = opts
        .get("candidates")
        .or_else(|| opts.get("backlinks"))
        .unwrap();
    let out_path = opts
        .get("output")
        .ok_or_else(|| "--output <report.json> is required".to_string())?;
    let config = read_config(cfg_path)?;

    let text = std::fs::read_to_string(cand_path)
        .map_err(|e| format!("cannot read candidates '{cand_path}': {e}"))?;
    let (candidates, import_summary) = if opts.contains_key("backlinks") {
        (
            parse_backlinks(&text, &config).map_err(|e| format!("invalid backlink import: {e}"))?,
            None,
        )
    } else {
        let (candidates, summary) =
            parse_candidates(&text).map_err(|e| format!("invalid candidates file: {e}"))?;
        (candidates, Some(summary))
    };
    if candidates.is_empty() {
        eprintln!("no usable candidates in '{cand_path}'; writing honest empty report");
    }

    // Network is strictly opt-in. Without it, detection cannot fetch and the
    // run reports that honestly instead of pretending.
    if !config.network.enabled {
        return Err(
            "config.network.enabled is false: detect performs no network I/O. \
             Set network.enabled=true to run live detection."
                .to_string(),
        );
    }
    let transport = PinnedReqwestTransport::with_body_limit(
        config.limits.timeout_seconds,
        config.limits.max_body_bytes,
    )
    .map_err(|e| format!("cannot build transport: {e}"))?;

    let mut report = run_detection(&config, &candidates, cfg_path, cand_path, &transport)
        .map_err(|e| format!("detection failed: {e}"))?;
    if let Some(summary) = import_summary {
        report.counters.candidates_input = summary.input;
        let removed = report
            .audit
            .get("duplicates_removed")
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0);
        report.audit.insert(
            "duplicates_removed".into(),
            (removed + summary.duplicates.len()).to_string(),
        );
        report
            .audit
            .insert("import_unique".into(), summary.unique.to_string());
    }

    let json =
        serde_json::to_string_pretty(&report).map_err(|e| format!("serialize report: {e}"))?;
    if let Some(parent) = std::path::Path::new(out_path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create output dir '{}': {e}", parent.display()))?;
        }
    }
    write_new_output(out_path, &(json + "\n"))?;
    println!(
        "report written: {out_path} (opportunities: {}, candidates fetched: {}, rejected: {})",
        report.opportunities.len(),
        report.counters.candidates_fetched,
        report.counters.candidates_rejected
    );
    Ok(ExitCode::from(0))
}
