//! Backlink opportunities detector: library facade.

pub mod detector;
pub mod evidence;
pub mod fetch;
#[cfg(test)]
pub mod fetch_tests;
pub mod import;
pub mod neturl;
pub mod queries;
pub mod report;
pub mod runner;
#[cfg(test)]
pub mod runner_tests;
pub mod text;
pub mod transport;
