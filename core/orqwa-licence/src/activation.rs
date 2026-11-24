//! An activation: the activation server's signed statement that a licence opens a world on one
//! device, with the world's content key sealed for that device's key. It never expires, so
//! the world opens offline from then on (R279).

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use serde::{Deserialize, Serialize};

use crate::device::{Device, DeviceKey};
use crate::keys::{Context, KeyPair};
use crate::stream::ContentKey;
use crate::trust::{Purpose, Signed, Trust};
use crate::{LicenceError, b64, derive, random};

/// What the activation server signs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Activation {
    /// The licence's id (not its key).
    pub licence: String,
    /// The world's id.
    pub world: String,
    /// The device's id.
    pub device: String,
    /// The buyer's name, shown on the world's page.
    pub licensee: String,
    /// The content key's id.
    pub content_key: String,
    /// The content key, sealed for the device.
    #[serde(with = "b64")]
    pub sealed_key: Vec<u8>,
    /// When it was issued.
    pub issued_unix_s: u64,
}

/// An activation as the device keeps it: the server's certificate and the signed activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationFile {
    /// A root's certificate for the activation key.
    pub certificate: Signed,
    /// The [`Activation`], signed by that key.
    pub activation: Signed,
}

fn wrapping_key(
    device_key: &DeviceKey,
    world: &str,
    device: &str,
    content_key: &str,
) -> LessSafeKey {
    let key: [u8; 32] = derive(
        b"ORQWA orqwa-1 wrap",
        device_key.bytes(),
        &[
            world.as_bytes(),
            b"\0",
            device.as_bytes(),
            b"\0",
            content_key.as_bytes(),
        ],
    );
    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &key).expect("32 bytes for AES-256"))
}

const WRAP_AAD: &[u8] = b"ORQWA orqwa-1 content key";

/// What the activation server is asked to activate.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    /// The licence's id.
    pub licence: &'a str,
    /// The buyer's name.
    pub licensee: &'a str,
    /// The world's id.
    pub world: &'a str,
    /// The device's id.
    pub device: &'a str,
    /// The device's key.
    pub device_key: &'a DeviceKey,
}

/// Issues an activation, as the activation server does once it has counted the device.
///
/// # Errors
/// [`LicenceError::Random`] if no secure random bytes are available.
pub fn issue(
    request: &Request<'_>,
    content_key: &ContentKey,
    signer: &KeyPair,
    certificate: Signed,
) -> Result<ActivationFile, LicenceError> {
    let nonce: [u8; NONCE_LEN] = random()?;
    let mut sealed = content_key.secret().to_vec();
    wrapping_key(
        request.device_key,
        request.world,
        request.device,
        content_key.id(),
    )
    .seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(WRAP_AAD),
        &mut sealed,
    )
    .map_err(|_| LicenceError::Invalid("could not seal the content key".into()))?;
    let mut sealed_key = nonce.to_vec();
    sealed_key.extend_from_slice(&sealed);
    let activation = Activation {
        licence: request.licence.to_owned(),
        world: request.world.to_owned(),
        device: request.device.to_owned(),
        licensee: request.licensee.to_owned(),
        content_key: content_key.id().to_owned(),
        sealed_key,
        issued_unix_s: crate::now_unix_s(),
    };
    Ok(ActivationFile {
        certificate,
        activation: Signed::sign(&activation, Context::Activation, signer)?,
    })
}

impl ActivationFile {
    /// The activation and the world's content key, if a trusted activation key signed it for
    /// `world` and this device.
    ///
    /// # Errors
    /// [`LicenceError::NotThisDevice`] for another world or device (or the same folder on
    /// another computer), or the certificate's and signature's errors.
    pub fn open(
        &self,
        trust: &Trust,
        world: &str,
        device: &Device,
    ) -> Result<(Activation, ContentKey), LicenceError> {
        let certificate = trust.certificate(&self.certificate, Purpose::Activation)?;
        let activation: Activation = self
            .activation
            .open(Context::Activation, &certificate.key)?;
        if activation.world != world || activation.device != device.id() {
            return Err(LicenceError::NotThisDevice);
        }
        if activation.sealed_key.len() < NONCE_LEN {
            return Err(LicenceError::NotThisDevice);
        }
        let (nonce, sealed) = activation.sealed_key.split_at(NONCE_LEN);
        let nonce =
            Nonce::try_assume_unique_for_key(nonce).map_err(|_| LicenceError::NotThisDevice)?;
        let mut sealed = sealed.to_vec();
        let key: [u8; 32] = wrapping_key(device.key(), world, device.id(), &activation.content_key)
            .open_in_place(nonce, Aad::from(WRAP_AAD), &mut sealed)
            .ok()
            .and_then(|key| <[u8; 32]>::try_from(&*key).ok())
            .ok_or(LicenceError::NotThisDevice)?;
        let content_key = ContentKey::new(&activation.content_key, key);
        Ok((activation, content_key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev;

    fn activate(device: &Device, world: &str, content: &ContentKey) -> ActivationFile {
        let request = Request {
            licence: "L-1",
            licensee: "Rider",
            world,
            device: device.id(),
            device_key: device.key(),
        };
        issue(
            &request,
            content,
            &dev::activation_key(),
            dev::activation_certificate(),
        )
        .unwrap()
    }

    #[test]
    fn an_activation_opens_the_world_on_its_device_only() {
        let content = ContentKey::new("k1", [9; 32]);
        let device = Device::new("d1", &[1; 32], b"machine a");
        let file = activate(&device, "w", &content);
        let trust = Trust::builtin();

        let (activation, key) = file.open(&trust, "w", &device).unwrap();
        assert_eq!(key, content);
        assert_eq!(activation.licensee, "Rider");

        let other_device = Device::new("d2", &[1; 32], b"machine a");
        assert!(matches!(
            file.open(&trust, "w", &other_device),
            Err(LicenceError::NotThisDevice)
        ));
        let other_machine = Device::new("d1", &[1; 32], b"machine b");
        assert!(matches!(
            file.open(&trust, "w", &other_machine),
            Err(LicenceError::NotThisDevice)
        ));
        assert!(matches!(
            file.open(&trust, "x", &device),
            Err(LicenceError::NotThisDevice)
        ));
    }

    #[test]
    fn only_a_certified_activation_key_can_activate() {
        let content = ContentKey::new("k1", [9; 32]);
        let device = Device::new("d1", &[1; 32], b"m");
        let request = Request {
            licence: "L",
            licensee: "R",
            world: "w",
            device: "d1",
            device_key: device.key(),
        };
        let forged = issue(
            &request,
            &content,
            &dev::world_key(),
            dev::world_certificate(),
        )
        .unwrap();
        assert!(matches!(
            forged.open(&Trust::builtin(), "w", &device),
            Err(LicenceError::WrongPurpose(_))
        ));

        let mut changed = activate(&device, "w", &content);
        changed.activation.body = changed.activation.body.replace("Rider", "Other");
        assert!(matches!(
            changed.open(&Trust::builtin(), "w", &device),
            Err(LicenceError::BadSignature)
        ));
    }
}
