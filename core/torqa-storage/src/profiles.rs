//! Rider profiles on disk (R22): `profiles/<id>/profile.toml` in the data directory, next to the
//! rider's own rides in `profiles/<id>/rides/`. One directory per rider keeps riders from
//! overwriting each other's files when the data directory is synced (ADR 0002).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use torqa_domain::profile::{Profile, UnitSystem};
use torqa_domain::units::{BeatsPerMinute, Kilograms, Watts};

const PROFILES: &str = "profiles";
const PROFILE_FILE: &str = "profile.toml";
const SETTINGS_FILE: &str = "settings.toml";
const HUD_FILE: &str = "hud.toml";

/// Reading or writing a profile failed.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// File system error.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The file is not valid TOML for a profile.
    #[error("invalid profile: {0}")]
    Parse(#[from] toml::de::Error),
    /// Serialising failed (cannot happen for profiles, but the encoder is fallible).
    #[error("cannot write profile: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// A profile and the directory name that identifies it.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredProfile {
    /// Directory name under `profiles/`; stable when the rider is renamed.
    pub id: String,
    /// The profile.
    pub profile: Profile,
}

/// The file format. Missing fields take defaults, so older files keep working when fields are
/// added.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct ProfileFile {
    name: String,
    rider_mass_kg: f64,
    bike_mass_kg: f64,
    ftp_w: f64,
    max_heart_rate_bpm: f64,
    units: Units,
    language: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Units {
    Metric,
    Imperial,
}

impl Default for ProfileFile {
    fn default() -> Self {
        Self::from(&Profile::default())
    }
}

impl From<&Profile> for ProfileFile {
    fn from(p: &Profile) -> Self {
        Self {
            name: p.name.clone(),
            rider_mass_kg: p.rider_mass.0,
            bike_mass_kg: p.bike_mass.0,
            ftp_w: p.ftp.0,
            max_heart_rate_bpm: p.max_heart_rate.0,
            units: match p.units {
                UnitSystem::Metric => Units::Metric,
                UnitSystem::Imperial => Units::Imperial,
            },
            language: p.language.clone(),
        }
    }
}

impl From<ProfileFile> for Profile {
    fn from(f: ProfileFile) -> Self {
        Self {
            name: f.name,
            rider_mass: Kilograms(f.rider_mass_kg),
            bike_mass: Kilograms(f.bike_mass_kg),
            ftp: Watts(f.ftp_w),
            max_heart_rate: BeatsPerMinute(f.max_heart_rate_bpm),
            units: match f.units {
                Units::Metric => UnitSystem::Metric,
                Units::Imperial => UnitSystem::Imperial,
            },
            language: f.language,
        }
    }
}

/// Per-installation settings shared by all riders.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    /// The profile chosen last.
    active_profile: Option<String>,
    /// The trainer connected last, reconnected at start (R41).
    trainer: Option<RememberedDevice>,
    /// The heart-rate sensor connected last.
    heart_rate: Option<RememberedDevice>,
}

/// A Bluetooth device to reconnect at start (R41).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RememberedDevice {
    /// The system's identifier for it (stable per computer).
    pub id: String,
    /// Its advertised name, for messages and as a fallback when the identifier changed.
    pub name: String,
}

/// The devices to reconnect at start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RememberedDevices {
    /// The trainer connected last.
    pub trainer: Option<RememberedDevice>,
    /// The heart-rate sensor connected last.
    pub heart_rate: Option<RememberedDevice>,
}

/// All readable profiles, by name. Unreadable ones are skipped.
#[must_use]
pub fn list(data_dir: &Path) -> Vec<StoredProfile> {
    let Ok(entries) = std::fs::read_dir(data_dir.join(PROFILES)) else {
        return Vec::new();
    };
    let mut profiles: Vec<StoredProfile> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let id = entry.file_name().to_str()?.to_owned();
            let profile = load(data_dir, &id).ok()?;
            Some(StoredProfile { id, profile })
        })
        .collect();
    profiles.sort_by(|a, b| a.profile.name.cmp(&b.profile.name).then(a.id.cmp(&b.id)));
    profiles
}

/// Reads one profile.
///
/// # Errors
/// If the file is missing or not a valid profile.
pub fn load(data_dir: &Path, id: &str) -> Result<Profile, ProfileError> {
    let text = std::fs::read_to_string(profile_dir(data_dir, id).join(PROFILE_FILE))?;
    Ok(toml::from_str::<ProfileFile>(&text)?.into())
}

/// Writes a profile atomically, creating its directory.
///
/// # Errors
/// On file system errors.
pub fn save(data_dir: &Path, id: &str, profile: &Profile) -> Result<(), ProfileError> {
    let text = toml::to_string_pretty(&ProfileFile::from(profile))?;
    write_atomically(&profile_dir(data_dir, id).join(PROFILE_FILE), &text)
}

/// A new, unused profile id derived from `name`.
#[must_use]
pub fn new_id(data_dir: &Path, name: &str) -> String {
    let slug = crate::slug(name, "rider");
    let mut id = slug.clone();
    let mut n = 1;
    while profile_dir(data_dir, &id).exists() {
        n += 1;
        id = format!("{slug}-{n}");
    }
    id
}

/// The rider's HUD layout: which metrics to show, in order (R23).
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct HudFile {
    metrics: Vec<String>,
}

/// The rider's chosen HUD metrics, in order, if the rider chose any.
#[must_use]
pub fn load_hud(data_dir: &Path, id: &str) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(profile_dir(data_dir, id).join(HUD_FILE)).ok()?;
    Some(toml::from_str::<HudFile>(&text).ok()?.metrics)
}

/// Saves the rider's HUD metrics.
///
/// # Errors
/// On file system errors.
pub fn save_hud(data_dir: &Path, id: &str, metrics: &[String]) -> Result<(), ProfileError> {
    let file = HudFile {
        metrics: metrics.to_vec(),
    };
    write_atomically(
        &profile_dir(data_dir, id).join(HUD_FILE),
        &toml::to_string_pretty(&file)?,
    )
}

/// Where a rider's activities are saved.
#[must_use]
pub fn rides_dir(data_dir: &Path, id: &str) -> PathBuf {
    profile_dir(data_dir, id).join("rides")
}

/// The id of the profile chosen last, if any.
#[must_use]
pub fn active(data_dir: &Path) -> Option<String> {
    settings(data_dir).active_profile
}

/// The devices connected last.
#[must_use]
pub fn remembered_devices(data_dir: &Path) -> RememberedDevices {
    let settings = settings(data_dir);
    RememberedDevices {
        trainer: settings.trainer,
        heart_rate: settings.heart_rate,
    }
}

/// Remembers a trainer (or, with `trainer` false, a heart-rate sensor) to reconnect next time.
///
/// # Errors
/// On file system errors.
pub fn remember_device(
    data_dir: &Path,
    trainer: bool,
    device: RememberedDevice,
) -> Result<(), ProfileError> {
    let mut settings = settings(data_dir);
    if trainer {
        settings.trainer = Some(device);
    } else {
        settings.heart_rate = Some(device);
    }
    save_settings(data_dir, &settings)
}

fn settings(data_dir: &Path) -> Settings {
    std::fs::read_to_string(data_dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_settings(data_dir: &Path, settings: &Settings) -> Result<(), ProfileError> {
    write_atomically(
        &data_dir.join(SETTINGS_FILE),
        &toml::to_string_pretty(settings)?,
    )
}

/// Remembers the chosen profile for the next start.
///
/// # Errors
/// On file system errors.
pub fn set_active(data_dir: &Path, id: &str) -> Result<(), ProfileError> {
    // Read first: the file holds other settings too.
    let mut settings = settings(data_dir);
    settings.active_profile = Some(id.to_owned());
    save_settings(data_dir, &settings)
}

fn profile_dir(data_dir: &Path, id: &str) -> PathBuf {
    data_dir.join(PROFILES).join(id)
}

fn write_atomically(path: &Path, text: &str) -> Result<(), ProfileError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let partial = path.with_extension("toml.part");
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("torqa-profiles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn profiles_round_trip_and_list_by_name() {
        let data = temp_dir("roundtrip");
        let zoe = Profile {
            name: "Zoë".to_owned(),
            ftp: Watts(310.0),
            units: UnitSystem::Imperial,
            language: "de".to_owned(),
            ..Profile::default()
        };
        let anna = Profile {
            name: "Anna".to_owned(),
            ..Profile::default()
        };

        save(&data, "zoe", &zoe).unwrap();
        save(&data, "anna", &anna).unwrap();

        let listed = list(&data);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].profile, anna);
        assert_eq!(
            listed[1],
            StoredProfile {
                id: "zoe".to_owned(),
                profile: zoe
            }
        );
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn files_missing_newer_fields_still_load() {
        let data = temp_dir("partial");
        std::fs::create_dir_all(data.join("profiles/old")).unwrap();
        std::fs::write(
            data.join("profiles/old/profile.toml"),
            "name = \"Old\"\nftp_w = 180.0\n",
        )
        .unwrap();

        let profile = load(&data, "old").unwrap();

        assert_eq!(profile.name, "Old");
        assert_eq!(profile.ftp, Watts(180.0));
        assert_eq!(profile.rider_mass, Profile::default().rider_mass);
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn new_ids_never_reuse_a_directory() {
        let data = temp_dir("ids");
        save(&data, "marco", &Profile::default()).unwrap();

        assert_eq!(new_id(&data, "Marco"), "marco-2");
        assert_eq!(new_id(&data, "Anna B."), "anna-b");
        assert_eq!(new_id(&data, "!!"), "rider");
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn hud_layouts_are_per_rider() {
        let data = temp_dir("hud");
        assert_eq!(load_hud(&data, "anna"), None);

        save_hud(&data, "anna", &["power_3s".to_owned(), "speed".to_owned()]).unwrap();

        assert_eq!(
            load_hud(&data, "anna"),
            Some(vec!["power_3s".to_owned(), "speed".to_owned()])
        );
        assert_eq!(load_hud(&data, "zoe"), None);
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn remembers_devices_alongside_the_active_profile() {
        let data = temp_dir("devices");
        assert_eq!(remembered_devices(&data), RememberedDevices::default());
        let kickr = RememberedDevice {
            id: "hci0/dev_AA".to_owned(),
            name: "KICKR CORE".to_owned(),
        };

        set_active(&data, "anna").unwrap();
        remember_device(&data, true, kickr.clone()).unwrap();
        set_active(&data, "zoe").unwrap();

        assert_eq!(remembered_devices(&data).trainer, Some(kickr));
        assert_eq!(remembered_devices(&data).heart_rate, None);
        assert_eq!(active(&data), Some("zoe".to_owned()));
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn remembers_the_active_profile() {
        let data = temp_dir("active");
        assert_eq!(active(&data), None);

        set_active(&data, "anna").unwrap();

        assert_eq!(active(&data), Some("anna".to_owned()));
        std::fs::remove_dir_all(data).unwrap();
    }
}
