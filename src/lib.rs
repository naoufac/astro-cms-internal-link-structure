#![allow(non_snake_case)]
//! Astro CMS internal link structure engine.
//!
//! Library crate backing the CLI binary; the Node bridge spawns the binary
//! with a versioned JSON request on stdin and reads a deterministic JSON
//! report from stdout.

pub mod audit;
pub mod cli;
pub mod gates;
pub mod graph;
pub mod hierarchy;
pub mod links;
pub mod parser;
pub mod request;
pub mod scan;
pub mod sitemap;
