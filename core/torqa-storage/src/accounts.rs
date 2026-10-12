//! A rider's accounts on training platforms (R29, ADR 0013): `profiles/<id>/accounts.toml`,
//! one table per platform with who is connected, whether rides go up automatically (R60) and
//! the platform's own credentials, kept as the platform gave them. The file holds secrets, so on macOS and Linux only its owner may
//! read it; it lives with the rider and syncs like the rest of their folder (ADR 0002).

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};
use torqa_domain::upload::Credentials;

use crate::profiles::{ProfileError, profile_dir};

const ACCOUNTS_FILE: &str = "accounts.toml";

/// A rider's account on one platform.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoredAccount {
    /// Who is connected, as the platform knows them.
    pub label: String,
    /// Rides the rider keeps go up without asking (R60).
    pub auto_upload: bool,
    /// What the platform needs for uploads.
    pub credentials: Credentials,
}

/// A rider's accounts, by platform id.
pub type Accounts = BTreeMap<String, StoredAccount>;

/// The accounts of rider `id`; none if they have connected none.
///
/// # Errors
/// If the file cannot be read or is not valid: saving over it would lose the other accounts.
pub fn load(data_dir: &Path, id: &str) -> Result<Accounts, ProfileError> {
    match std::fs::read_to_string(profile_dir(data_dir, id).join(ACCOUNTS_FILE)) {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Accounts::new()),
        Err(error) => Err(error.into()),
    }
}

/// Writes the accounts of rider `id` atomically, readable by the owner alone where the system
/// has such permissions.
///
/// # Errors
/// On file system errors.
pub fn save(data_dir: &Path, id: &str, accounts: &Accounts) -> Result<(), ProfileError> {
    let path = profile_dir(data_dir, id).join(ACCOUNTS_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let partial = path.with_extension("toml.part");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&partial)?;
    // A partial file left by an older Torqa keeps its mode on open.
    #[cfg(unix)]
    std::fs::set_permissions(
        &partial,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )?;
    file.write_all(toml::to_string_pretty(accounts)?.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&partial, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("torqa-accounts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn intervals() -> StoredAccount {
        StoredAccount {
            label: "Ann Rider".to_owned(),
            auto_upload: true,
            credentials: Credentials::from([("api_key".to_owned(), "s3cret".to_owned())]),
        }
    }

    #[test]
    fn a_rider_without_accounts_has_none() {
        let dir = temp_dir("none");

        assert_eq!(load(&dir, "ann").unwrap(), Accounts::new());
    }

    #[test]
    fn accounts_are_kept_per_rider_as_the_platform_gave_them() {
        let dir = temp_dir("roundtrip");
        let accounts = Accounts::from([("intervals".to_owned(), intervals())]);

        save(&dir, "ann", &accounts).unwrap();

        assert_eq!(load(&dir, "ann").unwrap(), accounts);
        assert_eq!(load(&dir, "bob").unwrap(), Accounts::new());
        assert!(dir.join("profiles/ann/accounts.toml").exists());
        assert!(!dir.join("profiles/ann/accounts.toml.part").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn only_the_owner_can_read_the_secrets() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("private");
        save(
            &dir,
            "ann",
            &Accounts::from([("intervals".to_owned(), intervals())]),
        )
        .unwrap();

        let mode = std::fs::metadata(dir.join("profiles/ann/accounts.toml"))
            .unwrap()
            .permissions()
            .mode();

        assert_eq!(mode & 0o777, 0o600);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn accounts_from_before_automatic_uploads_upload_only_when_asked() {
        let dir = temp_dir("before-auto");
        std::fs::create_dir_all(dir.join("profiles/ann")).unwrap();
        std::fs::write(
            dir.join("profiles/ann/accounts.toml"),
            "[intervals]\nlabel = \"Ann\"\n\n[intervals.credentials]\napi_key = \"k\"\n",
        )
        .unwrap();

        let accounts = load(&dir, "ann").unwrap();

        assert!(!accounts["intervals"].auto_upload);
        assert_eq!(accounts["intervals"].credentials["api_key"], "k");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_damaged_file_is_not_taken_for_no_accounts() {
        let dir = temp_dir("damaged");
        std::fs::create_dir_all(dir.join("profiles/ann")).unwrap();
        std::fs::write(dir.join("profiles/ann/accounts.toml"), "[intervals\n").unwrap();

        assert!(matches!(load(&dir, "ann"), Err(ProfileError::Parse(_))));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
