//! Core domain types and plugin interfaces of Torqa.

/// Human-readable application name.
pub const APP_NAME: &str = "Torqa";

/// Version of the Torqa core.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
