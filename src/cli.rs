//! CLI entry points shared by the binary and the Node bridge.

use crate::audit;

/// Run a full request (JSON string) and return the report.
pub fn run_request(input: &str) -> Result<audit::Report, (String, i32)> {
    let p = audit::prepare(input)?;
    audit::run_prepared(&p).map_err(|e| (e, audit::EXIT_FAILURE))
}
