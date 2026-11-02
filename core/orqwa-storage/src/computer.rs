//! Settings of this computer (R78, #257): how detailed the 3D world is drawn (R43), the
//! graphics options set apart from that preset (R90), whether the app fills the screen (#218),
//! the phone remote (#228), whether courses' worlds are kept (R101) and the servers group rides
//! ask for its internet address (R192) depend on the machine in front of the rider, so they live
//! in `computer.toml` next to the cache, never in the data directory, which a rider may share
//! between computers (R30): there the last one to save would win.

use std::collections::BTreeMap;
use std::path::Path;

use orqwa_domain::season::Season;
use serde::{Deserialize, Serialize};

use crate::profiles::{GraphicsQuality, ProfileError, write_atomically};

/// The file's name, in the cache directory.
pub const FILE: &str = "computer.toml";

/// The file format. Missing fields take defaults.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct ComputerFile {
    /// Last season selected in the ride options (#350).
    season: String,
    /// How detailed the 3D world is drawn on this computer (R43).
    graphics_quality: Option<GraphicsQuality>,
    /// Whether the app fills the screen (#218).
    fullscreen: Option<bool>,
    /// Graphics options set apart from the preset (R90), by name; the others follow it.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    graphics: BTreeMap<String, String>,
    /// The phone remote (#228).
    remote: PhoneRemote,
    /// Whether ORQWA looks for a newer version at start-up (R99); on unless turned off.
    check_updates: Option<bool>,
    /// Whether the courses' built worlds are kept (R101); off unless turned on.
    keep_worlds: Option<bool>,
    /// Whether regions' map detail is downloaded for courses (ADR 0033); off unless turned on.
    map_detail: Option<bool>,
    /// The address lookup (STUN) servers group rides ask (R192); ORQWA's own when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stun_servers: Vec<String>,
}

/// The phone remote on this computer (#228): a phone pairs with the computer it rides beside.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhoneRemote {
    /// Whether it is on; off until the rider turns it on.
    pub on: bool,
    /// The port it listens on; `None` for the remote's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The secret in its address, once it has one; a new one locks out the phones paired
    /// before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// The graphics options a rider may set apart from the quality preset (R90), each with the
/// values it takes. An option left unset follows the preset ("auto"); the app turns the values
/// into engine settings.
pub const GRAPHICS_OPTIONS: &[(&str, &[&str])] = &[
    ("shadows", &["low", "medium", "high", "ultra"]),
    ("ambient_occlusion", &["off", "on"]),
    ("reflections", &["off", "on"]),
    ("bloom", &["off", "on"]),
    ("view_distance", &["low", "medium", "high", "ultra"]),
    ("tree_density", &["low", "medium", "high", "ultra"]),
    ("grass", &["off", "near", "medium", "far"]),
    ("antialiasing", &["off", "fxaa", "msaa_2x", "msaa_4x"]),
    ("render_scale", &["50", "67", "77", "85", "100"]),
    ("fps_limit", &["unlimited", "30", "60", "90", "120", "144"]),
    ("vsync", &["off", "on"]),
];

/// The value of a graphics option that follows the preset.
pub const AUTO: &str = "auto";

/// Last selected ride season; summer for settings saved before seasons existed.
#[must_use]
pub fn season(file: &Path) -> Season {
    Season::from_name(&read(file).season).unwrap_or_default()
}

/// Remembers the ride season without changing the other computer settings.
///
/// # Errors
/// On file system errors.
pub fn set_season(file: &Path, season: Season) -> Result<(), ProfileError> {
    let mut computer = read(file);
    season.name().clone_into(&mut computer.season);
    save(file, &computer)
}

#[cfg(test)]
mod season_tests {
    use super::*;

    #[test]
    fn seasons_survive_reload_without_resetting_graphics_or_future_settings() {
        let file = std::env::temp_dir().join(format!("orqwa-season-{}.toml", std::process::id()));
        std::fs::write(&file, "graphics_quality = 'high'\nseason = 'Future'\n").unwrap();
        assert_eq!(season(&file), Season::Summer);
        assert_eq!(graphics_quality(&file), GraphicsQuality::High);
        for chosen in Season::ALL {
            set_season(&file, *chosen).unwrap();
            assert_eq!(season(&file), *chosen);
            assert_eq!(graphics_quality(&file), GraphicsQuality::High);
            set_fullscreen(&file, true).unwrap();
            assert_eq!(season(&file), *chosen);
        }
        std::fs::remove_file(file).unwrap();
    }
}

/// The graphics options set on this computer (R90), by name; options following the preset
/// are missing. Values no longer known (from another version) are left out.
#[must_use]
pub fn graphics_options(file: &Path) -> BTreeMap<String, String> {
    read(file)
        .graphics
        .into_iter()
        .filter(|(name, value)| is_known(name, value))
        .collect()
}

/// Sets graphics option `name` to `value` on this computer (R90); [`AUTO`] lets it follow the
/// preset again. Returns false, changing nothing, for an unknown option or value.
///
/// # Errors
/// On file system errors.
pub fn set_graphics_option(file: &Path, name: &str, value: &str) -> Result<bool, ProfileError> {
    if value != AUTO && !is_known(name, value) {
        return Ok(false);
    }
    if !GRAPHICS_OPTIONS.iter().any(|(known, _)| *known == name) {
        return Ok(false);
    }
    let mut computer = read(file);
    if value == AUTO {
        computer.graphics.remove(name);
    } else {
        computer.graphics.insert(name.to_owned(), value.to_owned());
    }
    save(file, &computer)?;
    Ok(true)
}

fn is_known(name: &str, value: &str) -> bool {
    GRAPHICS_OPTIONS
        .iter()
        .any(|(known, values)| *known == name && values.contains(&value))
}

/// The graphics quality chosen on this computer; until one is chosen Medium, or Low on Android,
/// where tablets run the 3D world on far less than a computer's graphics card (ADR 0024).
#[must_use]
pub fn graphics_quality(file: &Path) -> GraphicsQuality {
    read(file)
        .graphics_quality
        .unwrap_or(if cfg!(target_os = "android") {
            GraphicsQuality::Low
        } else {
            GraphicsQuality::default()
        })
}

/// Remembers the graphics quality for this computer.
///
/// # Errors
/// On file system errors.
pub fn set_graphics_quality(file: &Path, quality: GraphicsQuality) -> Result<(), ProfileError> {
    let mut computer = read(file);
    computer.graphics_quality = Some(quality);
    save(file, &computer)
}

/// Saves the calibrated preset and resets its details together, so the next ride draws
/// exactly what the benchmark measured (R151).
///
/// # Errors
/// On file system errors; the previous settings are kept if the atomic write fails.
pub fn apply_graphics_benchmark(file: &Path, quality: GraphicsQuality) -> Result<(), ProfileError> {
    let mut computer = read(file);
    computer.graphics_quality = Some(quality);
    computer.graphics.clear();
    save(file, &computer)
}

/// Whether the app fills the screen on this computer (#218).
#[must_use]
pub fn fullscreen(file: &Path) -> bool {
    read(file).fullscreen.unwrap_or(false)
}

/// Remembers whether the app fills the screen, to start that way next time.
///
/// # Errors
/// On file system errors.
pub fn set_fullscreen(file: &Path, fullscreen: bool) -> Result<(), ProfileError> {
    let mut computer = read(file);
    computer.fullscreen = Some(fullscreen);
    save(file, &computer)
}

/// Whether this computer looks for a newer ORQWA at start-up (R99): yes until turned off.
#[must_use]
pub fn check_updates(file: &Path) -> bool {
    read(file).check_updates.unwrap_or(true)
}

/// Turns looking for a newer ORQWA at start-up on or off.
///
/// # Errors
/// [`ProfileError`] if the file cannot be written.
pub fn set_check_updates(file: &Path, on: bool) -> Result<(), ProfileError> {
    let mut computer = read(file);
    computer.check_updates = Some(on);
    save(file, &computer)
}

/// Whether this computer keeps the worlds of its courses once built (R101): no until turned
/// on, as they take a lot of room.
#[must_use]
pub fn keep_worlds(file: &Path) -> bool {
    read(file).keep_worlds.unwrap_or(false)
}

/// Turns keeping the courses' worlds on or off.
///
/// # Errors
/// [`ProfileError`] if the file cannot be written.
pub fn set_keep_worlds(file: &Path, on: bool) -> Result<(), ProfileError> {
    let mut computer = read(file);
    computer.keep_worlds = Some(on);
    save(file, &computer)
}

/// Whether this computer downloads regions' map detail for its courses (ADR 0033): no until
/// turned on, as a region's download takes from half a gigabyte to several.
#[must_use]
pub fn map_detail(file: &Path) -> bool {
    read(file).map_detail.unwrap_or(false)
}

/// Turns downloading regions' map detail on or off.
///
/// # Errors
/// [`ProfileError`] if the file cannot be written.
pub fn set_map_detail(file: &Path, on: bool) -> Result<(), ProfileError> {
    let mut computer = read(file);
    computer.map_detail = Some(on);
    save(file, &computer)
}

/// The address lookup (STUN) servers the rider named for group rides on this computer (R192),
/// as they were saved; empty when ORQWA asks its own. Lookups depend on the network the
/// computer is on, so they are kept with it.
#[must_use]
pub fn stun_servers(file: &Path) -> Vec<String> {
    read(file).stun_servers
}

/// Remembers the address lookup servers for group rides; empty for ORQWA's own.
///
/// # Errors
/// [`ProfileError`] if the file cannot be written.
pub fn set_stun_servers(file: &Path, servers: &[String]) -> Result<(), ProfileError> {
    let mut computer = read(file);
    computer.stun_servers = servers.to_vec();
    save(file, &computer)
}

/// The phone remote as set on this computer.
#[must_use]
pub fn phone_remote(file: &Path) -> PhoneRemote {
    read(file).remote
}

/// Remembers how the phone remote is set on this computer.
///
/// # Errors
/// On file system errors.
pub fn set_phone_remote(file: &Path, remote: &PhoneRemote) -> Result<(), ProfileError> {
    let mut computer = read(file);
    computer.remote = remote.clone();
    save(file, &computer)
}

/// Takes over the settings a data directory from before R78 kept for every computer, where
/// this computer has not chosen its own yet.
pub(crate) fn take_over(
    file: &Path,
    quality: Option<GraphicsQuality>,
    fullscreen: Option<bool>,
) -> Result<(), ProfileError> {
    let mut computer = read(file);
    computer.graphics_quality = computer.graphics_quality.or(quality);
    computer.fullscreen = computer.fullscreen.or(fullscreen);
    save(file, &computer)
}

fn read(file: &Path) -> ComputerFile {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(file: &Path, computer: &ComputerFile) -> Result<(), ProfileError> {
    write_atomically(file, &toml::to_string_pretty(computer)?)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orqwa-computer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join(FILE)
    }

    #[test]
    fn the_computer_keeps_its_graphics_quality_and_full_screen() {
        let file = temp_file("settings");
        assert_eq!(graphics_quality(&file), GraphicsQuality::Medium);
        assert!(!fullscreen(&file), "a window until the rider chooses");

        set_graphics_quality(&file, GraphicsQuality::Ultra).unwrap();
        set_fullscreen(&file, true).unwrap();

        assert_eq!(graphics_quality(&file), GraphicsQuality::Ultra);
        assert!(fullscreen(&file));
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains(r#"graphics_quality = "ultra""#), "{text}");
        set_fullscreen(&file, false).unwrap();
        assert!(!fullscreen(&file));
        assert_eq!(
            graphics_quality(&file),
            GraphicsQuality::Ultra,
            "kept with it"
        );
    }

    #[test]
    fn updates_are_looked_for_until_turned_off() {
        let file = temp_file("updates");
        assert!(check_updates(&file), "on for a new computer (R99)");

        set_check_updates(&file, false).unwrap();
        set_fullscreen(&file, true).unwrap();

        assert!(!check_updates(&file), "kept beside other settings");
        set_check_updates(&file, true).unwrap();
        assert!(check_updates(&file));
    }

    #[test]
    fn worlds_are_kept_once_turned_on() {
        let file = temp_file("keep-worlds");
        assert!(!keep_worlds(&file), "off for a new computer (R101)");

        set_keep_worlds(&file, true).unwrap();
        set_check_updates(&file, false).unwrap();

        assert!(keep_worlds(&file), "kept beside other settings");
        set_keep_worlds(&file, false).unwrap();
        assert!(!keep_worlds(&file));
    }

    #[test]
    fn map_detail_is_downloaded_once_turned_on() {
        let file = temp_file("map-detail");
        assert!(!map_detail(&file), "off for a new computer");

        set_map_detail(&file, true).unwrap();
        set_keep_worlds(&file, true).unwrap();

        assert!(map_detail(&file), "kept beside other settings");
        set_map_detail(&file, false).unwrap();
        assert!(!map_detail(&file));
        assert!(keep_worlds(&file));
    }

    #[test]
    fn group_rides_ask_the_named_lookup_servers_until_cleared() {
        let file = temp_file("stun-servers");
        assert_eq!(
            stun_servers(&file),
            Vec::<String>::new(),
            "ORQWA's own for a new computer"
        );
        let named = vec![
            "stun.example.org:3478".to_owned(),
            "192.0.2.1:3479".to_owned(),
        ];

        set_stun_servers(&file, &named).unwrap();
        set_map_detail(&file, true).unwrap();

        assert_eq!(stun_servers(&file), named, "kept beside other settings");
        set_stun_servers(&file, &[]).unwrap();
        assert_eq!(stun_servers(&file), Vec::<String>::new());
        assert!(map_detail(&file));
    }

    #[test]
    fn graphics_options_follow_the_preset_until_set_and_again_on_auto() {
        let file = temp_file("graphics-options");
        assert!(
            graphics_options(&file).is_empty(),
            "all follow the preset at first"
        );

        assert!(set_graphics_option(&file, "shadows", "ultra").unwrap());
        assert!(set_graphics_option(&file, "fps_limit", "60").unwrap());
        set_graphics_quality(&file, GraphicsQuality::Low).unwrap();

        let options = graphics_options(&file);
        assert_eq!(options.get("shadows").map(String::as_str), Some("ultra"));
        assert_eq!(options.get("fps_limit").map(String::as_str), Some("60"));
        assert_eq!(
            graphics_quality(&file),
            GraphicsQuality::Low,
            "the preset is kept apart"
        );
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("[graphics]") && text.contains(r#"shadows = "ultra""#),
            "{text}"
        );

        assert!(set_graphics_option(&file, "shadows", AUTO).unwrap());
        assert!(
            !graphics_options(&file).contains_key("shadows"),
            "auto follows the preset"
        );
    }

    #[test]
    fn benchmark_resets_details_and_preserves_other_computer_settings() {
        let file = temp_file("benchmark");
        set_graphics_quality(&file, GraphicsQuality::Ultra).unwrap();
        set_graphics_option(&file, "shadows", "ultra").unwrap();
        set_graphics_option(&file, "fps_limit", "30").unwrap();
        set_graphics_option(&file, "vsync", "off").unwrap();
        set_fullscreen(&file, true).unwrap();
        set_keep_worlds(&file, true).unwrap();
        set_check_updates(&file, false).unwrap();

        apply_graphics_benchmark(&file, GraphicsQuality::Medium).unwrap();

        assert_eq!(graphics_quality(&file), GraphicsQuality::Medium);
        assert!(graphics_options(&file).is_empty());
        assert!(fullscreen(&file));
        assert!(keep_worlds(&file));
        assert!(!check_updates(&file));
    }

    #[test]
    fn tree_density_is_kept_independently_of_quality_and_distance() {
        let file = temp_file("tree-density");
        assert!(!graphics_options(&file).contains_key("tree_density"));
        set_graphics_option(&file, "view_distance", "high").unwrap();
        for density in ["low", "medium", "high", "ultra"] {
            assert!(set_graphics_option(&file, "tree_density", density).unwrap());
            set_graphics_quality(&file, GraphicsQuality::Ultra).unwrap();
            let options = graphics_options(&file);
            assert_eq!(
                options.get("tree_density").map(String::as_str),
                Some(density)
            );
            assert_eq!(
                options.get("view_distance").map(String::as_str),
                Some("high")
            );
        }
        assert!(!set_graphics_option(&file, "tree_density", "off").unwrap());
        assert!(set_graphics_option(&file, "tree_density", AUTO).unwrap());
        let options = graphics_options(&file);
        assert!(!options.contains_key("tree_density"));
        assert_eq!(
            options.get("view_distance").map(String::as_str),
            Some("high")
        );
        assert_eq!(graphics_quality(&file), GraphicsQuality::Ultra);
    }

    #[test]
    fn unknown_graphics_options_and_values_are_refused_and_not_read() {
        let file = temp_file("graphics-unknown");
        assert!(!set_graphics_option(&file, "shadows", "extreme").unwrap());
        assert!(!set_graphics_option(&file, "ray_tracing", "on").unwrap());
        assert!(!set_graphics_option(&file, "ray_tracing", AUTO).unwrap());
        assert!(graphics_options(&file).is_empty());

        // A file from another version may hold values this one does not know.
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            "[graphics]\nshadows = \"extreme\"\nbloom = \"off\"\n",
        )
        .unwrap();
        let options = graphics_options(&file);
        assert_eq!(options.len(), 1, "{options:?}");
        assert_eq!(options.get("bloom").map(String::as_str), Some("off"));
    }

    #[test]
    fn the_computer_keeps_its_phone_remote_off_until_turned_on() {
        let file = temp_file("remote");
        assert_eq!(
            phone_remote(&file),
            PhoneRemote::default(),
            "off, nothing chosen"
        );
        set_fullscreen(&file, true).unwrap();
        assert!(!phone_remote(&file).on, "other settings leave it off");

        let remote = PhoneRemote {
            on: true,
            port: Some(9000),
            token: Some("0123456789abcdef".to_owned()),
        };
        set_phone_remote(&file, &remote).unwrap();

        assert_eq!(phone_remote(&file), remote);
        assert!(fullscreen(&file), "kept with it");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("[remote]"), "{text}");
    }

    #[test]
    fn settings_taken_over_never_replace_the_computers_own() {
        let file = temp_file("take-over");
        set_fullscreen(&file, false).unwrap();

        take_over(&file, Some(GraphicsQuality::Low), Some(true)).unwrap();

        assert_eq!(
            graphics_quality(&file),
            GraphicsQuality::Low,
            "none chosen yet"
        );
        assert!(!fullscreen(&file), "chosen before");
        take_over(&file, Some(GraphicsQuality::Ultra), None).unwrap();
        assert_eq!(graphics_quality(&file), GraphicsQuality::Low);
    }
}
