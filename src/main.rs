//! Standalone binary: reads a versioned JSON request on stdin, writes a
//! deterministic JSON report on stdout. Exit codes:
//!   0 = success (audit mode), 1 = gate violations (enforce mode),
//!   2 = configuration or runtime failure.

use std::io::Read;

fn main() {
    let code = run();
    std::process::exit(code);
}

fn run() -> i32 {
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("engine: cannot read stdin: {e}");
        return 2;
    }
    match astro_cms_internal_link_structure::cli::run_request(&input) {
        Ok(report) => {
            let mode = report.gates.mode.clone();
            let violated = report.gates.violated;
            let enforce = mode == "enforce";
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("serialize")
            );
            if enforce && violated {
                1
            } else {
                0
            }
        }
        Err((msg, code)) => {
            eprintln!("engine: {msg}");
            code
        }
    }
}
