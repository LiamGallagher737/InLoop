#![forbid(unsafe_code)]
//! Decision files (docs/02).

pub mod lint;
pub mod model;
pub mod ops;
pub mod sources;
pub use model::{Decision, ParseError, Status};
pub use sources::{DecisionSet, Entry, ScanOptions, scan};
