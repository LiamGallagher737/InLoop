//! InLoop's operations, shared by the CLI and the MCP server so people and
//! agents get the same data (D-C21Z).
#![forbid(unsafe_code)]

pub mod decisions;
pub mod hooks;
pub mod json;
pub mod pm;
pub mod project;
pub mod scan;
pub mod sessions;

pub use project::{AppError, Project};
