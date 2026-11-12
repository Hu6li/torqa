//! Rider profiles on disk (R22): `profiles/<id>/profile.toml` in the data directory, next to the
//! rider's own rides in `profiles/<id>/rides/`, HUD layout, overlay place and devices. One
//! directory per rider keeps riders from overwriting each other's files when the data directory
//! is synced (ADR 0002). What depends on the computer instead is in [`crate::computer`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use orqwa_domain::profile::{
    Avatar, CalorieEffort, CalorieProfile, CalorieSex, Drivetrain, MapStyle, Profile, UnitSystem,
    Voice,
};
use orqwa_domain::shifting::{ButtonAction, ButtonMap, ControllerModel, Press};
use orqwa_domain::telemetry::CadenceSource;
use orqwa_domain::units::{BeatsPerMinute, Kilograms, Percent, Watts};
use serde::{Deserialize, Serialize};

const PROFILES: &str = "profiles";
const PROFILE_FILE: &str = "profile.toml";
const SETTINGS_FILE: &str = "settings.toml";
const HUD_FILE: &str = "hud.toml";
const OVERLAY_FILE: &str = "overlay.toml";
const DEVICES_FILE: &str = "devices.toml";

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
    /// The id is not the name of a directory under `profiles/`.
    #[error("invalid profile id {0:?}")]
    InvalidId(String),
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
    calories: CalorieFile,
    units: Units,
    language: String,
    avatar: AvatarFile,
    map_style: MapStyleFile,
    voice: VoiceFile,
    /// Off for older files, so nobody sees the colours without choosing them.
    gradient_colours: bool,
    /// 0 for none.
    rides_per_week: u8,
    drivetrain: DrivetrainFile,
    chainring: u8,
    cog: u8,
    /// Upper bounds of the power and heart-rate zones in percent of FTP and of the maximum
    /// heart rate; the standard zones where missing or out of order.
    power_zones_pct: Vec<f64>,
    heart_rate_zones_pct: Vec<f64>,
    default_difficulty_pct: f64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct CalorieFile {
    birth_year: Option<u16>,
    /// Legacy fixed age, interpreted in the year this profile was last saved.
    #[serde(skip_serializing_if = "Option::is_none")]
    age: Option<u8>,
    sex: Option<String>,
    effort: Option<String>,
}

impl From<CalorieProfile> for CalorieFile {
    fn from(profile: CalorieProfile) -> Self {
        Self {
            birth_year: profile.birth_year,
            age: None,
            sex: profile.sex.map(|sex| match sex {
                CalorieSex::Female => "female".to_owned(),
                CalorieSex::Male => "male".to_owned(),
            }),
            effort: profile.effort.map(|effort| match effort {
                CalorieEffort::Easy => "easy".to_owned(),
                CalorieEffort::Moderate => "moderate".to_owned(),
                CalorieEffort::Hard => "hard".to_owned(),
            }),
        }
    }
}

impl CalorieFile {
    fn into_profile(self, reference_year: u16) -> CalorieProfile {
        let file = self;
        let mut profile = CalorieProfile {
            birth_year: file.birth_year,
            sex: match file.sex.as_deref() {
                Some("female") => Some(CalorieSex::Female),
                Some("male") => Some(CalorieSex::Male),
                _ => None,
            },
            effort: match file.effort.as_deref() {
                Some("easy") => Some(CalorieEffort::Easy),
                Some("moderate") => Some(CalorieEffort::Moderate),
                Some("hard") => Some(CalorieEffort::Hard),
                _ => None,
            },
        };
        if profile.birth_year.is_none() {
            profile.set_age(file.age, reference_year);
        }
        profile
    }
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum MapStyleFile {
    Coloured,
    Dark,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum VoiceFile {
    Casual,
    Plain,
    Off,
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
            calories: p.calories.into(),
            units: match p.units {
                UnitSystem::Metric => Units::Metric,
                UnitSystem::Imperial => Units::Imperial,
            },
            language: p.language.clone(),
            avatar: match p.avatar {
                Avatar::Female => AvatarFile::Female,
                Avatar::Male => AvatarFile::Male,
            },
            map_style: match p.map_style {
                MapStyle::Coloured => MapStyleFile::Coloured,
                MapStyle::Dark => MapStyleFile::Dark,
            },
            voice: match p.voice {
                Voice::Casual => VoiceFile::Casual,
                Voice::Plain => VoiceFile::Plain,
                Voice::Off => VoiceFile::Off,
            },
            gradient_colours: p.gradient_colours,
            rides_per_week: p.rides_per_week.unwrap_or(0),
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
            power_zones_pct: p.power_zones.iter().map(|b| b * 100.0).collect(),
            heart_rate_zones_pct: p.heart_rate_zones.iter().map(|b| b * 100.0).collect(),
            default_difficulty_pct: p.default_difficulty.0,
        }
    }
}

/// Percentages as shares.
fn shares(percent: &[f64]) -> Vec<f64> {
    percent.iter().map(|p| p / 100.0).collect()
}

impl ProfileFile {
    fn into_profile(self, reference_year: u16) -> Profile {
        let f = self;
        Profile {
            name: f.name,
            rider_mass: Kilograms(f.rider_mass_kg),
            bike_mass: Kilograms(f.bike_mass_kg),
            ftp: Watts(f.ftp_w),
            max_heart_rate: BeatsPerMinute(f.max_heart_rate_bpm),
            calories: f.calories.into_profile(reference_year),
            units: match f.units {
                Units::Metric => UnitSystem::Metric,
                Units::Imperial => UnitSystem::Imperial,
            },
            language: f.language,
            avatar: match f.avatar {
                AvatarFile::Female => Avatar::Female,
                AvatarFile::Male => Avatar::Male,
            },
            map_style: match f.map_style {
                MapStyleFile::Coloured => MapStyle::Coloured,
                MapStyleFile::Dark => MapStyle::Dark,
            },
            voice: match f.voice {
                VoiceFile::Casual => Voice::Casual,
                VoiceFile::Plain => Voice::Plain,
                VoiceFile::Off => Voice::Off,
            },
            gradient_colours: f.gradient_colours,
            rides_per_week: Some(f.rides_per_week).filter(|n| *n > 0),
            drivetrain: match f.drivetrain {
                DrivetrainFile::Cassette => Drivetrain::Cassette,
                DrivetrainFile::SingleCog => Drivetrain::SingleCog {
                    chainring: f.chainring.max(1),
                    cog: f.cog.max(1),
                },
            },
            power_zones: Profile::sane_power_zones(&shares(&f.power_zones_pct)),
            heart_rate_zones: Profile::sane_heart_rate_zones(&shares(&f.heart_rate_zones_pct)),
            default_difficulty: Percent(f.default_difficulty_pct.clamp(0.0, 100.0)),
        }
    }
}

/// Per-installation settings shared by all riders.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    /// The profile chosen last.
    active_profile: Option<String>,
    /// The devices, the cadence's source and the shifter's buttons, from before R82 made them
    /// the rider's ([`DevicesFile`]); carried over once by [`carry_over`], then gone.
    trainer: Option<RememberedDevice>,
    heart_rate: Option<RememberedDevice>,
    power_meter: Option<RememberedDevice>,
    cadence_sensor: Option<RememberedDevice>,
    cadence_source: Option<String>,
    controller: Option<RememberedDevice>,
    buttons: Option<ButtonsFile>,
    /// The D-Fly channels that shifted up and down before buttons could be given other
    /// controls (#139): carried over as the buttons where those were never saved.
    shift_up_channel: Option<u8>,
    shift_down_channel: Option<u8>,
    /// How detailed the 3D world was drawn, from before R78 made it the computer's
    /// ([`crate::computer`]); carried over once by [`carry_over`], then gone.
    graphics_quality: Option<GraphicsQuality>,
    /// Where the overlay was last on screen, from before R78 made it the rider's; carried over
    /// once by [`carry_over`], then gone.
    overlay: Option<OverlayWindow>,
    /// Whether the ride view's control bar is folded away to its corner (#189).
    ride_bar_folded: Option<bool>,
    /// Whether the app plays its own sounds, such as the chime of a ride's moments (R125).
    sounds: Option<bool>,
    /// Whether the app filled the screen, from before R78 made it the computer's; carried over
    /// once by [`carry_over`], then gone.
    fullscreen: Option<bool>,
    /// How the Courses tab orders and groups the library (R67).
    courses_view: Option<CoursesView>,
}

/// How the Courses tab orders and groups the library (R67), by the names the app gives its
/// choices; empty names for the app's defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CoursesView {
    /// What the gallery is sorted by.
    pub sort: String,
    /// Whether from the largest, latest or last letter down.
    pub descending: bool,
    /// What it is grouped by.
    pub group: String,
}

/// What the shifter's buttons do (#139): for each button (a Di2's D-Fly channel, a Click's
/// numbered button), the action of each kind of press by name. A press left out, or with an
/// unknown action, does nothing. Files from before the Click v2 name four channels; the rest
/// stay empty.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_field_names)] // the channels, named as the rider reads them in the file
struct ButtonsFile {
    channel_1: PressesFile,
    channel_2: PressesFile,
    channel_3: PressesFile,
    channel_4: PressesFile,
    channel_5: PressesFile,
    channel_6: PressesFile,
    channel_7: PressesFile,
    channel_8: PressesFile,
    channel_9: PressesFile,
    channel_10: PressesFile,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct PressesFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    press: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hold: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    double_press: Option<String>,
}

impl From<&ButtonMap> for ButtonsFile {
    fn from(map: &ButtonMap) -> Self {
        let channel = |button: u8| {
            let name = |press| {
                map.action(button, press)
                    .map(|action| action.name().to_owned())
            };
            PressesFile {
                press: name(Press::Short),
                hold: name(Press::Long),
                double_press: name(Press::Double),
            }
        };
        Self {
            channel_1: channel(1),
            channel_2: channel(2),
            channel_3: channel(3),
            channel_4: channel(4),
            channel_5: channel(5),
            channel_6: channel(6),
            channel_7: channel(7),
            channel_8: channel(8),
            channel_9: channel(9),
            channel_10: channel(10),
        }
    }
}

impl ButtonsFile {
    fn map(&self) -> ButtonMap {
        let mut map = ButtonMap::NONE;
        let channels = [
            &self.channel_1,
            &self.channel_2,
            &self.channel_3,
            &self.channel_4,
            &self.channel_5,
            &self.channel_6,
            &self.channel_7,
            &self.channel_8,
            &self.channel_9,
            &self.channel_10,
        ];
        for (button, presses) in (1..).zip(channels) {
            for (press, name) in [
                (Press::Short, &presses.press),
                (Press::Long, &presses.hold),
                (Press::Double, &presses.double_press),
            ] {
                map.assign(
                    button,
                    press,
                    name.as_deref().and_then(ButtonAction::from_name),
                );
            }
        }
        map
    }
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

impl RememberedDevice {
    /// Whether the device the system knows as `id`, advertised as `name`, is this one: by its
    /// identifier, else by its name, as the identifier differs on another computer.
    #[must_use]
    pub fn is(&self, id: &str, name: &str) -> bool {
        self.id == id || self.name == name
    }
}

/// The devices to reconnect at start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RememberedDevices {
    /// The trainer connected last.
    pub trainer: Option<RememberedDevice>,
    /// The heart-rate sensor connected last.
    pub heart_rate: Option<RememberedDevice>,
    /// The power meter connected last (R71).
    pub power_meter: Option<RememberedDevice>,
    /// The cadence sensor connected last (R72).
    pub cadence_sensor: Option<RememberedDevice>,
    /// The controller (shifter) connected last.
    pub controller: Option<RememberedDevice>,
}

impl RememberedDevices {
    /// The device remembered in `role`, if any.
    #[must_use]
    pub fn get(&self, role: DeviceRole) -> Option<&RememberedDevice> {
        match role {
            DeviceRole::Trainer => self.trainer.as_ref(),
            DeviceRole::HeartRate => self.heart_rate.as_ref(),
            DeviceRole::PowerMeter => self.power_meter.as_ref(),
            DeviceRole::CadenceSensor => self.cadence_sensor.as_ref(),
            DeviceRole::Controller => self.controller.as_ref(),
        }
    }
}

/// What a remembered device is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceRole {
    /// The trainer.
    Trainer,
    /// The heart-rate sensor.
    HeartRate,
    /// The power meter (R71).
    PowerMeter,
    /// The cadence sensor (R72).
    CadenceSensor,
    /// A controller that shifts.
    Controller,
}

impl DeviceRole {
    /// Every role, the trainer first.
    pub const ALL: [Self; 5] = [
        Self::Trainer,
        Self::HeartRate,
        Self::PowerMeter,
        Self::CadenceSensor,
        Self::Controller,
    ];
}

/// The rider's devices (R82), in `profiles/<id>/devices.toml`: the ones to reconnect, where the
/// cadence comes from and what each shifter's buttons do. Riders on one computer each bring
/// their own strap or shifter, and their choices follow them to another sharing the data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct DevicesFile {
    trainer: Option<RememberedDevice>,
    heart_rate: Option<RememberedDevice>,
    power_meter: Option<RememberedDevice>,
    cadence_sensor: Option<RememberedDevice>,
    controller: Option<RememberedDevice>,
    /// Which device's cadence counts (R72), by [`CadenceSource::name`].
    cadence_source: Option<String>,
    /// What every shifter's buttons did before each model had its own (R84): what a model
    /// without its own still does, until the rider gives one its own.
    buttons: Option<ButtonsFile>,
    /// What each shifter model's buttons do (#139, R84), by [`ControllerModel::name`]; the
    /// first two shift until the model's are saved.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    shifter_buttons: BTreeMap<String, ButtonsFile>,
}

impl DevicesFile {
    fn slot(&mut self, role: DeviceRole) -> &mut Option<RememberedDevice> {
        match role {
            DeviceRole::Trainer => &mut self.trainer,
            DeviceRole::HeartRate => &mut self.heart_rate,
            DeviceRole::PowerMeter => &mut self.power_meter,
            DeviceRole::CadenceSensor => &mut self.cadence_sensor,
            DeviceRole::Controller => &mut self.controller,
        }
    }
}

impl Settings {
    /// Takes the device settings kept for every rider before R82, if there were any.
    fn take_devices(&mut self) -> Option<DevicesFile> {
        let up = self.shift_up_channel.take();
        let down = self.shift_down_channel.take();
        let buttons = self.buttons.take().or_else(|| {
            (up.is_some() || down.is_some()).then(|| {
                ButtonsFile::from(&ButtonMap::shifting(up.unwrap_or(1), down.unwrap_or(2)))
            })
        });
        let devices = DevicesFile {
            trainer: self.trainer.take(),
            heart_rate: self.heart_rate.take(),
            power_meter: self.power_meter.take(),
            cadence_sensor: self.cadence_sensor.take(),
            controller: self.controller.take(),
            cadence_source: self.cadence_source.take(),
            buttons,
            shifter_buttons: BTreeMap::new(),
        };
        let any = devices.trainer.is_some()
            || devices.heart_rate.is_some()
            || devices.power_meter.is_some()
            || devices.cadence_sensor.is_some()
            || devices.controller.is_some()
            || devices.cadence_source.is_some()
            || devices.buttons.is_some();
        any.then_some(devices)
    }
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
    let path = profile_dir(data_dir, id).join(PROFILE_FILE);
    let text = std::fs::read_to_string(&path)?;
    let file = toml::from_str::<ProfileFile>(&text)?;
    // Legacy ages have no reference year. The last save's timestamp is the available anchor;
    // conversion stays read-only, and the next explicit save stores the inferred birth year.
    let reference_year = std::fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(orqwa_domain::calendar::utc_year)
        .or_else(|| orqwa_domain::calendar::utc_year(std::time::SystemTime::now()))
        .unwrap_or(0);
    Ok(file.into_profile(reference_year))
}

/// Writes a profile atomically, creating its directory.
///
/// # Errors
/// On file system errors.
pub fn save(data_dir: &Path, id: &str, profile: &Profile) -> Result<(), ProfileError> {
    let text = toml::to_string_pretty(&ProfileFile::from(profile))?;
    write_atomically(&profile_dir(data_dir, id).join(PROFILE_FILE), &text)
}

/// Deletes a rider with everything in their directory: profile, HUD layout and rides.
///
/// # Errors
/// [`ProfileError::InvalidId`] unless `id` is a plain directory name, so nothing outside
/// `profiles/` can be removed; otherwise on file system errors.
pub fn delete(data_dir: &Path, id: &str) -> Result<(), ProfileError> {
    let mut parts = Path::new(id).components();
    if !matches!(
        (parts.next(), parts.next()),
        (Some(std::path::Component::Normal(_)), None)
    ) {
        return Err(ProfileError::InvalidId(id.to_owned()));
    }
    std::fs::remove_dir_all(profile_dir(data_dir, id))?;
    Ok(())
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
    workout_metrics: Option<Vec<String>>,
}

/// The rider's chosen ride or workout HUD metrics (R207), in order, if chosen.
/// Legacy layouts start both uses with the same figures.
#[must_use]
pub fn load_hud(data_dir: &Path, id: &str, workout: bool) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(profile_dir(data_dir, id).join(HUD_FILE)).ok()?;
    let file = toml::from_str::<HudFile>(&text).ok()?;
    Some(if workout {
        file.workout_metrics.unwrap_or(file.metrics)
    } else {
        file.metrics
    })
}

/// Saves one of the rider's HUD layouts, preserving the other (R207).
///
/// # Errors
/// On file system errors.
pub fn save_hud(
    data_dir: &Path,
    id: &str,
    metrics: &[String],
    workout: bool,
) -> Result<(), ProfileError> {
    let mut file = std::fs::read_to_string(profile_dir(data_dir, id).join(HUD_FILE))
        .ok()
        .and_then(|text| toml::from_str::<HudFile>(&text).ok())
        .unwrap_or_default();
    // Freeze the old shared layout before either use is edited for the first time.
    file.workout_metrics
        .get_or_insert_with(|| file.metrics.clone());
    if workout {
        file.workout_metrics = Some(metrics.to_vec());
    } else {
        file.metrics = metrics.to_vec();
    }
    write_atomically(
        &profile_dir(data_dir, id).join(HUD_FILE),
        &toml::to_string_pretty(&file)?,
    )
}

/// Where the rider's overlay window is on screen (R55, R78), in `profiles/<id>/overlay.toml`.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct OverlayFile {
    window: Option<OverlayWindow>,
}

/// Where the rider's overlay was last on screen; `None` before they first used it.
#[must_use]
pub fn overlay_window(data_dir: &Path, id: &str) -> Option<OverlayWindow> {
    let text = std::fs::read_to_string(profile_dir(data_dir, id).join(OVERLAY_FILE)).ok()?;
    toml::from_str::<OverlayFile>(&text).ok()?.window
}

/// Remembers where the rider's overlay is on screen, for the next time (R55).
///
/// # Errors
/// On file system errors.
pub fn set_overlay_window(
    data_dir: &Path,
    id: &str,
    window: OverlayWindow,
) -> Result<(), ProfileError> {
    let file = OverlayFile {
        window: Some(window),
    };
    write_atomically(
        &profile_dir(data_dir, id).join(OVERLAY_FILE),
        &toml::to_string_pretty(&file)?,
    )
}

/// Carries over, once, what `settings.toml` kept for every rider and computer before R78
/// (#257) and R82: the overlay's place and the devices (with the cadence's source and the
/// shifter's buttons) to every rider who has none of their own, the graphics quality and full
/// screen to this computer's `file` where it has not chosen. Then they are gone from the data
/// directory, so another computer sharing it starts from its own choices, and each rider from
/// their own devices.
///
/// # Errors
/// On file system errors.
pub fn carry_over(data_dir: &Path, file: &Path) -> Result<(), ProfileError> {
    let mut settings = settings(data_dir);
    let quality = settings.graphics_quality.take();
    let fullscreen = settings.fullscreen.take();
    let overlay = settings.overlay.take();
    let devices = settings.take_devices();
    if quality.is_none() && fullscreen.is_none() && overlay.is_none() && devices.is_none() {
        return Ok(());
    }
    for rider in list(data_dir) {
        if let Some(window) = overlay
            && overlay_window(data_dir, &rider.id).is_none()
        {
            set_overlay_window(data_dir, &rider.id, window)?;
        }
        if let Some(devices) = &devices
            && !devices_path(data_dir, &rider.id).exists()
        {
            save_devices(data_dir, &rider.id, devices)?;
        }
    }
    crate::computer::take_over(file, quality, fullscreen)?;
    save_settings(data_dir, &settings)
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

fn devices_path(data_dir: &Path, id: &str) -> PathBuf {
    profile_dir(data_dir, id).join(DEVICES_FILE)
}

/// The rider's devices; none, automatic cadence and shifting buttons before they chose any.
fn devices(data_dir: &Path, id: &str) -> DevicesFile {
    std::fs::read_to_string(devices_path(data_dir, id))
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_devices(data_dir: &Path, id: &str, devices: &DevicesFile) -> Result<(), ProfileError> {
    write_atomically(
        &devices_path(data_dir, id),
        &toml::to_string_pretty(devices)?,
    )
}

/// The devices the rider connected last (R82).
#[must_use]
pub fn remembered_devices(data_dir: &Path, id: &str) -> RememberedDevices {
    let devices = devices(data_dir, id);
    RememberedDevices {
        trainer: devices.trainer,
        heart_rate: devices.heart_rate,
        power_meter: devices.power_meter,
        cadence_sensor: devices.cadence_sensor,
        controller: devices.controller,
    }
}

/// Remembers a device in its `role` to reconnect for the rider next time.
///
/// # Errors
/// On file system errors.
pub fn remember_device(
    data_dir: &Path,
    id: &str,
    role: DeviceRole,
    device: RememberedDevice,
) -> Result<(), ProfileError> {
    set_remembered(data_dir, id, role, Some(device))
}

/// Stops reconnecting the rider's device in `role`.
///
/// # Errors
/// On file system errors.
pub fn forget_device(data_dir: &Path, id: &str, role: DeviceRole) -> Result<(), ProfileError> {
    set_remembered(data_dir, id, role, None)
}

fn set_remembered(
    data_dir: &Path,
    id: &str,
    role: DeviceRole,
    device: Option<RememberedDevice>,
) -> Result<(), ProfileError> {
    let mut devices = devices(data_dir, id);
    *devices.slot(role) = device;
    save_devices(data_dir, id, &devices)
}

/// Gives the rider `to` the devices, cadence source and buttons of the rider `from`: a new
/// rider starts with those in use, so adding one lets nothing connected go.
///
/// # Errors
/// On file system errors.
pub fn copy_devices(data_dir: &Path, from: &str, to: &str) -> Result<(), ProfileError> {
    save_devices(data_dir, to, &devices(data_dir, from))
}

/// Which device's cadence counts for the rider (R72); automatic until chosen.
#[must_use]
pub fn cadence_source(data_dir: &Path, id: &str) -> CadenceSource {
    devices(data_dir, id)
        .cadence_source
        .and_then(|name| CadenceSource::from_name(&name))
        .unwrap_or_default()
}

/// Remembers which device's cadence counts for the rider (R72).
///
/// # Errors
/// On file system errors.
pub fn set_cadence_source(
    data_dir: &Path,
    id: &str,
    source: CadenceSource,
) -> Result<(), ProfileError> {
    let mut devices = devices(data_dir, id);
    devices.cadence_source = Some(source.name().to_owned());
    save_devices(data_dir, id, &devices)
}

/// What the buttons of the rider's shifter `model` do (#139, R84); the first two shift up and
/// down until they chose.
#[must_use]
pub fn button_map(data_dir: &Path, id: &str, model: ControllerModel) -> ButtonMap {
    let devices = devices(data_dir, id);
    // The buttons every shifter shared before R84 were never the phone remote's.
    let shared = devices
        .buttons
        .as_ref()
        .filter(|_| ControllerModel::SHIFTERS.contains(&model));
    devices
        .shifter_buttons
        .get(model.name())
        .or(shared)
        .map_or_else(|| model.default_buttons(), ButtonsFile::map)
}

/// Remembers what the buttons of the rider's shifter `model` do; the other models' stay.
///
/// # Errors
/// On file system errors.
pub fn set_button_map(
    data_dir: &Path,
    id: &str,
    model: ControllerModel,
    map: &ButtonMap,
) -> Result<(), ProfileError> {
    let mut devices = devices(data_dir, id);
    // What every model did before stays each one's own.
    if let Some(shared) = devices.buttons.take() {
        for other in ControllerModel::SHIFTERS {
            devices
                .shifter_buttons
                .entry(other.name().to_owned())
                .or_insert_with(|| shared.clone());
        }
    }
    devices
        .shifter_buttons
        .insert(model.name().to_owned(), ButtonsFile::from(map));
    save_devices(data_dir, id, &devices)
}

/// Whether the ride view's control bar is folded away to its corner (#189); shown until it
/// is folded.
#[must_use]
pub fn ride_bar_folded(data_dir: &Path) -> bool {
    settings(data_dir).ride_bar_folded.unwrap_or(false)
}

/// Remembers whether the ride view's control bar is folded away.
///
/// # Errors
/// On file system errors.
pub fn set_ride_bar_folded(data_dir: &Path, folded: bool) -> Result<(), ProfileError> {
    let mut settings = settings(data_dir);
    settings.ride_bar_folded = Some(folded);
    save_settings(data_dir, &settings)
}

/// Whether the app plays its own sounds, such as the chime of a ride's moments (R125); on
/// until turned off.
#[must_use]
pub fn sounds(data_dir: &Path) -> bool {
    settings(data_dir).sounds.unwrap_or(true)
}

/// Remembers whether the app plays its own sounds.
///
/// # Errors
/// On file system errors.
pub fn set_sounds(data_dir: &Path, on: bool) -> Result<(), ProfileError> {
    let mut settings = settings(data_dir);
    settings.sounds = Some(on);
    save_settings(data_dir, &settings)
}

/// How the Courses tab was left (R67); the defaults until it is changed.
#[must_use]
pub fn courses_view(data_dir: &Path) -> CoursesView {
    settings(data_dir).courses_view.unwrap_or_default()
}

/// Remembers how the Courses tab orders and groups the library.
///
/// # Errors
/// On file system errors.
pub fn set_courses_view(data_dir: &Path, view: &CoursesView) -> Result<(), ProfileError> {
    let mut settings = settings(data_dir);
    settings.courses_view = Some(view.clone());
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

pub(crate) fn profile_dir(data_dir: &Path, id: &str) -> PathBuf {
    data_dir.join(PROFILES).join(id)
}

pub(crate) fn write_atomically(path: &Path, text: &str) -> Result<(), ProfileError> {
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
    use orqwa_domain::shifting::Control;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orqwa-profiles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn graphics_quality_presets_are_known_by_name() {
        assert_eq!(
            GraphicsQuality::from_name("high"),
            Some(GraphicsQuality::High)
        );
        assert_eq!(GraphicsQuality::from_name("epic"), None);
        assert_eq!(GraphicsQuality::Ultra.name(), "ultra");
    }

    #[test]
    fn buttons_shift_until_the_rider_gives_them_something_else() {
        let dir = temp_dir("buttons-default");

        for model in ControllerModel::SHIFTERS {
            assert_eq!(button_map(&dir, "anna", model), ButtonMap::shifting(1, 2));
        }
        assert_eq!(
            button_map(&dir, "anna", ControllerModel::PhoneRemote),
            ControllerModel::PhoneRemote.default_buttons()
        );
    }

    #[test]
    fn assigned_buttons_are_each_riders_own_and_kept_by_name() {
        let dir = temp_dir("buttons");
        set_cadence_source(&dir, "anna", CadenceSource::Trainer).unwrap();
        let mut map = ButtonMap::shifting(2, 1);
        map.assign(
            3,
            Press::Long,
            Some(ButtonAction::Control(Control::NextCamera)),
        );
        map.assign(1, Press::Double, None);

        set_button_map(&dir, "anna", ControllerModel::Di2, &map).unwrap();

        assert_eq!(button_map(&dir, "anna", ControllerModel::Di2), map);
        assert_eq!(
            button_map(&dir, "ben", ControllerModel::Di2),
            ButtonMap::default(),
            "another rider's own"
        );
        assert_eq!(
            cadence_source(&dir, "anna"),
            CadenceSource::Trainer,
            "the rider's other device settings stay"
        );
        let text = std::fs::read_to_string(devices_path(&dir, "anna")).unwrap();
        assert!(text.contains(r#"hold = "next_camera""#), "{text}");
        assert!(!dir.join(SETTINGS_FILE).exists(), "nothing for every rider");
    }

    #[test]
    fn each_shifter_models_buttons_are_its_own() {
        let dir = temp_dir("buttons-models");
        let mut di2 = ButtonMap::shifting(1, 2);
        di2.assign(1, Press::Short, Some(ButtonAction::Control(Control::Pause)));
        let click = ButtonMap::shifting(2, 1);

        set_button_map(&dir, "anna", ControllerModel::Di2, &di2).unwrap();
        set_button_map(&dir, "anna", ControllerModel::ZwiftClick, &click).unwrap();

        assert_eq!(button_map(&dir, "anna", ControllerModel::Di2), di2);
        assert_eq!(button_map(&dir, "anna", ControllerModel::ZwiftClick), click);
        assert_eq!(
            button_map(&dir, "anna", ControllerModel::ZwiftClickV2),
            ButtonMap::default(),
            "a model not given any shifts"
        );
        let text = std::fs::read_to_string(devices_path(&dir, "anna")).unwrap();
        assert!(
            text.contains("[shifter_buttons.zwift_click.channel_1]"),
            "{text}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn buttons_every_shifter_shared_stay_each_models_until_it_is_given_its_own() {
        let dir = temp_dir("buttons-shared");
        std::fs::create_dir_all(profile_dir(&dir, "anna")).unwrap();
        std::fs::write(
            devices_path(&dir, "anna"),
            "[buttons.channel_1]\npress = \"shift_down\"\n\n\
             [buttons.channel_3]\nhold = \"next_camera\"\n",
        )
        .unwrap();
        let shared = button_map(&dir, "anna", ControllerModel::Di2);
        assert_eq!(
            shared.action(1, Press::Short),
            Some(ButtonAction::ShiftDown)
        );
        assert_eq!(
            shared.action(3, Press::Long),
            Some(ButtonAction::Control(Control::NextCamera))
        );

        set_button_map(&dir, "anna", ControllerModel::Di2, &ButtonMap::default()).unwrap();

        assert_eq!(
            button_map(&dir, "anna", ControllerModel::Di2),
            ButtonMap::default()
        );
        for model in [ControllerModel::ZwiftClick, ControllerModel::ZwiftClickV2] {
            assert_eq!(button_map(&dir, "anna", model), shared, "{model:?}");
        }
        assert_eq!(
            button_map(&dir, "anna", ControllerModel::PhoneRemote),
            ControllerModel::PhoneRemote.default_buttons(),
            "never the phone remote's"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unknown_button_action_names_do_nothing() {
        let dir = temp_dir("buttons-unknown");
        std::fs::create_dir_all(profile_dir(&dir, "anna")).unwrap();
        std::fs::write(
            devices_path(&dir, "anna"),
            "[shifter_buttons.di2.channel_1]\npress = \"launch\"\nhold = \"shift_up\"\n",
        )
        .unwrap();

        let map = button_map(&dir, "anna", ControllerModel::Di2);

        assert_eq!(map.action(1, Press::Short), None);
        assert_eq!(map.action(1, Press::Long), Some(ButtonAction::ShiftUp));
        assert_eq!(map.action(2, Press::Short), None, "not shifting any more");
    }

    #[test]
    fn the_folded_ride_bar_is_remembered_with_the_other_settings() {
        let dir = temp_dir("ride-bar");
        assert!(!ride_bar_folded(&dir));
        set_active(&dir, "anna").unwrap();

        set_ride_bar_folded(&dir, true).unwrap();

        assert!(ride_bar_folded(&dir));
        assert_eq!(active(&dir).as_deref(), Some("anna"));
        set_ride_bar_folded(&dir, false).unwrap();
        assert!(!ride_bar_folded(&dir));
    }

    #[test]
    fn sounds_are_on_until_turned_off_and_stay_off() {
        let dir = temp_dir("sounds");
        assert!(sounds(&dir), "a new installation chimes (R125)");
        set_ride_bar_folded(&dir, true).unwrap();

        set_sounds(&dir, false).unwrap();

        assert!(!sounds(&dir));
        assert!(ride_bar_folded(&dir), "the other settings stay");
        set_sounds(&dir, true).unwrap();
        assert!(sounds(&dir));
    }

    #[test]
    fn the_courses_view_is_remembered_with_the_other_settings() {
        let dir = temp_dir("courses-view");
        assert_eq!(courses_view(&dir), CoursesView::default());
        set_ride_bar_folded(&dir, true).unwrap();
        let view = CoursesView {
            sort: "added".to_owned(),
            descending: true,
            group: "terrain".to_owned(),
        };

        set_courses_view(&dir, &view).unwrap();

        assert_eq!(courses_view(&dir), view);
        assert!(ride_bar_folded(&dir));
    }

    #[test]
    fn the_overlay_place_is_each_riders_own() {
        let dir = temp_dir("overlay");
        assert_eq!(overlay_window(&dir, "anna"), None);
        let window = OverlayWindow {
            x: -1200,
            y: 40,
            width: 320,
            height: 480,
            scale: Some(1.75),
        };

        set_overlay_window(&dir, "anna", window).unwrap();

        assert_eq!(overlay_window(&dir, "anna"), Some(window));
        assert_eq!(overlay_window(&dir, "ben"), None, "another rider's own");
        assert!(profile_dir(&dir, "anna").join(OVERLAY_FILE).exists());
    }

    #[test]
    fn an_overlay_remembered_before_its_size_could_be_chosen_keeps_its_place() {
        let dir = temp_dir("overlay-before-scale");
        std::fs::create_dir_all(profile_dir(&dir, "anna")).unwrap();
        std::fs::write(
            profile_dir(&dir, "anna").join(OVERLAY_FILE),
            "[window]\nx = 10\ny = 20\nwidth = 300\nheight = 400\n",
        )
        .unwrap();

        assert_eq!(
            overlay_window(&dir, "anna"),
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
    fn settings_from_before_are_carried_over_once_to_the_riders_and_the_computer() {
        let dir = temp_dir("carry-over");
        let computer = dir.join("cache").join(crate::computer::FILE);
        save(&dir, "anna", &Profile::default()).unwrap();
        save(&dir, "ben", &Profile::default()).unwrap();
        let own = OverlayWindow {
            x: 5,
            y: 5,
            width: 200,
            height: 300,
            scale: Some(1.0),
        };
        set_overlay_window(&dir, "ben", own).unwrap();
        std::fs::write(
            dir.join(SETTINGS_FILE),
            "active_profile = \"anna\"\ngraphics_quality = \"ultra\"\nfullscreen = true\n\n\
             [overlay]\nx = 10\ny = 20\nwidth = 300\nheight = 400\n",
        )
        .unwrap();

        carry_over(&dir, &computer).unwrap();

        assert_eq!(
            overlay_window(&dir, "anna"),
            Some(OverlayWindow {
                x: 10,
                y: 20,
                width: 300,
                height: 400,
                scale: None,
            }),
            "the place everyone shared is the rider's now"
        );
        assert_eq!(
            overlay_window(&dir, "ben"),
            Some(own),
            "a rider's own stays"
        );
        assert_eq!(
            crate::computer::graphics_quality(&computer),
            GraphicsQuality::Ultra
        );
        assert!(crate::computer::fullscreen(&computer));
        assert_eq!(
            active(&dir).as_deref(),
            Some("anna"),
            "the other settings stay"
        );
        let text = std::fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
        assert!(
            !text.contains("graphics_quality")
                && !text.contains("overlay")
                && !text.contains("fullscreen"),
            "gone from the data directory: {text}"
        );

        // Once: another computer sharing the data directory keeps what it chose since.
        crate::computer::set_graphics_quality(&computer, GraphicsQuality::Low).unwrap();
        std::fs::write(dir.join(SETTINGS_FILE), "graphics_quality = \"ultra\"\n").unwrap();
        carry_over(&dir, &computer).unwrap();
        assert_eq!(
            crate::computer::graphics_quality(&computer),
            GraphicsQuality::Low
        );
        carry_over(&dir, &computer).unwrap();
    }

    #[test]
    fn devices_from_before_are_carried_over_once_to_every_rider() {
        let dir = temp_dir("carry-over-devices");
        let computer = dir.join("cache").join(crate::computer::FILE);
        save(&dir, "anna", &Profile::default()).unwrap();
        save(&dir, "ben", &Profile::default()).unwrap();
        let own = RememberedDevice {
            id: "hci0/dev_CC".to_owned(),
            name: "Ben's strap".to_owned(),
        };
        remember_device(&dir, "ben", DeviceRole::HeartRate, own.clone()).unwrap();
        std::fs::write(
            dir.join(SETTINGS_FILE),
            "active_profile = \"anna\"\ncadence_source = \"trainer\"\n\
             shift_up_channel = 2\nshift_down_channel = 1\n\n\
             [trainer]\nid = \"hci0/dev_AA\"\nname = \"KICKR CORE\"\n\n\
             [heart_rate]\nid = \"hci0/dev_BB\"\nname = \"Polar H9\"\n",
        )
        .unwrap();

        carry_over(&dir, &computer).unwrap();

        let anna = remembered_devices(&dir, "anna");
        assert_eq!(anna.trainer.map(|d| d.name).as_deref(), Some("KICKR CORE"));
        assert_eq!(anna.heart_rate.map(|d| d.name).as_deref(), Some("Polar H9"));
        assert_eq!(cadence_source(&dir, "anna"), CadenceSource::Trainer);
        for model in ControllerModel::SHIFTERS {
            assert_eq!(
                button_map(&dir, "anna", model),
                ButtonMap::shifting(2, 1),
                "the channels chosen before still shift"
            );
        }
        assert_eq!(
            button_map(&dir, "anna", ControllerModel::PhoneRemote),
            ControllerModel::PhoneRemote.default_buttons(),
            "a shifter's channels were never the phone's"
        );
        let ben = remembered_devices(&dir, "ben");
        assert_eq!(ben.heart_rate, Some(own), "a rider's own stay");
        assert_eq!(ben.trainer, None, "and are not mixed with everyone's");
        assert_eq!(
            active(&dir).as_deref(),
            Some("anna"),
            "the other settings stay"
        );
        let text = std::fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
        assert!(
            !text.contains("trainer")
                && !text.contains("heart_rate")
                && !text.contains("cadence")
                && !text.contains("channel"),
            "gone from the shared settings: {text}"
        );

        // Once: what a rider chose since stays.
        forget_device(&dir, "anna", DeviceRole::HeartRate).unwrap();
        carry_over(&dir, &computer).unwrap();
        assert_eq!(remembered_devices(&dir, "anna").heart_rate, None);
        std::fs::remove_dir_all(dir).unwrap();
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
            map_style: MapStyle::Dark,
            voice: Voice::Plain,
            gradient_colours: true,
            rides_per_week: Some(4),
            drivetrain: Drivetrain::SingleCog {
                chainring: 46,
                cog: 14,
            },
            power_zones: [0.5, 0.7, 0.85, 1.0, 1.15, 1.4],
            heart_rate_zones: [0.55, 0.65, 0.75, 0.85],
            default_difficulty: Percent(65.0),
            calories: CalorieProfile {
                birth_year: Some(1986),
                sex: Some(CalorieSex::Female),
                effort: Some(CalorieEffort::Easy),
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
    fn older_profiles_do_not_assume_calorie_inputs_from_avatar() {
        let file: ProfileFile = toml::from_str("name = 'Rider'\navatar = 'male'\n").unwrap();
        let profile = file.into_profile(2026);
        assert_eq!(profile.calories, CalorieProfile::default());
        let file: ProfileFile =
            toml::from_str("[calories]\nage = 12\nsex = 'unknown'\neffort = 'unknown'\n").unwrap();
        assert_eq!(file.into_profile(2026).calories, CalorieProfile::default());
    }

    #[test]
    fn legacy_calorie_age_uses_the_last_save_year_and_persists_a_stable_birth_year() {
        use std::time::{Duration, UNIX_EPOCH};
        let dir = temp_dir("legacy-calorie-age");
        let path = profile_dir(&dir, "rider").join(PROFILE_FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "name = 'Older rider'\nftp_w = 250.0\n[calories]\nage = 40\nsex = 'female'\neffort = 'easy'\n";
        std::fs::write(&path, original).unwrap();
        let modified_in = |year| {
            let day = orqwa_domain::calendar::days_from_civil(year, 6, 1);
            let time = UNIX_EPOCH + Duration::from_secs(u64::try_from(day * 86_400).unwrap());
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(time))
                .unwrap();
        };
        modified_in(2026);
        let profile = load(&dir, "rider").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original,
            "loading stays read-only"
        );
        assert_eq!(profile.calories.birth_year, Some(1986));
        assert_eq!(profile.calories.age(2027), Some(41));
        assert_eq!(profile.calories.sex, Some(CalorieSex::Female));
        assert_eq!(profile.calories.effort, Some(CalorieEffort::Easy));
        assert_eq!(profile.name, "Older rider");
        assert_eq!(profile.ftp, Watts(250.0));
        save(&dir, "rider", &profile).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("birth_year = 1986"));
        assert!(!written.lines().any(|line| line.starts_with("age =")));
        modified_in(2028);
        let reopened = load(&dir, "rider").unwrap();
        assert_eq!(reopened, profile, "later saves never rebase the birth year");
        assert_eq!(reopened.calories.age(2028), Some(42));
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
            profile.voice,
            Voice::Casual,
            "riders hear the warm voice first (R124)"
        );
        assert_eq!(profile.rides_per_week, None);
        assert!(
            !profile.gradient_colours,
            "elevation colours stay off until the rider opts in (R242)"
        );
        assert_eq!(
            profile.drivetrain,
            Drivetrain::SingleCog {
                chainring: 50,
                cog: 14
            },
            "a shifter shifts virtual gears from the first ride (R98)"
        );
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn a_cassette_chosen_before_stays_with_the_new_default() {
        let data = temp_dir("cassette");
        let rider = Profile {
            drivetrain: Drivetrain::Cassette,
            ..Profile::default()
        };
        save(&data, "rider", &rider).unwrap();

        assert_eq!(
            load(&data, "rider").unwrap().drivetrain,
            Drivetrain::Cassette
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
    fn deleting_a_rider_removes_their_rides_and_nobody_else() {
        let data = temp_dir("delete");
        save(&data, "anna", &Profile::default()).unwrap();
        save_hud(&data, "anna", &["speed".to_owned()], false).unwrap();
        std::fs::create_dir_all(rides_dir(&data, "anna")).unwrap();
        std::fs::write(rides_dir(&data, "anna").join("ride.fit"), b"fit").unwrap();
        save(&data, "zoe", &Profile::default()).unwrap();

        delete(&data, "anna").unwrap();

        assert!(!profile_dir(&data, "anna").exists());
        assert_eq!(
            list(&data).into_iter().map(|p| p.id).collect::<Vec<_>>(),
            ["zoe"]
        );
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn only_a_rider_directory_can_be_deleted() {
        let data = temp_dir("delete-outside");
        save(&data, "anna", &Profile::default()).unwrap();

        for id in ["", ".", "..", "../profiles", "anna/rides", "/tmp"] {
            assert!(
                matches!(delete(&data, id), Err(ProfileError::InvalidId(_))),
                "{id:?}"
            );
        }

        assert!(data.join(PROFILES).join("anna").join(PROFILE_FILE).exists());
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn hud_layouts_are_per_rider() {
        let data = temp_dir("hud");
        assert_eq!(load_hud(&data, "anna", false), None);

        save_hud(
            &data,
            "anna",
            &["power_3s".to_owned(), "speed".to_owned()],
            false,
        )
        .unwrap();
        save_hud(&data, "anna", &["heart_rate".to_owned()], true).unwrap();

        assert_eq!(
            load_hud(&data, "anna", false),
            Some(vec!["power_3s".to_owned(), "speed".to_owned()])
        );
        assert_eq!(
            load_hud(&data, "anna", true),
            Some(vec!["heart_rate".to_owned()])
        );
        assert_eq!(load_hud(&data, "zoe", false), None);
        assert_eq!(load_hud(&data, "zoe", true), None);
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn legacy_hud_starts_both_uses_and_first_edit_preserves_the_other() {
        for workout in [false, true] {
            let data = temp_dir(if workout {
                "legacy-workout-hud"
            } else {
                "legacy-ride-hud"
            });
            let path = profile_dir(&data, "anna").join(HUD_FILE);
            write_atomically(&path, "metrics = [\"speed\", \"power_3s\"]\n").unwrap();
            let legacy = vec!["speed".to_owned(), "power_3s".to_owned()];
            assert_eq!(load_hud(&data, "anna", false), Some(legacy.clone()));
            assert_eq!(load_hud(&data, "anna", true), Some(legacy.clone()));

            let changed = vec!["heart_rate".to_owned()];
            save_hud(&data, "anna", &changed, workout).unwrap();
            assert_eq!(load_hud(&data, "anna", workout), Some(changed));
            assert_eq!(load_hud(&data, "anna", !workout), Some(legacy));
            std::fs::remove_dir_all(data).unwrap();
        }
    }

    #[test]
    fn each_rider_remembers_their_own_devices() {
        let data = temp_dir("devices");
        assert_eq!(
            remembered_devices(&data, "anna"),
            RememberedDevices::default()
        );
        let kickr = RememberedDevice {
            id: "hci0/dev_AA".to_owned(),
            name: "KICKR CORE".to_owned(),
        };
        let pedals = RememberedDevice {
            id: "hci0/dev_BB".to_owned(),
            name: "ASSIOMA12345R".to_owned(),
        };
        set_active(&data, "anna").unwrap();

        remember_device(&data, "anna", DeviceRole::Trainer, kickr.clone()).unwrap();
        remember_device(&data, "anna", DeviceRole::PowerMeter, pedals.clone()).unwrap();
        remember_device(&data, "zoe", DeviceRole::Trainer, kickr.clone()).unwrap();

        let anna = remembered_devices(&data, "anna");
        assert_eq!(anna.trainer, Some(kickr.clone()));
        assert_eq!(anna.get(DeviceRole::PowerMeter), Some(&pedals));
        assert_eq!(anna.heart_rate, None);
        assert_eq!(
            remembered_devices(&data, "zoe").power_meter,
            None,
            "Anna's pedals are not Zoë's"
        );
        forget_device(&data, "anna", DeviceRole::PowerMeter).unwrap();
        assert_eq!(remembered_devices(&data, "anna").power_meter, None);
        assert_eq!(remembered_devices(&data, "anna").trainer, Some(kickr));
        assert_eq!(active(&data).as_deref(), Some("anna"), "the settings stay");
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn a_device_is_known_by_its_identifier_or_else_by_its_name() {
        let strap = RememberedDevice {
            id: "hci0/dev_AA".to_owned(),
            name: "Polar H9 ABCD".to_owned(),
        };

        assert!(strap.is("hci0/dev_AA", "Polar H9 ABCD"));
        assert!(strap.is("5F1C-other-computer", "Polar H9 ABCD"));
        assert!(!strap.is("hci0/dev_BB", "Polar H9 EFGH"));
    }

    #[test]
    fn a_new_rider_can_start_with_another_riders_devices() {
        let data = temp_dir("devices-copy");
        let click = RememberedDevice {
            id: "hci0/dev_CC".to_owned(),
            name: "Zwift Click".to_owned(),
        };
        remember_device(&data, "anna", DeviceRole::Controller, click.clone()).unwrap();
        set_cadence_source(&data, "anna", CadenceSource::PowerMeter).unwrap();
        let map = ButtonMap::shifting(2, 1);
        set_button_map(&data, "anna", ControllerModel::ZwiftClick, &map).unwrap();

        copy_devices(&data, "anna", "ben").unwrap();

        assert_eq!(remembered_devices(&data, "ben").controller, Some(click));
        assert_eq!(cadence_source(&data, "ben"), CadenceSource::PowerMeter);
        assert_eq!(button_map(&data, "ben", ControllerModel::ZwiftClick), map);
        forget_device(&data, "ben", DeviceRole::Controller).unwrap();
        assert!(
            remembered_devices(&data, "anna").controller.is_some(),
            "a copy: the riders choose apart from then on"
        );
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn each_rider_chooses_where_their_cadence_comes_from() {
        let data = temp_dir("cadence");
        assert_eq!(cadence_source(&data, "anna"), CadenceSource::Automatic);

        set_cadence_source(&data, "anna", CadenceSource::Trainer).unwrap();

        assert_eq!(cadence_source(&data, "anna"), CadenceSource::Trainer);
        assert_eq!(cadence_source(&data, "ben"), CadenceSource::Automatic);
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
