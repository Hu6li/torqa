//! Hybrid keys: every signature is an Ed25519 and an ML-DSA-65 signature over the same bytes,
//! and both must hold. Ed25519 guards against a flaw in the young ML-DSA, ML-DSA against
//! quantum computers.

use std::num::NonZeroU32;

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use aws_lc_rs::signature::{
    ED25519, Ed25519KeyPair, KeyPair as _, ML_DSA_65, ML_DSA_65_SIGNING, PqdsaKeyPair,
    UnparsedPublicKey,
};
use serde::{Deserialize, Serialize};

use crate::{LicenceError, SUITE, b64, hex, random};

/// Both seeds: Ed25519's, then ML-DSA-65's, 32 bytes each.
const SEEDS: usize = 64;

/// What a signature is for. Signing it into the message keeps a signature on one kind of
/// document from passing for another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    /// A root certifying a working key.
    Certificate,
    /// A root withdrawing keys and worlds.
    Revocations,
    /// A world key signing a world's manifest.
    Manifest,
    /// The activation key signing an activation.
    Activation,
}

impl Context {
    fn label(self) -> &'static str {
        match self {
            Self::Certificate => "certificate",
            Self::Revocations => "revocations",
            Self::Manifest => "manifest",
            Self::Activation => "activation",
        }
    }

    fn message(self, bytes: &[u8]) -> Vec<u8> {
        let mut message = format!("ORQWA {SUITE} {}\0", self.label()).into_bytes();
        message.extend_from_slice(bytes);
        message
    }
}

/// A key's id: the first 16 bytes of the SHA-512 of both public keys, in hex.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyId(pub String);

impl std::fmt::Display for KeyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The public half of a hybrid key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicKey {
    /// Ed25519, 32 bytes.
    #[serde(with = "b64")]
    pub ed25519: Vec<u8>,
    /// ML-DSA-65, raw.
    #[serde(with = "b64")]
    pub ml_dsa: Vec<u8>,
}

impl PublicKey {
    /// This key's id.
    #[must_use]
    pub fn id(&self) -> KeyId {
        let mut context = aws_lc_rs::digest::Context::new(&aws_lc_rs::digest::SHA512);
        context.update(format!("ORQWA {SUITE} key\0").as_bytes());
        context.update(&self.ed25519);
        context.update(&self.ml_dsa);
        KeyId(hex(&context.finish().as_ref()[..16]))
    }

    /// Checks both signatures over `bytes` signed for `context`.
    ///
    /// # Errors
    /// [`LicenceError::BadSignature`] unless both hold.
    pub fn verify(
        &self,
        context: Context,
        bytes: &[u8],
        signature: &Signature,
    ) -> Result<(), LicenceError> {
        let message = context.message(bytes);
        let ed =
            UnparsedPublicKey::new(&ED25519, &self.ed25519).verify(&message, &signature.ed25519);
        let ml =
            UnparsedPublicKey::new(&ML_DSA_65, &self.ml_dsa).verify(&message, &signature.ml_dsa);
        if ed.is_ok() && ml.is_ok() {
            Ok(())
        } else {
            Err(LicenceError::BadSignature)
        }
    }
}

/// An Ed25519 and an ML-DSA-65 signature over the same message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    /// Ed25519, 64 bytes.
    #[serde(with = "b64")]
    pub ed25519: Vec<u8>,
    /// ML-DSA-65.
    #[serde(with = "b64")]
    pub ml_dsa: Vec<u8>,
}

/// A hybrid key pair, made from two 32-byte seeds.
pub struct KeyPair {
    seeds: [u8; SEEDS],
    ed25519: Ed25519KeyPair,
    ml_dsa: PqdsaKeyPair,
    public: PublicKey,
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair")
            .field("id", &self.public.id())
            .finish_non_exhaustive()
    }
}

impl KeyPair {
    /// A new key from the secure random source.
    ///
    /// # Errors
    /// [`LicenceError::Random`] if no secure random bytes are available.
    pub fn generate() -> Result<Self, LicenceError> {
        Self::from_seeds(&random()?)
    }

    /// The key these seeds make.
    ///
    /// # Errors
    /// [`LicenceError::Invalid`] if aws-lc rejects a seed.
    pub fn from_seeds(seeds: &[u8; SEEDS]) -> Result<Self, LicenceError> {
        let rejected = |e| LicenceError::Invalid(format!("unusable key seed: {e}"));
        let ed25519 = Ed25519KeyPair::from_seed_unchecked(&seeds[..32]).map_err(rejected)?;
        let ml_dsa = PqdsaKeyPair::from_seed(&ML_DSA_65_SIGNING, &seeds[32..]).map_err(rejected)?;
        let public = PublicKey {
            ed25519: ed25519.public_key().as_ref().to_vec(),
            ml_dsa: ml_dsa.public_key().as_ref().to_vec(),
        };
        Ok(Self {
            seeds: *seeds,
            ed25519,
            ml_dsa,
            public,
        })
    }

    /// The public half.
    #[must_use]
    pub fn public(&self) -> &PublicKey {
        &self.public
    }

    /// Signs `bytes` for `context` with both schemes.
    ///
    /// # Panics
    /// Never: aws-lc signs into a buffer of the scheme's own signature length.
    #[must_use]
    pub fn sign(&self, context: Context, bytes: &[u8]) -> Signature {
        let message = context.message(bytes);
        let mut ml_dsa = vec![0; ML_DSA_65_SIGNING.signature_len()];
        let len = self
            .ml_dsa
            .sign(&message, &mut ml_dsa)
            .expect("ML-DSA signs any message");
        ml_dsa.truncate(len);
        Signature {
            ed25519: self.ed25519.sign(&message).as_ref().to_vec(),
            ml_dsa,
        }
    }

    /// The key as a JSON file encrypted with `password` (PBKDF2-HMAC-SHA-512, AES-256-GCM).
    /// Private keys are kept only like this.
    ///
    /// # Errors
    /// [`LicenceError::Random`] if no secure random bytes are available.
    pub fn protect(&self, password: &str) -> Result<String, LicenceError> {
        let salt: [u8; 32] = random()?;
        let nonce: [u8; NONCE_LEN] = random()?;
        let mut sealed = self.seeds.to_vec();
        password_key(password, &salt, ITERATIONS)
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(PROTECTED),
                &mut sealed,
            )
            .map_err(|_| LicenceError::Invalid("could not seal the key".into()))?;
        let file = ProtectedKey {
            format: PROTECTED.to_owned(),
            iterations: ITERATIONS.get(),
            salt: salt.to_vec(),
            nonce: nonce.to_vec(),
            sealed,
            public: self.public.clone(),
        };
        Ok(serde_json::to_string_pretty(&file)?)
    }

    /// A key from a file [`KeyPair::protect`] wrote.
    ///
    /// # Errors
    /// [`LicenceError::Password`] for a wrong password or a changed file.
    pub fn unprotect(text: &str, password: &str) -> Result<Self, LicenceError> {
        let file: ProtectedKey = serde_json::from_str(text)?;
        let iterations = NonZeroU32::new(file.iterations)
            .filter(|_| file.format == PROTECTED)
            .ok_or_else(|| LicenceError::Invalid("not an ORQWA key file".into()))?;
        let nonce =
            Nonce::try_assume_unique_for_key(&file.nonce).map_err(|_| LicenceError::Password)?;
        let mut sealed = file.sealed;
        let seeds: [u8; SEEDS] = password_key(password, &file.salt, iterations)
            .open_in_place(nonce, Aad::from(PROTECTED), &mut sealed)
            .ok()
            .and_then(|seeds| <[u8; SEEDS]>::try_from(&*seeds).ok())
            .ok_or(LicenceError::Password)?;
        let key = Self::from_seeds(&seeds)?;
        if key.public != file.public {
            return Err(LicenceError::Password);
        }
        Ok(key)
    }
}

const PROTECTED: &str = "orqwa-key-1";
const ITERATIONS: NonZeroU32 = NonZeroU32::new(600_000).expect("not zero");

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtectedKey {
    format: String,
    iterations: u32,
    #[serde(with = "b64")]
    salt: Vec<u8>,
    #[serde(with = "b64")]
    nonce: Vec<u8>,
    #[serde(with = "b64")]
    sealed: Vec<u8>,
    public: PublicKey,
}

fn password_key(password: &str, salt: &[u8], iterations: NonZeroU32) -> LessSafeKey {
    let mut key = [0; 32];
    aws_lc_rs::pbkdf2::derive(
        aws_lc_rs::pbkdf2::PBKDF2_HMAC_SHA512,
        iterations,
        salt,
        password.as_bytes(),
        &mut key,
    );
    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &key).expect("32 bytes for AES-256"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_signatures_must_hold() {
        let key = KeyPair::generate().unwrap();
        let other = KeyPair::generate().unwrap();
        let signature = key.sign(Context::Manifest, b"world");
        key.public()
            .verify(Context::Manifest, b"world", &signature)
            .unwrap();

        let foreign = other.sign(Context::Manifest, b"world");
        let half_ed = Signature {
            ed25519: signature.ed25519.clone(),
            ml_dsa: foreign.ml_dsa,
        };
        let half_ml = Signature {
            ed25519: foreign.ed25519,
            ml_dsa: signature.ml_dsa.clone(),
        };
        for signature in [half_ed, half_ml] {
            assert!(matches!(
                key.public().verify(Context::Manifest, b"world", &signature),
                Err(LicenceError::BadSignature)
            ));
        }
    }

    #[test]
    fn a_signature_holds_only_for_its_bytes_and_context() {
        let key = KeyPair::generate().unwrap();
        let signature = key.sign(Context::Manifest, b"world");
        assert!(
            key.public()
                .verify(Context::Manifest, b"World", &signature)
                .is_err()
        );
        assert!(
            key.public()
                .verify(Context::Activation, b"world", &signature)
                .is_err()
        );
    }

    #[test]
    fn the_same_seeds_make_the_same_key() {
        let seeds = [7; SEEDS];
        let a = KeyPair::from_seeds(&seeds).unwrap();
        let b = KeyPair::from_seeds(&seeds).unwrap();
        assert_eq!(a.public(), b.public());
        assert_eq!(a.public().id(), b.public().id());
        assert_eq!(a.public().id().0.len(), 32);
        assert_ne!(a.public().id(), KeyPair::generate().unwrap().public().id());
    }

    #[test]
    fn a_protected_key_opens_only_with_its_password() {
        let key = KeyPair::generate().unwrap();
        let file = key.protect("correct horse").unwrap();
        assert_eq!(
            KeyPair::unprotect(&file, "correct horse").unwrap().public(),
            key.public()
        );
        assert!(matches!(
            KeyPair::unprotect(&file, "wrong"),
            Err(LicenceError::Password)
        ));
    }
}
