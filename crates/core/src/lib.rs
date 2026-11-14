//! Shared types for InLoop: IDs, provenance, configuration and time.
#![forbid(unsafe_code)]

pub mod config;
pub mod id;
pub mod provenance;

pub use config::{PersonalConfig, ProjectConfig};
pub use id::{Id, IdError, IdSpec};
pub use provenance::{Provenance, State};

/// Today's date in the local time zone, as `YYYY-MM-DD`.
pub fn today() -> String {
    jiff::Zoned::now().date().to_string()
}

/// The current time as an RFC 3339 UTC timestamp.
pub fn now() -> jiff::Timestamp {
    jiff::Timestamp::now()
}
