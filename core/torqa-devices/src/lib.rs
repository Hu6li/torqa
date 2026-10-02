//! Trainer and sensor drivers for Torqa.
//!
//! Every device — Bluetooth trainer, heart-rate strap or the fake trainer — is driven by a
//! background task and exposed through the same [`DeviceHandle`], so callers never depend on a
//! concrete driver.

pub mod ble;
mod bytes;
pub mod fake;
pub mod ftms;
mod handle;
pub mod heart_rate;

pub use bytes::ParseError;
pub use handle::{DeviceError, DeviceEvent, DeviceHandle};
