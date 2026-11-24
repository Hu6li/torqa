//! Which keys ORQWA trusts. Roots are built in; a root certifies working keys (one that seals
//! worlds, one that signs activations), so those can be replaced without an update. A
//! root-signed, numbered list withdraws keys and world versions.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::LicenceError;
use crate::keys::{Context, KeyId, KeyPair, PublicKey, Signature};

/// What a working key may sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Purpose {
    /// Seals world files.
    World,
    /// Signs activations on the activation server.
    Activation,
}

/// A root's statement that a key may sign for a purpose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Certificate {
    /// What the key may sign.
    pub purpose: Purpose,
    /// A name for people, such as "world key 2026-11".
    pub name: String,
    /// The certified key.
    pub key: PublicKey,
    /// When the root signed it. Not checked against the clock: sold worlds must keep opening,
    /// so keys end by withdrawal, not by date.
    pub issued_unix_s: u64,
}

/// A JSON document and the signature over its exact text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signed {
    /// The document's JSON, signed as it is.
    pub body: String,
    /// The key that signed it.
    pub by: KeyId,
    /// Over `body`'s bytes.
    pub signature: Signature,
}

impl Signed {
    /// Signs `value` with `key` for `context`.
    ///
    /// # Errors
    /// [`LicenceError::Json`] if `value` does not serialize.
    pub fn sign<T: Serialize>(
        value: &T,
        context: Context,
        key: &KeyPair,
    ) -> Result<Self, LicenceError> {
        let body = serde_json::to_string(value)?;
        let signature = key.sign(context, body.as_bytes());
        Ok(Self {
            body,
            by: key.public().id(),
            signature,
        })
    }

    /// The document, if `key` signed it for `context`.
    ///
    /// # Errors
    /// [`LicenceError::BadSignature`] if `key` did not sign it, or a JSON error.
    pub fn open<T: DeserializeOwned>(
        &self,
        context: Context,
        key: &PublicKey,
    ) -> Result<T, LicenceError> {
        if self.by != key.id() {
            return Err(LicenceError::BadSignature);
        }
        key.verify(context, self.body.as_bytes(), &self.signature)?;
        Ok(serde_json::from_str(&self.body)?)
    }
}

/// A world version that may no longer be opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WithdrawnWorld {
    /// The world's id.
    pub id: String,
    /// The version withdrawn; every version without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Withdrawn keys and worlds. A higher `sequence` replaces a lower one, never the reverse.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revocations {
    /// Counts up with every new list.
    pub sequence: u64,
    /// Withdrawn keys, roots included.
    #[serde(default)]
    pub keys: Vec<KeyId>,
    /// Withdrawn worlds.
    #[serde(default)]
    pub worlds: Vec<WithdrawnWorld>,
}

impl Revocations {
    /// Whether `key` is withdrawn.
    #[must_use]
    pub fn key(&self, key: &KeyId) -> bool {
        self.keys.contains(key)
    }

    /// Whether this version of the world is withdrawn.
    #[must_use]
    pub fn world(&self, id: &str, version: Option<&str>) -> bool {
        self.worlds
            .iter()
            .any(|w| w.id == id && (w.version.is_none() || w.version.as_deref() == version))
    }
}

/// The roots ORQWA trusts and the newest withdrawals it knows.
#[derive(Debug, Clone)]
pub struct Trust {
    roots: Vec<PublicKey>,
    revocations: Revocations,
}

impl Trust {
    /// The roots and withdrawals built into this ORQWA, with the development root when built
    /// with `dev-keys`.
    ///
    /// # Panics
    /// If the built-in roots or list do not parse, which tests rule out.
    #[must_use]
    pub fn builtin() -> Self {
        #[allow(unused_mut)] // without dev-keys, nothing is added
        let mut roots: Vec<PublicKey> = RELEASE_ROOTS
            .iter()
            .map(|text| serde_json::from_str(text).expect("built-in root parses"))
            .collect();
        #[cfg(any(test, feature = "dev-keys"))]
        roots.push(crate::dev::root().public().clone());
        let revocations = serde_json::from_str(BUILTIN_REVOCATIONS).expect("built-in list parses");
        Self { roots, revocations }
    }

    /// Trusts exactly these roots.
    #[must_use]
    pub fn new(roots: Vec<PublicKey>, revocations: Revocations) -> Self {
        Self { roots, revocations }
    }

    /// Trusts `root` as well, for a root not yet built into ORQWA (the maintainers' tools).
    #[must_use]
    pub fn with_root(mut self, root: PublicKey) -> Self {
        if !self.roots.contains(&root) {
            self.roots.push(root);
        }
        self
    }

    /// The withdrawals in force.
    #[must_use]
    pub fn revocations(&self) -> &Revocations {
        &self.revocations
    }

    /// Takes a list a root signed if it is newer than the one in force. Returns whether it
    /// was taken, so the caller keeps it.
    ///
    /// # Errors
    /// If no trusted root signed it.
    pub fn accept_revocations(&mut self, signed: &Signed) -> Result<bool, LicenceError> {
        let root = self.root(&signed.by)?;
        let list: Revocations = signed.open(Context::Revocations, root)?;
        if list.sequence <= self.revocations.sequence {
            return Ok(false);
        }
        self.revocations = list;
        Ok(true)
    }

    /// The certificate, if a trusted root signed it, for `purpose`, and neither is withdrawn.
    ///
    /// # Errors
    /// [`LicenceError::Untrusted`], [`LicenceError::Revoked`], [`LicenceError::WrongPurpose`]
    /// or [`LicenceError::BadSignature`].
    pub fn certificate(
        &self,
        signed: &Signed,
        purpose: Purpose,
    ) -> Result<Certificate, LicenceError> {
        let root = self.root(&signed.by)?;
        let certificate: Certificate = signed.open(Context::Certificate, root)?;
        let id = certificate.key.id();
        if self.revocations.key(&id) {
            return Err(LicenceError::Revoked(id.0));
        }
        if certificate.purpose != purpose {
            return Err(LicenceError::WrongPurpose(id.0));
        }
        Ok(certificate)
    }

    fn root(&self, id: &KeyId) -> Result<&PublicKey, LicenceError> {
        if self.revocations.key(id) {
            return Err(LicenceError::Revoked(id.0.clone()));
        }
        self.roots
            .iter()
            .find(|root| root.id() == *id)
            .ok_or_else(|| LicenceError::Untrusted(id.0.clone()))
    }
}

/// A root certifies a working key.
///
/// # Errors
/// [`LicenceError::Json`] only in theory.
pub fn certify(
    root: &KeyPair,
    key: &PublicKey,
    purpose: Purpose,
    name: &str,
) -> Result<Signed, LicenceError> {
    let certificate = Certificate {
        purpose,
        name: name.to_owned(),
        key: key.clone(),
        issued_unix_s: crate::now_unix_s(),
    };
    Signed::sign(&certificate, Context::Certificate, root)
}

/// The maintainers' roots, as `PublicKey` JSON. Made offline with `orqwa-cli key-new --root`;
/// a new root comes with a release, and one is removed only if it leaks (ADR 0043).
const RELEASE_ROOTS: &[&str] = &[];

/// The withdrawals known when this ORQWA was built.
const BUILTIN_REVOCATIONS: &str = include_str!("../revocations.json");

#[cfg(test)]
mod tests {
    use super::*;

    fn trust(root: &KeyPair) -> Trust {
        Trust::new(vec![root.public().clone()], Revocations::default())
    }

    #[test]
    fn a_root_certifies_a_working_key_for_one_purpose() {
        let root = KeyPair::generate().unwrap();
        let key = KeyPair::generate().unwrap();
        let signed = certify(&root, key.public(), Purpose::World, "world key").unwrap();
        let trust = trust(&root);
        assert_eq!(
            trust.certificate(&signed, Purpose::World).unwrap().key,
            *key.public()
        );
        assert!(matches!(
            trust.certificate(&signed, Purpose::Activation),
            Err(LicenceError::WrongPurpose(_))
        ));
    }

    #[test]
    fn a_key_not_certified_by_a_trusted_root_is_refused() {
        let root = KeyPair::generate().unwrap();
        let stranger = KeyPair::generate().unwrap();
        let key = KeyPair::generate().unwrap();
        let signed = certify(&stranger, key.public(), Purpose::World, "world key").unwrap();
        assert!(matches!(
            trust(&root).certificate(&signed, Purpose::World),
            Err(LicenceError::Untrusted(_))
        ));
    }

    #[test]
    fn a_changed_certificate_is_refused() {
        let root = KeyPair::generate().unwrap();
        let key = KeyPair::generate().unwrap();
        let mut signed = certify(&root, key.public(), Purpose::Activation, "server").unwrap();
        signed.body = signed.body.replace("activation", "world");
        assert!(matches!(
            trust(&root).certificate(&signed, Purpose::World),
            Err(LicenceError::BadSignature)
        ));
    }

    #[test]
    fn withdrawals_refuse_keys_and_only_ever_grow_newer() {
        let root = KeyPair::generate().unwrap();
        let key = KeyPair::generate().unwrap();
        let certificate = certify(&root, key.public(), Purpose::World, "world key").unwrap();
        let mut trust = trust(&root);

        let list = |sequence, keys| Revocations {
            sequence,
            keys,
            worlds: vec![],
        };
        let newer = Signed::sign(
            &list(2, vec![key.public().id()]),
            Context::Revocations,
            &root,
        )
        .unwrap();
        assert!(trust.accept_revocations(&newer).unwrap());
        assert!(matches!(
            trust.certificate(&certificate, Purpose::World),
            Err(LicenceError::Revoked(_))
        ));

        let older = Signed::sign(&list(1, vec![]), Context::Revocations, &root).unwrap();
        assert!(!trust.accept_revocations(&older).unwrap());
        assert_eq!(trust.revocations().sequence, 2);

        let forged = Signed::sign(&list(3, vec![]), Context::Revocations, &key).unwrap();
        assert!(trust.accept_revocations(&forged).is_err());
        assert_eq!(trust.revocations().sequence, 2);
    }

    #[test]
    fn a_withdrawn_root_certifies_nothing() {
        let root = KeyPair::generate().unwrap();
        let key = KeyPair::generate().unwrap();
        let certificate = certify(&root, key.public(), Purpose::World, "world key").unwrap();
        let revocations = Revocations {
            sequence: 1,
            keys: vec![root.public().id()],
            worlds: vec![],
        };
        let trust = Trust::new(vec![root.public().clone()], revocations);
        assert!(matches!(
            trust.certificate(&certificate, Purpose::World),
            Err(LicenceError::Revoked(_))
        ));
    }

    #[test]
    fn a_world_is_withdrawn_by_version_or_entirely() {
        let list = Revocations {
            sequence: 1,
            keys: vec![],
            worlds: vec![
                WithdrawnWorld {
                    id: "a".into(),
                    version: Some("1.0".into()),
                },
                WithdrawnWorld {
                    id: "b".into(),
                    version: None,
                },
            ],
        };
        assert!(list.world("a", Some("1.0")));
        assert!(!list.world("a", Some("1.1")));
        assert!(list.world("b", Some("9")));
        assert!(list.world("b", None));
        assert!(!list.world("c", None));
    }

    #[test]
    fn the_builtin_trust_parses() {
        let trust = Trust::builtin();
        assert!(trust.roots.contains(crate::dev::root().public()));
    }
}
