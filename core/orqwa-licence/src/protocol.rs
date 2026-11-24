//! What ORQWA and the activation server send each other, as JSON over HTTPS (ADR 0043). The
//! server is not part of ORQWA; it shares these types so both read the same messages.

use serde::{Deserialize, Serialize};

use crate::activation::ActivationFile;
use crate::b64;
use crate::trust::Signed;

/// `POST /v1/activate`: activate a world on this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivateRequest {
    /// The licence key from the purchase, as the rider typed it.
    pub licence_key: String,
    /// The world's id.
    pub world: String,
    /// The device's random id.
    pub device: String,
    /// The device key the content key is sealed for. The server keeps only its hash, to refuse
    /// the same device id with another key.
    #[serde(with = "b64")]
    pub device_key: Vec<u8>,
}

/// The answer to an activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivateResponse {
    /// The activation to keep on the device.
    pub activation: ActivationFile,
    /// The newest withdrawals, signed by a root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocations: Option<Signed>,
}

/// `POST /v1/release`: free this device's slot in every licence it holds one in. The device
/// proves itself with its key; the licence key is not stored, so it is not needed. ORQWA then
/// starts a new device identity, so an activation kept aside opens nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRequest {
    /// The device's id.
    pub device: String,
    /// The device's key.
    #[serde(with = "b64")]
    pub device_key: Vec<u8>,
}

/// Why the server refused, with any status other than success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// The licence key is unknown, withdrawn or not for this world.
    Licence,
    /// The licence is active on as many devices as it allows.
    DeviceLimit,
    /// This device id is active with another key: another computer.
    OtherDevice,
    /// Release: the server knows no such device.
    UnknownDevice,
    /// Release: a licence on this device has released as many devices as it may for now.
    ReleaseLimit,
    /// Too many attempts; try later.
    TooMany,
}

/// The body of a refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refused {
    /// Why.
    pub refusal: Refusal,
}
