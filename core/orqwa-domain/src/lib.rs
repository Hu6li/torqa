//! Core domain types and plugin interfaces of ORQWA.

pub mod battery;
pub mod files;
pub mod profile;
pub mod recording;
pub mod season;
pub mod shifting;
pub mod telemetry;
pub mod units;
pub mod upload;
pub mod voice;
pub mod workout;

/// Human-readable application name.
pub const APP_NAME: &str = "ORQWA";

/// Version of the ORQWA core.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
