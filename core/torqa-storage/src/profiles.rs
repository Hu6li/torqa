//! Rider profiles on disk (R22): `profiles/<id>/profile.toml` in the data directory, next to the
//! rider's own rides in `profiles/<id>/rides/`. One directory per rider keeps riders from
//! overwriting each other's files when the data directory is synced (ADR 0002).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use torqa_domain::profile::{Avatar, Drivetrain, Profile, UnitSystem};
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
    avatar: AvatarFile,
    drivetrain: DrivetrainFile,
    chainring: u8,
    cog: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DrivetrainFile {
    Cassette,
    SingleCog,
}

/// A common road chainring on the Zwift Cog: the gears a rider sets up first.
const DEFAULT_CHAINRING: u8 = 50;
const DEFAULT_COG: u8 = 14;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Units {
    Metric,
    Imperial,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AvatarFile {
    Female,
    Male,
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
            avatar: match p.avatar {
                Avatar::Female => AvatarFile::Female,
                Avatar::Male => AvatarFile::Male,
            },
            drivetrain: match p.drivetrain {
                Drivetrain::Cassette => DrivetrainFile::Cassette,
                Drivetrain::SingleCog { .. } => DrivetrainFile::SingleCog,
            },
            // Kept while riding a cassette, for switching back.
            chainring: match p.drivetrain {
                Drivetrain::SingleCog { chainring, .. } => chainring,
                Drivetrain::Cassette => DEFAULT_CHAINRING,
            },
            cog: match p.drivetrain {
                Drivetrain::SingleCog { cog, .. } => cog,
                Drivetrain::Cassette => DEFAULT_COG,
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
            language: f.language,
            avatar: match f.avatar {
                AvatarFile::Female => Avatar::Female,
                AvatarFile::Male => Avatar::Male,
            },
            drivetrain: match f.drivetrain {
                DrivetrainFile::Cassette => Drivetrain::Cassette,
                DrivetrainFile::SingleCog => Drivetrain::SingleCog {
                    chainring: f.chainring.max(1),
                    cog: f.cog.max(1),
                },
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
    /// The trainer connected last, reconnected at start (R41).
    trainer: Option<RememberedDevice>,
    /// The heart-rate sensor connected last.
    heart_rate: Option<RememberedDevice>,
    /// The controller (shifter) connected last.
    controller: Option<RememberedDevice>,
    /// The D-Fly channels of a Di2 shifter that shift up and down (R7).
    shift_up_channel: Option<u8>,
    shift_down_channel: Option<u8>,
    /// How detailed the 3D world is drawn on this computer (R43).
    graphics_quality: Option<GraphicsQuality>,
    /// Where the overlay was last on screen (R55).
    overlay: Option<OverlayWindow>,
}

/// Where the overlay window is on screen (R55), in screen pixels, and how large it draws.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OverlayWindow {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// How large the overlay draws its figures (#124): 1 is one interface unit per point.
    /// `None` in settings from before it could be chosen, which take the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
}

/// How detailed the 3D world is drawn (R43): more detail needs a stronger GPU. Medium holds
/// 60 fps on a base M1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphicsQuality {
    /// For weak integrated GPUs.
    Low,
    /// The default: 60 fps on a base M1.
    #[default]
    Medium,
    /// For stronger Apple GPUs and discrete GPUs.
    High,
    /// Everything on, including global illumination.
    Ultra,
}

impl GraphicsQuality {
    /// All presets, from the lightest.
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Ultra];

    /// The name stored and passed to the front end.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Ultra => "ultra",
        }
    }

    /// The preset called `name`, if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|q| q.name() == name)
    }
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
    /// The controller (shifter) connected last.
    pub controller: Option<RememberedDevice>,
}

/// What a remembered device is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceRole {
    /// The trainer.
    Trainer,
    /// The heart-rate sensor.
    HeartRate,
    /// A controller that shifts.
    Controller,
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
        controller: settings.controller,
    }
}

/// Remembers a device in its `role` to reconnect next time.
///
/// # Errors
/// On file system errors.
pub fn remember_device(
    data_dir: &Path,
    role: DeviceRole,
    device: RememberedDevice,
) -> Result<(), ProfileError> {
    let mut settings = settings(data_dir);
    let slot = match role {
        DeviceRole::Trainer => &mut settings.trainer,
        DeviceRole::HeartRate => &mut settings.heart_rate,
        DeviceRole::Controller => &mut settings.controller,
    };
    *slot = Some(device);
    save_settings(data_dir, &settings)
}

/// The D-Fly channels (1–4) whose buttons shift up and down; 1 and 2 until chosen.
#[must_use]
pub fn shift_channels(data_dir: &Path) -> (u8, u8) {
    let settings = settings(data_dir);
    (
        settings.shift_up_channel.unwrap_or(1),
        settings.shift_down_channel.unwrap_or(2),
    )
}

/// Remembers which D-Fly channels shift up and down.
///
/// # Errors
/// On file system errors.
pub fn set_shift_channels(data_dir: &Path, up: u8, down: u8) -> Result<(), ProfileError> {
    let mut settings = settings(data_dir);
    settings.shift_up_channel = Some(up);
    settings.shift_down_channel = Some(down);
    save_settings(data_dir, &settings)
}

/// The graphics quality chosen on this installation; Medium until one is chosen.
#[must_use]
pub fn graphics_quality(data_dir: &Path) -> GraphicsQuality {
    settings(data_dir).graphics_quality.unwrap_or_default()
}

/// Remembers the graphics quality for this installation.
///
/// # Errors
/// On file system errors.
pub fn set_graphics_quality(data_dir: &Path, quality: GraphicsQuality) -> Result<(), ProfileError> {
    let mut settings = settings(data_dir);
    settings.graphics_quality = Some(quality);
    save_settings(data_dir, &settings)
}

/// Where the overlay was last on screen; `None` before it was first used.
#[must_use]
pub fn overlay_window(data_dir: &Path) -> Option<OverlayWindow> {
    settings(data_dir).overlay
}

/// Remembers where the overlay is on screen, for the next time (R55).
///
/// # Errors
/// On file system errors.
pub fn set_overlay_window(data_dir: &Path, window: OverlayWindow) -> Result<(), ProfileError> {
    let mut settings = settings(data_dir);
    settings.overlay = Some(window);
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
    fn graphics_quality_is_kept_with_the_other_settings() {
        let dir = temp_dir("quality");
        assert_eq!(graphics_quality(&dir), GraphicsQuality::Medium);
        remember_device(
            &dir,
            DeviceRole::Trainer,
            RememberedDevice {
                id: "kickr".to_owned(),
                name: "KICKR".to_owned(),
            },
        )
        .unwrap();

        set_graphics_quality(&dir, GraphicsQuality::Ultra).unwrap();

        assert_eq!(graphics_quality(&dir), GraphicsQuality::Ultra);
        assert!(remembered_devices(&dir).trainer.is_some());
        assert!(
            std::fs::read_to_string(dir.join(SETTINGS_FILE))
                .unwrap()
                .contains(r#"graphics_quality = "ultra""#)
        );
        assert_eq!(
            GraphicsQuality::from_name("high"),
            Some(GraphicsQuality::High)
        );
    }

    #[test]
    fn shift_channels_default_to_one_and_two() {
        let dir = temp_dir("channels");
        assert_eq!(shift_channels(&dir), (1, 2));

        set_shift_channels(&dir, 3, 4).unwrap();

        assert_eq!(shift_channels(&dir), (3, 4));
    }

    #[test]
    fn the_overlay_window_is_remembered_with_the_other_settings() {
        let dir = temp_dir("overlay");
        assert_eq!(overlay_window(&dir), None);
        set_graphics_quality(&dir, GraphicsQuality::High).unwrap();
        let window = OverlayWindow {
            x: -1200,
            y: 40,
            width: 320,
            height: 480,
            scale: Some(1.75),
        };

        set_overlay_window(&dir, window).unwrap();

        assert_eq!(overlay_window(&dir), Some(window));
        assert_eq!(graphics_quality(&dir), GraphicsQuality::High);
        assert_eq!(GraphicsQuality::from_name("epic"), None);
    }

    #[test]
    fn an_overlay_remembered_before_its_size_could_be_chosen_keeps_its_place() {
        let dir = temp_dir("overlay-before-scale");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(SETTINGS_FILE),
            "[overlay]\nx = 10\ny = 20\nwidth = 300\nheight = 400\n",
        )
        .unwrap();

        assert_eq!(
            overlay_window(&dir),
            Some(OverlayWindow {
                x: 10,
                y: 20,
                width: 300,
                height: 400,
                scale: None,
            })
        );
    }

    #[test]
    fn profiles_round_trip_and_list_by_name() {
        let data = temp_dir("roundtrip");
        let zoe = Profile {
            name: "Zoë".to_owned(),
            ftp: Watts(310.0),
            units: UnitSystem::Imperial,
            language: "de".to_owned(),
            avatar: Avatar::Male,
            drivetrain: Drivetrain::SingleCog {
                chainring: 46,
                cog: 14,
            },
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
        assert_eq!(profile.avatar, Profile::default().avatar);
        assert_eq!(
            profile.drivetrain,
            Drivetrain::Cassette,
            "riders shift on the bike"
        );
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
        remember_device(&data, DeviceRole::Trainer, kickr.clone()).unwrap();
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
