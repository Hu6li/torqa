//! This device's identity for activations: a random id and secret, kept in a private folder
//! that is never synced (`device.json`). The key sent to the activation server is derived from
//! the secret and the operating system's machine id, which never leaves the device, so a copy
//! of the folder on another computer derives another key and opens nothing.

use std::fs;
use std::io::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{LicenceError, b64, derive, hex, random};

/// The file in the device folder.
pub const FILE: &str = "device.json";

/// The key an activation is sealed for. Secret: sent only to the activation server.
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceKey(pub(crate) [u8; 32]);

impl std::fmt::Debug for DeviceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeviceKey(…)")
    }
}

impl DeviceKey {
    /// The key's bytes, for the request to the activation server.
    #[must_use]
    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The key the activation server received.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// This device as activations know it.
#[derive(Debug, Clone)]
pub struct Device {
    id: String,
    key: DeviceKey,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceFile {
    id: String,
    #[serde(with = "b64")]
    secret: Vec<u8>,
}

impl Device {
    /// The device from its id, its secret and the machine id.
    #[must_use]
    pub fn new(id: &str, secret: &[u8; 32], machine: &[u8]) -> Self {
        Self {
            id: id.to_owned(),
            key: DeviceKey(derive(b"ORQWA orqwa-1 device", secret, &[machine])),
        }
    }

    /// The device kept in `dir`, made there on first use.
    ///
    /// # Errors
    /// If the folder cannot be written, or holds a damaged device file. A damaged file is not
    /// replaced: a new identity would take another of the licence's devices.
    pub fn load_or_create(dir: &Path, machine: &[u8]) -> Result<Self, LicenceError> {
        let path = dir.join(FILE);
        match fs::read_to_string(&path) {
            Ok(text) => {
                let file: DeviceFile = serde_json::from_str(&text)?;
                let secret: [u8; 32] = file
                    .secret
                    .try_into()
                    .map_err(|_| LicenceError::Invalid(format!("{} is damaged", path.display())))?;
                Ok(Self::new(&file.id, &secret, machine))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let id = hex(&random::<16>()?);
                let secret: [u8; 32] = random()?;
                let text = serde_json::to_string_pretty(&DeviceFile {
                    id: id.clone(),
                    secret: secret.to_vec(),
                })?;
                fs::create_dir_all(dir)?;
                write_private(&path, text.as_bytes())?;
                Ok(Self::new(&id, &secret, machine))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// The random id the activation server counts.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The key activations for this device are sealed for.
    #[must_use]
    pub fn key(&self) -> &DeviceKey {
        &self.key
    }
}

/// Writes a file only its owner may read, in full or not at all.
///
/// # Errors
/// If writing or renaming fails.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = path.with_extension("part");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&partial)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&partial, path)
}

/// The operating system's id for this computer, or `None` where it has none ORQWA can read.
/// Android's comes from the app, through Java.
#[must_use]
pub fn machine_id() -> Option<Vec<u8>> {
    let id = platform_machine_id()?;
    let id = id.trim();
    (!id.is_empty()).then(|| id.as_bytes().to_vec())
}

#[cfg(target_os = "linux")]
fn platform_machine_id() -> Option<String> {
    ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .iter()
        .find_map(|p| fs::read_to_string(p).ok())
}

#[cfg(target_os = "macos")]
fn platform_machine_id() -> Option<String> {
    let out = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|line| line.contains("\"IOPlatformUUID\""))
        .and_then(|line| line.rsplit('"').nth(1).map(str::to_owned))
}

#[cfg(windows)]
fn platform_machine_id() -> Option<String> {
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Cryptography",
            "/v",
            "MachineGuid",
        ])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|line| line.contains("MachineGuid"))
        .and_then(|line| line.split_whitespace().last().map(str::to_owned))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn platform_machine_id() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("orqwa-device-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_device_keeps_its_identity() {
        let dir = dir("keeps");
        let first = Device::load_or_create(&dir, b"machine").unwrap();
        let again = Device::load_or_create(&dir, b"machine").unwrap();
        assert_eq!(first.id(), again.id());
        assert_eq!(first.key(), again.key());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join(FILE)).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "only the owner may read the device secret");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_copied_device_folder_derives_another_key_on_another_machine() {
        let dir = dir("copied");
        let here = Device::load_or_create(&dir, b"this machine").unwrap();
        let there = Device::load_or_create(&dir, b"other machine").unwrap();
        assert_eq!(here.id(), there.id());
        assert_ne!(here.key(), there.key());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_damaged_device_file_is_not_replaced() {
        let dir = dir("damaged");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(FILE), "{").unwrap();
        assert!(Device::load_or_create(&dir, b"m").is_err());
        assert_eq!(fs::read_to_string(dir.join(FILE)).unwrap(), "{");
        fs::remove_dir_all(dir).unwrap();
    }
}
