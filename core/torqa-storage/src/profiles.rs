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
        }
    }
}

/// Per-installation settings shared by all riders.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    /// The profile chosen last.
    active_profile: Option<String>,
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

/// Where a rider's activities are saved.
#[must_use]
pub fn rides_dir(data_dir: &Path, id: &str) -> PathBuf {
    profile_dir(data_dir, id).join("rides")
}

/// The id of the profile chosen last, if any.
#[must_use]
pub fn active(data_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(data_dir.join(SETTINGS_FILE)).ok()?;
    toml::from_str::<Settings>(&text).ok()?.active_profile
}

/// Remembers the chosen profile for the next start.
///
/// # Errors
/// On file system errors.
pub fn set_active(data_dir: &Path, id: &str) -> Result<(), ProfileError> {
    let settings = Settings {
        active_profile: Some(id.to_owned()),
    };
    write_atomically(
        &data_dir.join(SETTINGS_FILE),
        &toml::to_string_pretty(&settings)?,
    )
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
    fn remembers_the_active_profile() {
        let data = temp_dir("active");
        assert_eq!(active(&data), None);

        set_active(&data, "anna").unwrap();

        assert_eq!(active(&data), Some("anna".to_owned()));
        std::fs::remove_dir_all(data).unwrap();
    }
}
