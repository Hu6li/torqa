//! A world file's `manifest.json` lists every other entry with its SHA-512, and
//! `signature.json` signs the manifest's exact bytes with a certified world key. ORQWA checks
//! the signature before it reads anything else, then each entry against its hash as it reads it.

use std::collections::BTreeMap;
use std::io::{self, Read};

use serde::{Deserialize, Serialize};

use crate::keys::{Context, KeyId, KeyPair, Signature};
use crate::trust::{Purpose, Signed, Trust};
use crate::{LicenceError, SUITE, hex, unhex};

/// The manifest's entry name.
pub const MANIFEST: &str = "manifest.json";
/// The signature's entry name.
pub const SIGNATURE: &str = "signature.json";

/// What `manifest.json` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// [`SUITE`].
    pub suite: String,
    /// The world's id, as in `world.json`.
    pub world: String,
    /// The world's version, as in `world.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Every entry but the manifest and the signature.
    pub entries: BTreeMap<String, Entry>,
}

/// One entry of the archive, as the archive hands it out (inflated).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Bytes.
    pub size: u64,
    /// SHA-512, hex.
    pub sha512: String,
}

impl Entry {
    /// Whether these bytes, hashed with [`Hashing`], are this entry's.
    #[must_use]
    pub fn matches(&self, digest: &[u8], size: u64) -> bool {
        size == self.size
            && unhex(&self.sha512).is_ok_and(|expected| {
                aws_lc_rs::constant_time::verify_slices_are_equal(&expected, digest).is_ok()
            })
    }
}

/// What `signature.json` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldSignature {
    /// A root's certificate for the world key.
    pub certificate: Signed,
    /// The world key's signature over `manifest.json`'s bytes.
    pub signature: Signature,
}

/// A manifest whose signature held.
#[derive(Debug, Clone)]
pub struct Verified {
    /// The manifest.
    pub manifest: Manifest,
    /// The world key that signed it.
    pub signer: KeyId,
    /// The certificate's name for that key.
    pub signer_name: String,
}

/// Signs a manifest's bytes with a world key and its certificate; returns `signature.json`.
///
/// # Errors
/// [`LicenceError::Json`] only in theory.
pub fn sign(manifest: &[u8], key: &KeyPair, certificate: Signed) -> Result<Vec<u8>, LicenceError> {
    let signature = WorldSignature {
        certificate,
        signature: key.sign(Context::Manifest, manifest),
    };
    Ok(serde_json::to_vec_pretty(&signature)?)
}

/// Checks `signature.json` over `manifest.json` against `trust`, then reads the manifest.
///
/// # Errors
/// If the signature, its certificate or the manifest is refused, or the world is withdrawn.
pub fn verify(manifest: &[u8], signature: &[u8], trust: &Trust) -> Result<Verified, LicenceError> {
    let signature: WorldSignature = serde_json::from_slice(signature)?;
    let certificate = trust.certificate(&signature.certificate, Purpose::World)?;
    certificate
        .key
        .verify(Context::Manifest, manifest, &signature.signature)?;
    let manifest: Manifest = serde_json::from_slice(manifest)?;
    if manifest.suite != SUITE {
        return Err(LicenceError::Invalid(format!(
            "signed with suite {}, which this ORQWA does not know",
            manifest.suite
        )));
    }
    if trust
        .revocations()
        .world(&manifest.world, manifest.version.as_deref())
    {
        return Err(LicenceError::WorldWithdrawn);
    }
    Ok(Verified {
        manifest,
        signer: certificate.key.id(),
        signer_name: certificate.name,
    })
}

/// The SHA-512 of everything read through it.
pub struct Hashing<R> {
    inner: R,
    context: aws_lc_rs::digest::Context,
    read: u64,
}

impl<R: Read> Hashing<R> {
    /// Hashes what is read from `inner`.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            context: aws_lc_rs::digest::Context::new(&aws_lc_rs::digest::SHA512),
            read: 0,
        }
    }

    /// Reads the rest, then gives the hash and the number of bytes read in all.
    ///
    /// # Errors
    /// A read error.
    pub fn finish(mut self) -> io::Result<(Vec<u8>, u64)> {
        io::copy(&mut self, &mut io::sink())?;
        Ok((self.context.finish().as_ref().to_vec(), self.read))
    }
}

impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.context.update(&buf[..n]);
        self.read += n as u64;
        Ok(n)
    }
}

/// The SHA-512 of everything written through it, passed on to `inner`.
pub struct HashingWriter<W> {
    inner: W,
    context: aws_lc_rs::digest::Context,
    written: u64,
}

impl<W: io::Write> HashingWriter<W> {
    /// Hashes what is written to `inner`.
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            context: aws_lc_rs::digest::Context::new(&aws_lc_rs::digest::SHA512),
            written: 0,
        }
    }

    /// The [`Entry`] for what was written.
    #[must_use]
    pub fn entry(self) -> Entry {
        Entry {
            size: self.written,
            sha512: hex(self.context.finish().as_ref()),
        }
    }
}

impl<W: io::Write> io::Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.context.update(&buf[..n]);
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// An [`Entry`] for these bytes.
#[must_use]
pub fn entry(bytes: &[u8]) -> Entry {
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA512, bytes);
    Entry {
        size: bytes.len() as u64,
        sha512: hex(digest.as_ref()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev;
    use crate::trust::{Revocations, WithdrawnWorld, certify};

    fn manifest() -> Vec<u8> {
        let mut entries = BTreeMap::new();
        entries.insert("world.json".to_owned(), entry(b"{}"));
        let manifest = Manifest {
            suite: SUITE.into(),
            world: "w".into(),
            version: Some("1".into()),
            entries,
        };
        serde_json::to_vec_pretty(&manifest).unwrap()
    }

    #[test]
    fn a_signed_manifest_verifies_and_names_its_signer() {
        let bytes = manifest();
        let signature = sign(&bytes, &dev::world_key(), dev::world_certificate()).unwrap();
        let verified = verify(&bytes, &signature, &Trust::builtin()).unwrap();
        assert_eq!(verified.signer, dev::world_key().public().id());
        assert_eq!(verified.manifest.world, "w");
    }

    #[test]
    fn a_changed_manifest_is_refused() {
        let bytes = manifest();
        let signature = sign(&bytes, &dev::world_key(), dev::world_certificate()).unwrap();
        let mut changed = bytes.clone();
        let at = changed.len() - 3;
        changed[at] ^= 1;
        assert!(matches!(
            verify(&changed, &signature, &Trust::builtin()),
            Err(LicenceError::BadSignature)
        ));
    }

    #[test]
    fn a_world_key_certified_by_an_unknown_root_is_refused() {
        let bytes = manifest();
        let own_root = KeyPair::generate().unwrap();
        let certificate =
            certify(&own_root, dev::world_key().public(), Purpose::World, "x").unwrap();
        let signature = sign(&bytes, &dev::world_key(), certificate).unwrap();
        assert!(matches!(
            verify(&bytes, &signature, &Trust::builtin()),
            Err(LicenceError::Untrusted(_))
        ));
    }

    #[test]
    fn a_withdrawn_world_or_key_is_refused() {
        let bytes = manifest();
        let signature = sign(&bytes, &dev::world_key(), dev::world_certificate()).unwrap();
        let roots = vec![dev::root().public().clone()];
        let world = Revocations {
            sequence: 1,
            keys: vec![],
            worlds: vec![WithdrawnWorld {
                id: "w".into(),
                version: Some("1".into()),
            }],
        };
        assert!(matches!(
            verify(&bytes, &signature, &Trust::new(roots.clone(), world)),
            Err(LicenceError::WorldWithdrawn)
        ));
        let key = Revocations {
            sequence: 1,
            keys: vec![dev::world_key().public().id()],
            worlds: vec![],
        };
        assert!(matches!(
            verify(&bytes, &signature, &Trust::new(roots, key)),
            Err(LicenceError::Revoked(_))
        ));
    }

    #[test]
    fn hashing_covers_every_byte_read() {
        let data = vec![5u8; 200_000];
        let expected = entry(&data);
        let mut reader = Hashing::new(&data[..]);
        let mut first = [0; 10];
        reader.read_exact(&mut first).unwrap();
        let (digest, size) = reader.finish().unwrap();
        assert!(expected.matches(&digest, size));
        assert!(!expected.matches(&digest, size - 1));
        assert!(!entry(&data[1..]).matches(&digest, size));
    }
}
