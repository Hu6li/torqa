//! Core domain types and plugin interfaces of Torqa.

pub mod files;
pub mod profile;
pub mod recording;
pub mod shifting;
pub mod telemetry;
pub mod units;
pub mod workout;

/// Human-readable application name.
pub const APP_NAME: &str = "Torqa";

/// Version of the Torqa core.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
