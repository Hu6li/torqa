//! Signed, encrypted and activated world files (ADR 0043, R278–R281).
//!
//! ORQWA opens only world files the maintainers signed. Sold worlds are encrypted, and a device
//! activated with a licence key opens them. Everything comes from aws-lc-rs, which ORQWA
//! already builds for TLS:
//!
//! - signatures: Ed25519 **and** ML-DSA-65 over the same bytes ([`keys`]), so a signature
//!   holds while either scheme does;
//! - trust: root keys built into ORQWA certify working keys, which can be replaced without an
//!   update, and a root-signed list withdraws keys and world versions ([`trust`]);
//! - files: a signed manifest of every entry's SHA-512 ([`manifest`]);
//! - encryption: AES-256-GCM in chunks, seekable ([`stream`]);
//! - activation: a world's content key sealed for one device ([`activation`], [`device`]).

// A release build trusting the development root would open worlds anyone can sign.
#[cfg(all(feature = "dev-keys", not(debug_assertions)))]
compile_error!("dev-keys is for development builds only: release builds must not trust it");

pub mod activation;
#[cfg(any(test, feature = "dev-keys"))]
pub mod dev;
pub mod device;
pub mod keys;
pub mod manifest;
pub mod stream;
pub mod trust;

/// The name of this set of algorithms, carried by every manifest. A later set gets a new name.
pub const SUITE: &str = "orqwa-1";

/// Why a world file, key or activation was refused.
#[derive(Debug, thiserror::Error)]
pub enum LicenceError {
    /// A signature does not hold for the bytes it covers.
    #[error("The signature does not match: the file was changed or not signed by ORQWA.")]
    BadSignature,
    /// Signed by a key no root of this ORQWA vouches for.
    #[error("Signed by key {0}, which this ORQWA does not trust.")]
    Untrusted(String),
    /// Signed by a withdrawn key.
    #[error("Signed by key {0}, which has been withdrawn.")]
    Revoked(String),
    /// This version of the world has been withdrawn.
    #[error("This version of the world has been withdrawn; please download it again.")]
    WorldWithdrawn,
    /// A key signed something it is not certified for.
    #[error("Key {0} may not sign this.")]
    WrongPurpose(String),
    /// Encrypted data that is damaged, cut short, or sealed with another key.
    #[error("Encrypted data is damaged or belongs to another world.")]
    Decrypt,
    /// An activation that is not for this world, or not for this device.
    #[error("This activation belongs to another device or world.")]
    NotThisDevice,
    /// A protected key file with a wrong password, or damaged.
    #[error("Wrong password, or the key file is damaged.")]
    Password,
    /// The operating system's secure random source failed.
    #[error("Secure random numbers are unavailable.")]
    Random,
    /// Well-formed data that breaks a rule.
    #[error("{0}")]
    Invalid(String),
    /// Reading or writing a file.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON that does not parse.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Fills an array from the operating system's secure random source.
///
/// # Errors
/// [`LicenceError::Random`] instead of a weaker fallback.
pub fn random<const N: usize>() -> Result<[u8; N], LicenceError> {
    use aws_lc_rs::rand::SecureRandom;
    let mut bytes = [0; N];
    aws_lc_rs::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| LicenceError::Random)?;
    Ok(bytes)
}

/// Lowercase hexadecimal.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Bytes from lowercase or uppercase hexadecimal.
///
/// # Errors
/// [`LicenceError::Invalid`] for an odd length or a character that is not a hex digit.
pub fn unhex(text: &str) -> Result<Vec<u8>, LicenceError> {
    let bad = || LicenceError::Invalid(format!("not hexadecimal: {text:.32}"));
    if !text.len().is_multiple_of(2) {
        return Err(bad());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            text.get(i..i + 2)
                .and_then(|d| u8::from_str_radix(d, 16).ok())
                .ok_or_else(bad)
        })
        .collect()
}

/// Seconds since 1970, or 0 on a clock set before it.
#[must_use]
pub fn now_unix_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Binary fields in JSON as standard base64.
pub(crate) mod b64 {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(crate) fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}

/// HKDF output of a chosen length, for keys that are not AEAD keys.
pub(crate) struct Len(pub(crate) usize);

impl aws_lc_rs::hkdf::KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

/// HKDF-SHA-512 of `secret` into `N` bytes.
pub(crate) fn derive<const N: usize>(salt: &[u8], secret: &[u8], info: &[&[u8]]) -> [u8; N] {
    use aws_lc_rs::hkdf::{HKDF_SHA512, Salt};
    let mut out = [0; N];
    Salt::new(HKDF_SHA512, salt)
        .extract(secret)
        .expand(info, Len(N))
        .and_then(|okm| okm.fill(&mut out))
        .expect("HKDF-SHA-512 gives up to 255 × 64 bytes");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let bytes = [0, 1, 0xab, 0xff];
        assert_eq!(hex(&bytes), "0001abff");
        assert_eq!(unhex("0001ABff").unwrap(), bytes);
        assert!(unhex("abc").is_err());
        assert!(unhex("zz").is_err());
    }
}
