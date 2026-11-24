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
    /// A hash of the machine id it was made on: a copy elsewhere starts a new identity.
    machine: String,
}

/// What the device file keeps of the machine id: enough to notice another computer.
fn machine_hash(machine: &[u8]) -> String {
    let digest = aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA512,
        &[b"ORQWA orqwa-1 machine\0".as_slice(), machine].concat(),
    );
    hex(&digest.as_ref()[..16])
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

    /// The device kept in `dir` for this machine, made there on first use. A device file
    /// copied from another computer (a backup restored, a migration) is replaced by a new
    /// identity: this computer becomes a device of its own instead of sharing the other's id.
    ///
    /// # Errors
    /// [`LicenceError::NoMachine`] without a machine id: an activation not bound to the
    /// machine would open wherever the folder is copied. Otherwise if the folder cannot be
    /// written, or holds a damaged device file, which is not replaced: a new identity would take
    /// another of the licence's devices.
    pub fn load_or_create(dir: &Path, machine: &[u8]) -> Result<Self, LicenceError> {
        if machine.is_empty() {
            return Err(LicenceError::NoMachine);
        }
        let path = dir.join(FILE);
        let here = machine_hash(machine);
        // Twice at most: a first start racing another makes one identity, the other reads it.
        for _ in 0..2 {
            match fs::read_to_string(&path) {
                Ok(text) => {
                    let file: DeviceFile = serde_json::from_str(&text)?;
                    let secret: [u8; 32] = file.secret.try_into().map_err(|_| {
                        LicenceError::Invalid(format!("{} is damaged", path.display()))
                    })?;
                    if file.machine == here {
                        return Ok(Self::new(&file.id, &secret, machine));
                    }
                    let (text, device) = Self::fresh(machine, &here)?;
                    write_private(&path, text.as_bytes())?;
                    return Ok(device);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let (text, device) = Self::fresh(machine, &here)?;
                    fs::create_dir_all(dir)?;
                    match write_new_private(&path, text.as_bytes()) {
                        Ok(()) => return Ok(device),
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(e) => return Err(e.into()),
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(LicenceError::Invalid(format!(
            "{} keeps changing",
            path.display()
        )))
    }

    /// Replaces the device kept in `dir` with a new identity, as after releasing it: activations
    /// sealed for the old one, even copies put back, open nothing.
    ///
    /// # Errors
    /// [`LicenceError::NoMachine`] without a machine id, or if the file cannot be written.
    pub fn renew(dir: &Path, machine: &[u8]) -> Result<Self, LicenceError> {
        if machine.is_empty() {
            return Err(LicenceError::NoMachine);
        }
        let (text, device) = Self::fresh(machine, &machine_hash(machine))?;
        fs::create_dir_all(dir)?;
        write_private(&dir.join(FILE), text.as_bytes())?;
        Ok(device)
    }

    /// A new identity for this machine, and its device file.
    fn fresh(machine: &[u8], here: &str) -> Result<(String, Self), LicenceError> {
        let id = hex(&random::<16>()?);
        let secret: [u8; 32] = random()?;
        let text = serde_json::to_string_pretty(&DeviceFile {
            id: id.clone(),
            secret: secret.to_vec(),
            machine: here.to_owned(),
        })?;
        Ok((text, Self::new(&id, &secret, machine)))
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

/// Writes a file only its owner may read, in full or not at all, replacing one there.
///
/// # Errors
/// If writing or renaming fails.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = write_partial(path, bytes)?;
    fs::rename(&partial, path).inspect_err(|_| {
        let _ = fs::remove_file(&partial);
    })
}

/// Writes a new file only its owner may read, in full or not at all, never replacing one:
/// [`std::io::ErrorKind::AlreadyExists`] if `path` exists, also when another writer got there
/// first.
///
/// # Errors
/// As above, or if writing fails.
pub fn write_new_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = write_partial(path, bytes)?;
    // A hard link never replaces: of two writers, exactly one succeeds.
    let linked = fs::hard_link(&partial, path);
    let _ = fs::remove_file(&partial);
    linked
}

fn write_partial(path: &Path, bytes: &[u8]) -> std::io::Result<std::path::PathBuf> {
    let unique = random::<8>().map_err(std::io::Error::other)?;
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.part", hex(&unique)));
    let partial = path.with_file_name(name);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options.open(&partial).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    match written {
        Ok(()) => Ok(partial),
        Err(error) => {
            let _ = fs::remove_file(&partial);
            Err(error)
        }
    }
}

/// The operating system's id for this computer, or `None` where it has none ORQWA can read.
/// Android's comes from the app, through Java.
#[must_use]
pub fn machine_id() -> Option<Vec<u8>> {
    // Read once: on macOS and Windows it takes starting a program.
    static ID: std::sync::OnceLock<Option<Vec<u8>>> = std::sync::OnceLock::new();
    ID.get_or_init(|| {
        let id = platform_machine_id()?;
        let id = id.trim();
        (!id.is_empty()).then(|| id.as_bytes().to_vec())
    })
    .clone()
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
    use std::os::windows::process::CommandExt as _;
    // Not looked up on the path, and without the console window a GUI program would flash.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let system = std::env::var_os("SystemRoot")?;
    let reg = std::path::Path::new(&system)
        .join("System32")
        .join("reg.exe");
    let out = std::process::Command::new(reg)
        .creation_flags(CREATE_NO_WINDOW)
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
    fn a_device_folder_copied_to_another_computer_becomes_a_new_device_there() {
        let dir = dir("copied");
        let here = Device::load_or_create(&dir, b"this machine").unwrap();
        let there = Device::load_or_create(&dir, b"other machine").unwrap();
        assert_ne!(here.id(), there.id(), "not two computers sharing one id");
        assert_ne!(here.key(), there.key());
        assert_eq!(
            Device::load_or_create(&dir, b"other machine").unwrap().id(),
            there.id()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_renewed_device_is_another_device() {
        let dir = dir("renewed");
        let before = Device::load_or_create(&dir, b"m").unwrap();
        let renewed = Device::renew(&dir, b"m").unwrap();
        assert_ne!(before.id(), renewed.id());
        assert_ne!(before.key(), renewed.key());
        assert_eq!(
            Device::load_or_create(&dir, b"m").unwrap().id(),
            renewed.id()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn without_a_machine_id_there_is_no_device() {
        let dir = dir("unbound");
        assert!(matches!(
            Device::load_or_create(&dir, b""),
            Err(LicenceError::NoMachine)
        ));
        assert!(!dir.join(FILE).exists());
    }

    #[test]
    fn a_new_file_is_never_written_over() {
        let dir = dir("new");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key");
        write_new_private(&path, b"first").unwrap();
        let again = write_new_private(&path, b"second").unwrap_err();
        assert_eq!(again.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), b"first");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no partial files left"
        );
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
