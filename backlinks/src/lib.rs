//! Backlink opportunities detector: library facade.

#[cfg(test)]
mod competitor_tests;
pub mod competitors;
pub mod detector;
pub mod evidence;
pub mod fetch;
#[cfg(test)]
pub mod fetch_tests;
pub mod gap;
pub mod import;
pub mod neturl;
mod pacing;
pub mod queries;
pub mod report;
mod robots;
pub mod runner;
#[cfg(test)]
pub mod runner_tests;
mod sections;
pub mod text;
pub mod transport;
