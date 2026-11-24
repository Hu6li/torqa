//! The development root and its working keys. Their seeds are in this source, so anyone can
//! sign with them: only builds with `dev-keys` (tests, the fuzzer, worlds being built) trust
//! them, never a release (ADR 0043).

use crate::keys::KeyPair;
use crate::trust::{Purpose, Signed, certify};

fn key(name: &str) -> KeyPair {
    let digest = aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA512,
        format!("ORQWA development {name}").as_bytes(),
    );
    let seeds = digest.as_ref().try_into().expect("SHA-512 has 64 bytes");
    KeyPair::from_seeds(&seeds).expect("fixed seeds make a key")
}

/// The development root.
#[must_use]
pub fn root() -> KeyPair {
    key("root")
}

/// The development world key.
#[must_use]
pub fn world_key() -> KeyPair {
    key("world key")
}

/// The development root's certificate for [`world_key`].
///
/// # Panics
/// Never: a certificate always serializes.
#[must_use]
pub fn world_certificate() -> Signed {
    certify(
        &root(),
        world_key().public(),
        Purpose::World,
        "development world key",
    )
    .expect("a certificate serializes")
}
