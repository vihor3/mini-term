//! Transport-free GitHub Issues and Pull Requests domain layer.
//!
//! This crate owns only normalized identities, structured command plans,
//! bounded parsers, and stable error classification. It never spawns a
//! process, reads application configuration, opens a browser, or handles
//! credentials.

mod commands;
mod error;
mod model;
mod remote;

pub use commands::*;
pub use error::*;
pub use model::*;
pub use remote::*;
