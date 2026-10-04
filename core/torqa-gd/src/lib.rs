//! Godot GDExtension bindings exposing the Torqa core to the presentation layer.
//!
//! This crate only translates between Godot types and [`torqa_app`]; it holds no logic.

// This crate is the FFI boundary: gdext's entry point is an `unsafe impl`, and the
// `#[gdextension]` macro drops item-level attributes, so the allow must be crate-wide.
#![allow(unsafe_code)]

use std::path::PathBuf;
use std::time::Duration;

use godot::classes::image::Format as ImageFormat;
use godot::classes::{Engine, INode, Image, Node};
use godot::prelude::*;
use torqa_app::hud::MetricKind;
use torqa_app::media::MediaCommand;
use torqa_app::view;
use torqa_app::{App, AppEvent, GhostChoice, TrainerChoice, paths};
use torqa_devices::ble::DeviceKind;
use torqa_devices::fake::FakeRider;
use torqa_domain::profile::{Profile, UnitSystem};
use torqa_domain::units::{BeatsPerMinute, Kilograms, Meters, Percent, Rpm, Watts};
use torqa_physics::DescentMode;
use torqa_routes::{ElevationSource, LocalProjection};

struct TorqaExtension;

#[gdextension]
unsafe impl ExtensionLibrary for TorqaExtension {}

/// Static information about the Torqa core.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct TorqaCore;

#[godot_api]
impl TorqaCore {
    /// Version of the Torqa core.
    #[func]
    fn version() -> GString {
        GString::from(torqa_domain::version())
    }
}

/// The Torqa application as a node: add it to the scene tree, call its commands and listen to
/// its signals. It advances rides in `_process`.
#[derive(GodotClass)]
#[class(base = Node)]
pub struct TorqaApp {
    base: Base<Node>,
    app: Option<App>,
}

#[godot_api]
impl INode for TorqaApp {
    fn init(base: Base<Node>) -> Self {
        // The editor instantiates nodes too; it must not start runtimes or touch Bluetooth.
        let app = if Engine::singleton().is_editor_hint() {
            None
        } else {
            App::new(paths::data_dir(), paths::cache_dir())
                .inspect_err(|error| {
                    godot_error!("Torqa: {error}");
                })
                .ok()
        };
        Self { base, app }
    }

    fn process(&mut self, delta: f64) {
        let Some(app) = self.app.as_mut() else {
            return;
        };
        let events = app.update(Duration::from_secs_f64(delta.max(0.0)));
        for event in events {
            self.emit(event);
        }
    }

    fn exit_tree(&mut self) {
        if let Some(app) = self.app.as_mut() {
            app.shutdown();
        }
    }
}

#[godot_api]
impl TorqaApp {
    /// A scan finished: an array of dictionaries `{index, name, kind ("trainer" or
    /// "heart_rate"), rssi}`.
    #[signal]
    fn devices_found(devices: VarArray);

    /// Preparing a course advanced: what is being done, the unit counted, done and total.
    #[signal]
    fn loading_progress(step: GString, unit: GString, done: i64, total: i64);

    /// A route was imported: `{name, length_m, elevation_gain_m, max_grade, elevation_source}`.
    #[signal]
    fn route_loaded(route: VarDictionary);

    /// The 3D world for the loaded route is ready: `{chunks, fallback_samples}`.
    #[signal]
    fn world_ready(world: VarDictionary);

    /// A device connected (also after reconnecting).
    #[signal]
    fn device_connected(name: GString);

    /// A device lost its connection and is reconnecting.
    #[signal]
    fn device_disconnected(name: GString);

    /// The rider reached the finish.
    #[signal]
    fn ride_finished();

    /// The rider reached the top of climb `index` (as in `climbs()`); `previous_best_s` is
    /// −1 without an earlier time.
    #[signal]
    fn climb_completed(index: i64, elapsed_s: f64, previous_best_s: f64);

    /// The rider finished the route; `previous_best_s` is −1 without an earlier time.
    #[signal]
    fn route_completed(elapsed_s: f64, previous_best_s: f64);

    /// The ride was saved as a FIT file.
    #[signal]
    fn ride_saved(path: GString);

    /// A course was saved or imported into the library.
    #[signal]
    fn course_added(path: GString);

    /// Devices used last that the reconnect at start did not find, by name.
    #[signal]
    fn remembered_missing(names: PackedStringArray);

    /// Something went wrong.
    #[signal]
    fn failed(message: GString);

    /// Reconnects the trainer and heart-rate sensor used last, in the background (emits
    /// `devices_found`, `device_connected` and, for those not found, `remembered_missing`).
    /// False if none is remembered.
    #[func]
    fn reconnect_remembered(&mut self) -> bool {
        self.app.as_mut().is_some_and(App::reconnect_remembered)
    }

    /// Scans for trainers and heart-rate sensors.
    #[func]
    fn scan(&mut self, seconds: f64) {
        if let Some(app) = self.app.as_mut() {
            app.scan(Duration::from_secs_f64(seconds.max(1.0)));
        }
    }

    /// Imports a GPX file; `offline` uses cached terrain only.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn load_route(&mut self, path: GString, offline: bool) {
        if let Some(app) = self.app.as_mut() {
            app.load_route(PathBuf::from(path.to_string()), offline);
        }
    }

    /// Prepares a video course (R17) from a GoPro video with GPS or an Incyclist route video's
    /// `.xml` file; emits `route_loaded`, `world_ready` and `course_added` like `load_route`.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn load_video(&mut self, path: GString, offline: bool) {
        if let Some(app) = self.app.as_mut() {
            app.load_video(PathBuf::from(path.to_string()), offline);
        }
    }

    /// File extensions `load_video` accepts, for file dialogs: videos and Incyclist's `xml`.
    #[func]
    fn video_extensions() -> PackedStringArray {
        torqa_app::video::VIDEO_EXTENSIONS
            .iter()
            .chain(&["xml"])
            .map(|e| GString::from(*e))
            .collect()
    }

    /// The loaded video course: `{path, duration_s, offset_s, aligned_by_hand, marks}`;
    /// `marks` (x metres along the route, y seconds into the video) are those of a video
    /// placed on the route by hand, from the route's start to its end. Empty for other courses.
    #[func]
    fn video(&self) -> VarDictionary {
        let Some(video) = self.app.as_ref().and_then(App::video) else {
            return VarDictionary::new();
        };
        let path = video.video.display().to_string();
        vdict! {
            "path" => path.as_str(),
            "duration_s" => video.duration.as_secs_f64(),
            "offset_s" => video.offset.as_secs_f64(),
            "aligned_by_hand" => video.aligned_by_hand(),
            "marks" => &video
                .marks
                .iter()
                .map(|m| vector2(m.distance.0, m.time.as_secs_f64()))
                .collect::<PackedVector2Array>(),
        }
    }

    /// A video about to be imported: `{duration_s, has_gps}`; empty (and `failed`) if it
    /// cannot be read. Without GPS it is added to a GPX course with `add_video`.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn video_probe(&mut self, path: GString) -> VarDictionary {
        match torqa_app::video::probe(&PathBuf::from(path.to_string())) {
            Ok(probe) => vdict! {
                "duration_s" => probe.duration.as_secs_f64(),
                "has_gps" => probe.has_gps,
            },
            Err(message) => {
                self.signals()
                    .failed()
                    .emit(&GString::from(message.as_str()));
                VarDictionary::new()
            }
        }
    }

    /// Adds a video (e.g. one without GPS) to the loaded GPX course, placed by `marks` (x
    /// metres along the route, y seconds into the video, from the route's start to its end).
    /// The course becomes a video course; emits `failed` if that is not possible.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn add_video(&mut self, video: GString, marks: PackedVector2Array) -> bool {
        let video = PathBuf::from(video.to_string());
        let marks = sync_marks(&marks);
        self.command(|app| app.add_video(&video, &marks))
    }

    /// Takes a video added with `add_video` off the loaded course, which is reopened as a 3D
    /// course (emits `route_loaded`).
    #[func]
    fn remove_video(&mut self) -> bool {
        self.command(App::remove_video)
    }

    /// Replaces the marks of the loaded video course (see `add_video`); emits `failed` if they
    /// do not fit.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn align_video(&mut self, marks: PackedVector2Array) -> bool {
        let marks = sync_marks(&marks);
        self.command(|app| app.align_video(&marks))
    }

    /// The frame of the video at `path` shown `time_s` seconds in, e.g. to align it; `null`
    /// if it cannot be decoded. Call `close_video_preview` when done.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn video_preview(&mut self, path: GString, time_s: f64) -> Option<Gd<Image>> {
        let frame = self
            .app
            .as_mut()?
            .video_preview(&PathBuf::from(path.to_string()), seconds(time_s))
            .ok()?;
        image(frame)
    }

    /// Closes the video opened by `video_preview`.
    #[func]
    fn close_video_preview(&mut self) {
        if let Some(app) = self.app.as_mut() {
            app.close_video_preview();
        }
    }

    /// The moment of the video to show now, in seconds; -1 when not riding a video course.
    #[func]
    fn video_time(&self) -> f64 {
        self.app
            .as_ref()
            .and_then(App::video_time)
            .map_or(-1.0, |t| t.as_secs_f64())
    }

    /// The next video frame during a ride on a video course, once decoded: `{time_s, image}`
    /// (an RGBA8 `Image`); empty while none is new. It is the frame following `video_time()`,
    /// to blend towards from the previous one.
    #[func]
    fn video_frame(&mut self) -> VarDictionary {
        let Some(frame) = self.app.as_mut().and_then(App::video_frame) else {
            return VarDictionary::new();
        };
        let time_s = frame.time.as_secs_f64();
        let Some(image) = image(frame) else {
            return VarDictionary::new();
        };
        vdict! {
            "time_s" => time_s,
            "image" => &image,
        }
    }

    /// Opens a course file from the library or elsewhere; emits `route_loaded` like
    /// `load_route`. Its 3D world is built by `build_world`.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn open_course(&mut self, path: GString) {
        if let Some(app) = self.app.as_mut() {
            app.open_course(PathBuf::from(path.to_string()));
        }
    }

    /// Builds the opened course's 3D world, for riding it (emits `world_ready`); true if it is
    /// ready already.
    #[func]
    fn build_world(&mut self) -> bool {
        self.app.as_mut().is_some_and(App::build_world)
    }

    /// The course file of the loaded route; empty if it is not one (yet).
    #[func]
    fn loaded_course(&self) -> GString {
        self.app
            .as_ref()
            .and_then(App::loaded_course)
            .map_or_else(GString::new, |p| {
                GString::from(p.display().to_string().as_str())
            })
    }

    /// Whether a route is loaded (its figures, climbs and records are available).
    #[func]
    fn has_route(&self) -> bool {
        self.app.as_ref().is_some_and(|app| app.route().is_some())
    }

    /// Renames a course in the library; emits `failed` on errors (e.g. a blank name).
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn rename_course(&mut self, path: GString, name: GString) -> bool {
        let path = PathBuf::from(path.to_string());
        let name = name.to_string();
        self.command(|app| app.rename_course(&path, &name))
    }

    /// Deletes a course from the library; rides on it stay in the history.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn delete_course(&mut self, path: GString) -> bool {
        let path = PathBuf::from(path.to_string());
        self.command(|app| app.delete_course(&path))
    }

    /// Saves the loaded route as a course in the library (emits `course_added` or `failed`).
    /// False until the world is ready.
    #[func]
    fn save_course(&mut self) -> bool {
        self.command(App::save_course)
    }

    /// Copies a course file into the library (emits `course_added` or `failed`).
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn import_course(&mut self, path: GString) {
        if let Some(app) = self.app.as_mut() {
            app.import_course(PathBuf::from(path.to_string()));
        }
    }

    /// The courses in the library: `[{path, name, length_m, elevation_gain_m, max_grade,
    /// created_unix_s, track, profile, video}]`; `track` (metres east/north of the start) and
    /// `profile` (distance, elevation) are thinned for cards and empty for older courses;
    /// `video` is the video's file name for video courses, else empty.
    #[func]
    fn courses(&self) -> VarArray {
        let mut array = VarArray::new();
        for course in self.app.as_ref().map(App::courses).unwrap_or_default() {
            let path = course.path.display().to_string();
            let video = course
                .manifest
                .video
                .as_ref()
                .map_or("", |v| v.file_name.as_str());
            array.push(
                &vdict! {
                    "path" => path.as_str(),
                    "name" => course.manifest.name.as_str(),
                    "length_m" => course.manifest.length_m,
                    "elevation_gain_m" => course.manifest.elevation_gain_m,
                    "max_grade" => course.manifest.max_grade_percent,
                    "created_unix_s" => i64::try_from(course.manifest.created_unix_s).unwrap_or(0),
                    "track" => &points(&course.manifest.track),
                    "profile" => &points(&course.manifest.profile),
                    "video" => video,
                }
                .to_variant(),
            );
        }
        array
    }

    /// The active rider's rides, newest first: `[{path, route, name, start_unix_s, elapsed_s,
    /// distance_m, elevation_gain_m, avg_speed_kmh, avg_power, max_power, normalized_power,
    /// intensity_factor, training_stress, work_kj, avg_cadence, avg_heart_rate,
    /// max_heart_rate}]`; values the ride did not record are `null`.
    #[func]
    fn history(&self) -> VarArray {
        let optional = |value: Option<f64>| value.map_or_else(Variant::nil, |v| v.to_variant());
        let mut array = VarArray::new();
        for entry in self.app.as_ref().map(App::history).unwrap_or_default() {
            let s = &entry.record.summary;
            let path = entry.fit.display().to_string();
            let start = entry
                .record
                .start
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
            array.push(
                &vdict! {
                    "path" => path.as_str(),
                    "route" => entry.record.route.as_str(),
                    "name" => entry.record.name.as_deref().unwrap_or_default(),
                    "start_unix_s" => start,
                    "elapsed_s" => s.elapsed.as_secs_f64(),
                    "distance_m" => s.distance.0,
                    "elevation_gain_m" => s.elevation_gain.0,
                    "avg_speed_kmh" => s.avg_speed.as_kilometers_per_hour(),
                    "avg_power" => &optional(s.avg_power.map(|v| v.0)),
                    "max_power" => &optional(s.max_power.map(|v| v.0)),
                    "normalized_power" => &optional(s.normalized_power.map(|v| v.0)),
                    "intensity_factor" => &optional(s.intensity_factor),
                    "training_stress" => &optional(s.training_stress),
                    "work_kj" => &optional(s.work.map(|w| w.0 / 1000.0)),
                    "avg_cadence" => &optional(s.avg_cadence.map(|v| v.0)),
                    "avg_heart_rate" => &optional(s.avg_heart_rate.map(|v| v.0)),
                    "max_heart_rate" => &optional(s.max_heart_rate.map(|v| v.0)),
                    "route_time_s" => &optional(entry.record.route_time.map(|t| t.as_secs_f64())),
                    "route_record" => entry.route_record,
                    "climbs" => &climb_times(&entry),
                }
                .to_variant(),
            );
        }
        array
    }

    /// Charts of one ride as `(elapsed_s, value)` points — `power`, `heart_rate`, `cadence`,
    /// `speed_kmh`, `elevation_m` — and seconds per zone of the active rider in `power_zones`
    /// (7) and `heart_rate_zones` (5). Empty (and emits `failed`) if the file cannot be read.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn ride_detail(&mut self, path: GString, max_points: i64) -> VarDictionary {
        let Some(app) = self.app.as_ref() else {
            return VarDictionary::new();
        };
        let detail = match app.ride_detail(std::path::Path::new(&path.to_string())) {
            Ok(detail) => detail,
            Err(error) => {
                let message = error.to_string();
                self.signals()
                    .failed()
                    .emit(&GString::from(message.as_str()));
                return VarDictionary::new();
            }
        };
        let max_points = usize::try_from(max_points).unwrap_or(500);
        let series = |value: fn(&torqa_domain::recording::Sample) -> Option<f64>| {
            view::ride_series(&detail.samples, max_points, value)
                .into_iter()
                .map(|(a, b)| vector2(a, b))
                .collect::<PackedVector2Array>()
        };
        let seconds = |zones: &[Duration]| {
            zones
                .iter()
                .map(Duration::as_secs_f64)
                .collect::<PackedFloat64Array>()
        };
        vdict! {
            "power" => &series(|s| s.power.map(|p| p.0)),
            "heart_rate" => &series(|s| s.heart_rate.map(|h| h.0)),
            "cadence" => &series(|s| s.cadence.map(|c| c.0)),
            "speed_kmh" => &series(|s| Some(s.speed.as_kilometers_per_hour())),
            "elevation_m" => &series(|s| Some(s.elevation.0)),
            "power_zones" => &seconds(&detail.power_zones),
            "heart_rate_zones" => &seconds(&detail.heart_rate_zones),
        }
    }

    /// Names a ride (empty: back to route and date); emits `failed` on errors.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn rename_ride(&mut self, path: GString, name: GString) -> bool {
        let path = PathBuf::from(path.to_string());
        let name = name.to_string();
        self.command(|app| app.rename_ride(&path, &name))
    }

    /// Deletes a ride from the history.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn delete_ride(&mut self, path: GString) -> bool {
        let path = PathBuf::from(path.to_string());
        self.command(|app| app.delete_ride(&path))
    }

    /// Whether the trainer is connected now (rides start at once then).
    #[func]
    fn trainer_connected(&self) -> bool {
        self.app.as_ref().is_some_and(App::trainer_connected)
    }

    /// Connects the simulated trainer.
    #[func]
    fn connect_fake_trainer(&mut self, power: f64, cadence: f64) -> bool {
        let choice = TrainerChoice::Fake(FakeRider {
            power: Watts(power),
            cadence: Rpm(cadence),
        });
        self.command(|app| app.connect_trainer(choice))
    }

    /// Connects a scanned trainer by its index.
    #[func]
    fn connect_trainer(&mut self, index: i64) -> bool {
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        self.command(|app| app.connect_trainer(TrainerChoice::Discovered(index)))
    }

    /// Connects a scanned heart-rate sensor by its index.
    #[func]
    fn connect_heart_rate(&mut self, index: i64) -> bool {
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        self.command(|app| app.connect_heart_rate(index))
    }

    /// Starts riding the loaded route as the active rider, against `ghost`: `{kind}` with
    /// `kind` one of `none`, `best` (own best on the route), `power` (`{watts}`), `wkg`
    /// (`{watts_per_kg}`) or `activity` (`{path}` of a GPX or FIT file). Emits `failed` and
    /// returns false if it cannot start, e.g. no best time yet.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn start_ride(&mut self, difficulty: f64, flat_descents: bool, ghost: VarDictionary) -> bool {
        let descent = if flat_descents {
            DescentMode::Flat
        } else {
            DescentMode::Coast
        };
        let number = |key: &str| {
            ghost
                .get(key)
                .and_then(|v| v.try_to::<f64>().ok())
                .unwrap_or(0.0)
        };
        let kind = ghost
            .get("kind")
            .and_then(|v| v.try_to::<GString>().ok())
            .map(|k| k.to_string())
            .unwrap_or_default();
        let choice = match kind.as_str() {
            "best" => GhostChoice::PersonalBest,
            "power" => GhostChoice::Power(Watts(number("watts"))),
            "wkg" => GhostChoice::WattsPerKg(number("watts_per_kg")),
            "activity" => GhostChoice::Activity(PathBuf::from(
                ghost
                    .get("path")
                    .and_then(|v| v.try_to::<GString>().ok())
                    .map(|p| p.to_string())
                    .unwrap_or_default(),
            )),
            _ => GhostChoice::None,
        };
        self.command(|app| app.start_ride(Percent(difficulty), descent, &choice))
    }

    /// Controls the rider's music app: `play_pause`, `next` or `previous` (emits `failed` if
    /// no player reacts).
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn control_music(&mut self, command: GString) {
        let command = match command.to_string().as_str() {
            "next" => MediaCommand::Next,
            "previous" => MediaCommand::Previous,
            _ => MediaCommand::PlayPause,
        };
        if let Some(app) = self.app.as_mut() {
            app.control_music(command);
        }
    }

    /// Changes difficulty and descent mode of the current ride.
    #[func]
    fn adjust_ride(&mut self, difficulty: f64, flat_descents: bool) {
        let descent = if flat_descents {
            DescentMode::Flat
        } else {
            DescentMode::Coast
        };
        if let Some(app) = self.app.as_mut() {
            app.adjust_ride(Percent(difficulty), descent);
        }
    }

    /// Ends the current ride without saving it.
    #[func]
    fn abort_ride(&mut self) {
        if let Some(app) = self.app.as_mut() {
            app.abort_ride();
        }
    }

    /// Whether the active rider has finished the loaded route before, so `best` can be raced.
    #[func]
    fn has_personal_best(&self) -> bool {
        self.app
            .as_ref()
            .and_then(|app| app.route().map(|route| app.records_for(route)))
            .is_some_and(|records| records.route.is_some())
    }

    /// All riders: `[{id, name}]`, by name.
    #[func]
    fn profiles(&self) -> VarArray {
        let mut array = VarArray::new();
        for stored in self.app.as_ref().map(App::profiles).unwrap_or_default() {
            array.push(
                &vdict! {
                    "id" => stored.id.as_str(),
                    "name" => stored.profile.name.as_str(),
                }
                .to_variant(),
            );
        }
        array
    }

    /// The active rider: `{id, name, rider_mass_kg, bike_mass_kg, ftp_w, max_heart_rate_bpm,
    /// units, language}` with `units` either `"metric"` or `"imperial"` and `language` a locale
    /// code, empty for the system language.
    #[func]
    fn profile(&self) -> VarDictionary {
        let Some(stored) = self.app.as_ref().map(App::profile) else {
            return VarDictionary::new();
        };
        let p = &stored.profile;
        vdict! {
            "id" => stored.id.as_str(),
            "name" => p.name.as_str(),
            "rider_mass_kg" => p.rider_mass.0,
            "bike_mass_kg" => p.bike_mass.0,
            "ftp_w" => p.ftp.0,
            "max_heart_rate_bpm" => p.max_heart_rate.0,
            "units" => match p.units {
                UnitSystem::Metric => "metric",
                UnitSystem::Imperial => "imperial",
            },
            "language" => p.language.as_str(),
        }
    }

    /// Switches to another rider.
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn select_profile(&mut self, id: GString) -> bool {
        self.command(|app| app.select_profile(&id.to_string()))
    }

    /// Saves a rider (a new one if `id` is empty) from a dictionary shaped like `profile()`
    /// and makes it active; returns its id, or an empty string on failure (emits `failed`).
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn save_profile(&mut self, id: GString, data: VarDictionary) -> GString {
        let number = |key: &str, default: f64| {
            data.get(key)
                .and_then(|v| v.try_to::<f64>().ok())
                .unwrap_or(default)
        };
        let defaults = Profile::default();
        let profile = Profile {
            name: data
                .get("name")
                .and_then(|v| v.try_to::<GString>().ok())
                .map_or(defaults.name, |n| n.to_string()),
            rider_mass: Kilograms(number("rider_mass_kg", defaults.rider_mass.0)),
            bike_mass: Kilograms(number("bike_mass_kg", defaults.bike_mass.0)),
            ftp: Watts(number("ftp_w", defaults.ftp.0)),
            max_heart_rate: BeatsPerMinute(number("max_heart_rate_bpm", defaults.max_heart_rate.0)),
            language: data
                .get("language")
                .and_then(|v| v.try_to::<GString>().ok())
                .map(|l| l.to_string())
                .unwrap_or_default(),
            units: if data
                .get("units")
                .and_then(|v| v.try_to::<GString>().ok())
                .is_some_and(|u| u == "imperial")
            {
                UnitSystem::Imperial
            } else {
                UnitSystem::Metric
            },
        };
        let id = id.to_string();
        let id = (!id.is_empty()).then_some(id);
        let Some(app) = self.app.as_mut() else {
            return GString::new();
        };
        match app.save_profile(id.as_deref(), profile) {
            Ok(id) => GString::from(id.as_str()),
            Err(error) => {
                let message = error.to_string();
                self.signals()
                    .failed()
                    .emit(&GString::from(message.as_str()));
                GString::new()
            }
        }
    }

    /// Ends the ride and saves it (emits `ride_saved` or `failed`).
    #[func]
    fn finish_ride(&mut self) {
        let Some(app) = self.app.as_mut() else {
            return;
        };
        for event in app.finish_ride() {
            self.emit(event);
        }
    }

    /// The ride state: `{elapsed_s, distance_m, remaining_m, speed_kmh, grade, elevation_m, x, y,
    /// heading, power, cadence, heart_rate, watts_per_kg, power_zone, heart_rate_zone, metrics,
    /// ghost, climb}`; sensor
    /// values and what derives from them are `null` when unknown. Empty when
    /// not riding. `x`/`y` are metres east/north of the route start, as in `track()`; `heading`
    /// is the direction of travel in radians clockwise from north.
    #[func]
    fn ride_state(&self) -> VarDictionary {
        let Some(app) = self.app.as_ref() else {
            return VarDictionary::new();
        };
        let (Some(state), Some(route)) = (app.ride_state(), app.route()) else {
            return VarDictionary::new();
        };
        let (x, y) =
            LocalProjection::for_route(route).project(state.position.lat, state.position.lon);
        let optional = |value: Option<f64>| value.map_or_else(Variant::nil, |v| v.to_variant());
        let t = state.telemetry;
        let rider = &app.profile().profile;
        let zone =
            |value: Option<u8>| value.map_or_else(Variant::nil, |z| i64::from(z).to_variant());
        vdict! {
            "elapsed_s" => state.elapsed.as_secs_f64(),
            "distance_m" => state.distance.0,
            "remaining_m" => state.remaining.0,
            "speed_kmh" => state.speed.as_kilometers_per_hour(),
            "grade" => state.position.grade.0,
            "elevation_m" => state.position.elevation.0,
            "x" => x,
            "y" => y,
            "heading" => state.position.heading,
            "power" => &optional(t.power.map(|p| p.0)),
            "cadence" => &optional(t.cadence.map(|c| c.0)),
            "heart_rate" => &optional(t.heart_rate.map(|h| h.0)),
            "watts_per_kg" => &optional(t.power.map(|p| rider.watts_per_kg(p))),
            "power_zone" => &zone(t.power.map(|p| rider.power_zone(p))),
            "heart_rate_zone" => &zone(t.heart_rate.map(|h| rider.heart_rate_zone(h))),
            "metrics" => &hud_values(app),
            "ghost" => &app.ghost_state().map_or_else(Variant::nil, |g| {
                let at = route.position(g.distance);
                let (gx, gy) = LocalProjection::for_route(route).project(at.lat, at.lon);
                vdict! {
                    "name" => g.name.as_str(),
                    "distance_m" => g.distance.0,
                    "x" => gx,
                    "y" => gy,
                    "elevation_m" => at.elevation.0,
                    "heading" => at.heading,
                    "grade" => at.grade.0,
                    "gap_s" => &optional(g.gap),
                }
                .to_variant()
            }),
            "climb" => &app.current_climb().map_or_else(Variant::nil, |c| {
                let count = i64::try_from(route.climbs().len()).unwrap_or(0);
                vdict! {
                    "index" => i64::try_from(c.index).unwrap_or(0),
                    "count" => count,
                    "category" => c.climb.category.label(),
                    "length_m" => c.climb.length().0,
                    "ridden_m" => c.ridden.0,
                    "grade" => c.climb.average_grade.0,
                    "elapsed_s" => c.elapsed.as_secs_f64(),
                    "best_s" => &optional(c.best.map(|b| b.as_secs_f64())),
                }
                .to_variant()
            }),
        }
    }

    /// Every metric the HUD can show: `[{id, caption, unit, decimals, kind, sample}]`, with `kind` one of
    /// `number`, `speed`, `distance`, `elevation`, `duration`, `grade`, `zone`. Values come in
    /// `ride_state()["metrics"]` in km/h, km, m and s.
    #[func]
    fn hud_metrics() -> VarArray {
        let mut array = VarArray::new();
        for metric in torqa_app::hud::METRICS {
            let kind = match metric.kind {
                MetricKind::Number => "number",
                MetricKind::Speed => "speed",
                MetricKind::Distance => "distance",
                MetricKind::Elevation => "elevation",
                MetricKind::Duration => "duration",
                MetricKind::Grade => "grade",
                MetricKind::Zone => "zone",
            };
            array.push(
                &vdict! {
                    "id" => metric.id,
                    "caption" => metric.caption,
                    "unit" => metric.unit,
                    "decimals" => i64::from(metric.decimals),
                    "kind" => kind,
                    "sample" => metric.sample,
                }
                .to_variant(),
            );
        }
        array
    }

    /// The HUD metric ids a new rider starts with.
    #[func]
    fn hud_default_layout() -> PackedStringArray {
        torqa_app::hud::DEFAULT_LAYOUT
            .iter()
            .map(|&id| GString::from(id))
            .collect()
    }

    /// Most metrics a HUD layout may hold.
    #[func]
    fn hud_max_metrics() -> i64 {
        i64::try_from(torqa_app::hud::MAX_METRICS).unwrap_or(i64::MAX)
    }

    /// The active rider's HUD metric ids, in order; the first is shown large.
    #[func]
    fn hud_layout(&self) -> PackedStringArray {
        self.app
            .as_ref()
            .map(App::hud_layout)
            .unwrap_or_default()
            .iter()
            .map(|id| GString::from(id.as_str()))
            .collect()
    }

    /// Saves the active rider's HUD metrics and returns them as saved (unknown ids dropped).
    #[func]
    #[allow(clippy::needless_pass_by_value)] // #[func] parameters are passed by value from Godot
    fn set_hud_layout(&mut self, layout: PackedStringArray) -> PackedStringArray {
        let ids: Vec<String> = layout.as_slice().iter().map(ToString::to_string).collect();
        let Some(app) = self.app.as_mut() else {
            return PackedStringArray::new();
        };
        match app.set_hud_layout(&ids) {
            Ok(saved) => saved.iter().map(|id| GString::from(id.as_str())).collect(),
            Err(error) => {
                let message = error.to_string();
                self.signals()
                    .failed()
                    .emit(&GString::from(message.as_str()));
                PackedStringArray::new()
            }
        }
    }

    /// The loaded route's climbs with the active rider's best times: `{route_best_s,
    /// climbs: [{start_m, end_m, length_m, gain_m, grade, category, best_s}]}`; times are `null`
    /// without an earlier ride.
    #[func]
    fn climbs(&self) -> VarDictionary {
        let Some((app, route)) = self.app.as_ref().and_then(|a| a.route().map(|r| (a, r))) else {
            return VarDictionary::new();
        };
        let optional = |value: Option<Duration>| {
            value.map_or_else(Variant::nil, |v| v.as_secs_f64().to_variant())
        };
        let records = app.records_for(route);
        let mut climbs = VarArray::new();
        for (index, climb) in route.climbs().iter().enumerate() {
            climbs.push(
                &vdict! {
                    "start_m" => climb.start.0,
                    "end_m" => climb.end.0,
                    "length_m" => climb.length().0,
                    "gain_m" => climb.gain.0,
                    "grade" => climb.average_grade.0,
                    "category" => climb.category.label(),
                    "best_s" => &optional(records.climbs.get(index).copied().flatten()),
                }
                .to_variant(),
            );
        }
        vdict! {
            "route_best_s" => &optional(records.route),
            "climbs" => &climbs,
        }
    }

    /// Number of terrain chunks in the generated world (0 until `world_ready`).
    #[func]
    fn world_chunk_count(&self) -> i64 {
        self.app
            .as_ref()
            .and_then(App::world)
            .map_or(0, |world| i64::try_from(world.chunks.len()).unwrap_or(0))
    }

    /// World chunk `index`: `{center, terrain, buildings, conifers, broadleaves}`. `terrain`
    /// and `buildings` are mesh arrays (`{vertices, normals, uvs, colors, indices}`), the tree
    /// entries `MultiMesh` transform buffers. Geometry is relative to `center`, in Godot
    /// coordinates (x east, y up, −z north, metres from the route start).
    #[func]
    fn world_chunk(&self, index: i64) -> VarDictionary {
        let chunk = self
            .app
            .as_ref()
            .and_then(App::world)
            .zip(usize::try_from(index).ok())
            .and_then(|(world, index)| world.chunks.get(index));
        let Some(chunk) = chunk else {
            return VarDictionary::new();
        };
        let [x, y, z] = chunk.center;
        let conifers = PackedFloat32Array::from(chunk.trees.conifers.as_slice());
        let broadleaves = PackedFloat32Array::from(chunk.trees.broadleaves.as_slice());
        vdict! {
            "center" => Vector3::new(x, y, z),
            "terrain" => &mesh_arrays(&chunk.mesh),
            "buildings" => &mesh_arrays(&chunk.buildings),
            "conifers" => &conifers,
            "broadleaves" => &broadleaves,
        }
    }

    /// Bridges and tunnels as mesh arrays (vertex-coloured), in route coordinates.
    #[func]
    fn structures_mesh(&self) -> VarDictionary {
        self.app
            .as_ref()
            .and_then(App::world)
            .map_or_else(VarDictionary::new, |world| mesh_arrays(&world.structures))
    }

    /// The minimap as coloured triangles: `{vertices, colors, background}`, vertices in metres
    /// east/north of the route start (as in `track()`).
    #[func]
    fn minimap_mesh(&self) -> VarDictionary {
        let Some(world) = self.app.as_ref().and_then(App::world) else {
            return VarDictionary::new();
        };
        let vertices: PackedVector2Array = world
            .minimap
            .vertices
            .iter()
            .map(|&[x, y]| Vector2::new(x, y))
            .collect();
        let colors: PackedColorArray = world
            .minimap
            .colors
            .iter()
            .map(|&[r, g, b, a]| Color::from_rgba(r, g, b, a))
            .collect();
        let [r, g, b, a] = torqa_world::MINIMAP_BACKGROUND;
        vdict! {
            "vertices" => &vertices,
            "colors" => &colors,
            "background" => Color::from_rgba(r, g, b, a),
        }
    }

    /// Rivers and streams as mesh arrays, in route coordinates.
    #[func]
    fn water_mesh(&self) -> VarDictionary {
        self.app
            .as_ref()
            .and_then(App::world)
            .map_or_else(VarDictionary::new, |world| mesh_arrays(&world.water))
    }

    /// The road as mesh arrays `{vertices, normals, uvs, indices}`; `uv.y` is the distance
    /// along the route in metres.
    #[func]
    fn road_mesh(&self) -> VarDictionary {
        self.app
            .as_ref()
            .and_then(App::world)
            .map_or_else(VarDictionary::new, |world| mesh_arrays(&world.road))
    }

    /// `(distance m, elevation m)` points of the loaded route, at most `max_points`.
    #[func]
    fn elevation_profile(&self, max_points: i64) -> PackedVector2Array {
        self.points(max_points, view::elevation_profile)
    }

    /// The loaded route in metres east/north of its start, at most `max_points`.
    #[func]
    fn track(&self, max_points: i64) -> PackedVector2Array {
        self.points(max_points, view::track)
    }
}

impl TorqaApp {
    fn command(&mut self, run: impl FnOnce(&mut App) -> Result<(), torqa_app::AppError>) -> bool {
        let Some(app) = self.app.as_mut() else {
            return false;
        };
        match run(app) {
            Ok(()) => true,
            Err(error) => {
                let message = error.to_string();
                self.signals()
                    .failed()
                    .emit(&GString::from(message.as_str()));
                false
            }
        }
    }

    fn points(
        &self,
        max_points: i64,
        pick: fn(&torqa_routes::Route, usize) -> Vec<(f64, f64)>,
    ) -> PackedVector2Array {
        let Some(route) = self.app.as_ref().and_then(App::route) else {
            return PackedVector2Array::new();
        };
        let max_points = usize::try_from(max_points).unwrap_or(2);
        pick(route, max_points)
            .into_iter()
            .map(|(a, b)| vector2(a, b))
            .collect()
    }

    fn emit(&mut self, event: AppEvent) {
        match event {
            AppEvent::DevicesFound(devices) => {
                self.signals().devices_found().emit(&device_array(devices));
            }
            AppEvent::RouteLoaded(route) => {
                let source = match route.elevation_source {
                    ElevationSource::Terrain => "terrain",
                    ElevationSource::File => "file",
                };
                let info = vdict! {
                    "name" => route.name.as_str(),
                    "length_m" => route.length,
                    "elevation_gain_m" => route.elevation_gain,
                    "max_grade" => route.max_grade,
                    "elevation_source" => source,
                };
                self.signals().route_loaded().emit(&info);
            }
            AppEvent::LoadProgress { stage, done, total } => {
                self.signals().loading_progress().emit(
                    &GString::from(stage.label()),
                    &GString::from(stage.unit()),
                    i64::try_from(done).unwrap_or(i64::MAX),
                    i64::try_from(total).unwrap_or(i64::MAX),
                );
            }
            AppEvent::WorldReady {
                chunks,
                fallback_samples,
            } => {
                let info = vdict! {
                    "chunks" => i64::try_from(chunks).unwrap_or(0),
                    "fallback_samples" => i64::try_from(fallback_samples).unwrap_or(0),
                };
                self.signals().world_ready().emit(&info);
            }
            AppEvent::Connected(name) => {
                self.signals()
                    .device_connected()
                    .emit(&GString::from(name.as_str()));
            }
            AppEvent::Disconnected(name) => {
                self.signals()
                    .device_disconnected()
                    .emit(&GString::from(name.as_str()));
            }
            AppEvent::RideFinished => self.signals().ride_finished().emit(),
            AppEvent::RideSaved(path) => {
                let path = path.display().to_string();
                self.signals()
                    .ride_saved()
                    .emit(&GString::from(path.as_str()));
            }
            AppEvent::CourseAdded(path) => {
                let path = path.display().to_string();
                self.signals()
                    .course_added()
                    .emit(&GString::from(path.as_str()));
            }
            AppEvent::ClimbCompleted {
                index,
                elapsed,
                previous_best,
            } => {
                self.signals().climb_completed().emit(
                    i64::try_from(index).unwrap_or(0),
                    elapsed.as_secs_f64(),
                    previous_best.map_or(-1.0, |b| b.as_secs_f64()),
                );
            }
            AppEvent::RouteCompleted {
                elapsed,
                previous_best,
            } => {
                self.signals().route_completed().emit(
                    elapsed.as_secs_f64(),
                    previous_best.map_or(-1.0, |b| b.as_secs_f64()),
                );
            }
            AppEvent::RememberedMissing(names) => {
                let names: PackedStringArray =
                    names.iter().map(|n| GString::from(n.as_str())).collect();
                self.signals().remembered_missing().emit(&names);
            }
            AppEvent::Error(message) => {
                self.signals()
                    .failed()
                    .emit(&GString::from(message.as_str()));
            }
        }
    }
}

/// Live HUD values as `{id: value or null}`.
fn hud_values(app: &App) -> VarDictionary {
    let mut values = VarDictionary::new();
    for (id, value) in app.hud_values() {
        values.set(id, &value.map_or_else(Variant::nil, |v| v.to_variant()));
    }
    values
}

/// Preview points as Godot vectors.
fn points(points: &[[f32; 2]]) -> PackedVector2Array {
    points.iter().map(|&[x, y]| Vector2::new(x, y)).collect()
}

/// Scanned devices as `[{index, name, kind, rssi}]` for `devices_found`.
fn device_array(devices: Vec<torqa_app::DeviceInfo>) -> VarArray {
    let mut array = VarArray::new();
    for device in devices {
        let kind = match device.kind {
            DeviceKind::Trainer => "trainer",
            DeviceKind::HeartRateSensor => "heart_rate",
        };
        let rssi = device
            .rssi
            .map_or_else(Variant::nil, |rssi| i64::from(rssi).to_variant());
        let index = i64::try_from(device.index).unwrap_or(-1);
        array.push(
            &vdict! {
                "index" => index,
                "name" => device.name.as_str(),
                "kind" => kind,
                "rssi" => &rssi,
                "remembered" => device.remembered,
            }
            .to_variant(),
        );
    }
    array
}

/// A history entry's climb times: `[{start_m, end_m, length_m, time_s, avg_power, record}]`.
fn climb_times(entry: &torqa_app::HistoryEntry) -> VarArray {
    let mut array = VarArray::new();
    for (climb, record) in entry.record.climbs.iter().zip(&entry.climb_records) {
        let power = climb
            .avg_power
            .map_or_else(Variant::nil, |p| p.0.to_variant());
        array.push(
            &vdict! {
                "start_m" => climb.start.0,
                "end_m" => climb.end.0,
                "length_m" => climb.end.0 - climb.start.0,
                "time_s" => climb.elapsed.as_secs_f64(),
                "avg_power" => &power,
                "record" => *record,
            }
            .to_variant(),
        );
    }
    array
}

/// Converts mesh data to the arrays Godot's `ArrayMesh` takes; `colors` is empty when the
/// mesh has none.
fn mesh_arrays(mesh: &torqa_world::MeshData) -> VarDictionary {
    let vertices: PackedVector3Array = mesh
        .vertices
        .iter()
        .map(|&[x, y, z]| Vector3::new(x, y, z))
        .collect();
    let normals: PackedVector3Array = mesh
        .normals
        .iter()
        .map(|&[x, y, z]| Vector3::new(x, y, z))
        .collect();
    let uvs: PackedVector2Array = mesh.uvs.iter().map(|&[u, v]| Vector2::new(u, v)).collect();
    let colors: PackedColorArray = mesh
        .colors
        .iter()
        .map(|&[r, g, b, a]| Color::from_rgba(r, g, b, a))
        .collect();
    let indices: PackedInt32Array = mesh
        .indices
        .iter()
        .map(|&i| i32::try_from(i).unwrap_or(0))
        .collect();
    vdict! {
        "vertices" => &vertices,
        "normals" => &normals,
        "uvs" => &uvs,
        "colors" => &colors,
        "indices" => &indices,
    }
}

// Godot vectors are f32; metre precision is ample for drawing.
#[allow(clippy::cast_possible_truncation)]
fn vector2(x: f64, y: f64) -> Vector2 {
    Vector2::new(x as f32, y as f32)
}

/// A decoded video frame as a Godot RGBA8 image.
fn image(frame: torqa_app::Frame) -> Option<Gd<Image>> {
    let width = i32::try_from(frame.width).ok()?;
    let height = i32::try_from(frame.height).ok()?;
    let data = PackedByteArray::from(frame.rgba);
    Image::create_from_data(width, height, false, ImageFormat::RGBA8, &data)
}

/// Seconds from Godot as a duration; negative or invalid values count as zero.
fn seconds(value: f64) -> Duration {
    Duration::try_from_secs_f64(value.max(0.0)).unwrap_or_default()
}

/// Sync marks from Godot: x metres along the route, y seconds into the video.
fn sync_marks(marks: &PackedVector2Array) -> Vec<torqa_app::SyncMark> {
    marks
        .as_slice()
        .iter()
        .map(|m| torqa_app::SyncMark {
            distance: Meters(f64::from(m.x)),
            time: seconds(f64::from(m.y)),
        })
        .collect()
}
