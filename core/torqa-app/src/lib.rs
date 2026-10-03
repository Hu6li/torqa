//! The Torqa application layer: devices, routes, the ride engine and storage behind a simple,
//! frame-driven API. Front ends (the Godot app, tests) call commands and [`App::update`] once per
//! frame; all asynchronous work runs on an internal runtime, so callers never block or await.

pub mod hud;
mod import;
pub mod media;
pub mod paths;
pub mod view;

pub use import::{Imported, LoadStage, Progress, import_gpx, import_route};

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, SystemTime};

use torqa_devices::ble::{Bluetooth, DeviceKind, DiscoveredDevice};
use torqa_devices::fake::{self, FakeRider};
use torqa_devices::{DeviceEvent, DeviceHandle};
use torqa_domain::files::UsedFiles;
use torqa_domain::profile::Profile;
use torqa_domain::recording::{RideSummary, Sample};
use torqa_domain::units::{Meters, Percent, Watts};
use torqa_physics::{DescentMode, RiderSetup};
use torqa_routes::{Climb, ElevationSource, Route};
use torqa_session::analysis::{
    effort, summarize, time_at, time_in_heart_rate_zones, time_in_power_zones,
};
use torqa_session::ghost::Ghost;
use torqa_session::{Ride, RideConfig, RideState};
use torqa_storage::course::{self, Manifest};
use torqa_storage::profiles::{self, StoredProfile};
use torqa_storage::rides::{self, ClimbTime, RideRecord};
use torqa_terrain::{Terrain, TileSource};
use torqa_world::World;
use tracing::warn;

/// Credits for the data a course bundles, stored in course files (ODbL, CC BY).
const ATTRIBUTION: [&str; 3] = [
    "© OpenFreeMap © OpenMapTiles · Data © OpenStreetMap contributors (ODbL)",
    "Terrain: Mapterhorn (CC BY 4.0)",
    "Terrain: AWS Terrain Tiles",
];

/// Points of the thinned track and profile stored with a course for its card.
const PREVIEW_POINTS: usize = 200;

/// How long the reconnect at start scans for the devices used last.
const RECONNECT_SCAN: Duration = Duration::from_secs(6);

/// How long [`App::shutdown`] waits for devices to disconnect.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

/// Errors from commands that fail immediately.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The async runtime could not be started.
    #[error("cannot start runtime: {0}")]
    Runtime(#[from] std::io::Error),
    /// A ride needs a loaded route.
    #[error("no route loaded")]
    NoRoute,
    /// A ride needs a trainer.
    #[error("no trainer connected")]
    NoTrainer,
    /// The device index does not refer to a discovered device of the right kind.
    #[error("unknown device")]
    UnknownDevice,
    /// Saving a course needs a route whose world has been built.
    #[error("the course is not ready yet")]
    CourseNotReady,
    /// No profile with that id.
    #[error("unknown profile")]
    UnknownProfile,
    /// Reading or writing a file in the data directory failed.
    #[error("{0}")]
    Storage(String),
    /// The chosen ghost cannot ride this route.
    #[error("{0}")]
    GhostUnavailable(String),
}

/// Who to race against (R20).
#[derive(Debug, Clone, PartialEq)]
pub enum GhostChoice {
    /// Nobody.
    None,
    /// The rider's fastest earlier ride on this route.
    PersonalBest,
    /// A pacer holding constant power.
    Power(Watts),
    /// A pacer holding constant power per kilogram of the rider's body weight.
    WattsPerKg(f64),
    /// A recorded activity (GPX with times, or FIT) along this route.
    Activity(PathBuf),
}

/// Where the ghost is relative to the rider.
#[derive(Debug, Clone, PartialEq)]
pub struct GhostState {
    /// What the ghost is.
    pub name: String,
    /// Its distance from the route start.
    pub distance: Meters,
    /// Seconds the rider is behind it (negative: ahead); `None` once it is out of reach.
    pub gap: Option<f64>,
}

/// A device found by a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Index to pass to the connect commands.
    pub index: usize,
    /// Advertised name.
    pub name: String,
    /// Whether it is a trainer or a heart-rate sensor.
    pub kind: DeviceKind,
    /// Signal strength in dBm, if known.
    pub rssi: Option<i16>,
    /// Whether this is the trainer or sensor used last (R41).
    pub remembered: bool,
}

/// Key facts about a loaded route.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteSummary {
    /// Name from the file, or the file name.
    pub name: String,
    /// Length in metres.
    pub length: f64,
    /// Total climbing in metres.
    pub elevation_gain: f64,
    /// Steepest climbing gradient in percent.
    pub max_grade: f64,
    /// Where the elevations come from.
    pub elevation_source: ElevationSource,
}

/// A course in the library.
#[derive(Debug, Clone, PartialEq)]
pub struct CourseEntry {
    /// The course file.
    pub path: PathBuf,
    /// What the course file says about itself.
    pub manifest: Manifest,
}

/// A ride in the history.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    /// The FIT activity file.
    pub fit: PathBuf,
    /// Route and summary.
    pub record: RideRecord,
    /// Whether this is the rider's fastest time over the whole route.
    pub route_record: bool,
    /// Per entry of [`RideRecord::climbs`]: whether it is the rider's fastest time there.
    pub climb_records: Vec<bool>,
}

/// The rider's best times on a route (R27).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RouteRecords {
    /// Fastest time over the whole route.
    pub route: Option<Duration>,
    /// Fastest time per climb, in the order of [`Route::climbs`].
    pub climbs: Vec<Option<Duration>>,
}

/// The climb the rider is on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClimbProgress {
    /// Index in [`Route::climbs`].
    pub index: usize,
    /// The climb.
    pub climb: Climb,
    /// Distance ridden on it so far.
    pub ridden: Meters,
    /// Time on it so far.
    pub elapsed: Duration,
    /// The rider's best time on it.
    pub best: Option<Duration>,
}

/// Everything recorded during one ride, for its analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct RideDetail {
    /// The 1 Hz samples.
    pub samples: Vec<Sample>,
    /// Time in each power zone of the active rider.
    pub power_zones: [Duration; 7],
    /// Time in each heart-rate zone of the active rider.
    pub heart_rate_zones: [Duration; 5],
}

/// Something that happened since the last [`App::update`].
#[derive(Debug, Clone, PartialEq)]
pub enum AppEvent {
    /// A scan finished.
    DevicesFound(Vec<DeviceInfo>),
    /// Preparing a course advanced.
    LoadProgress {
        /// The current step.
        stage: LoadStage,
        /// Units done in this step.
        done: usize,
        /// Units in this step.
        total: usize,
    },
    /// A route was imported; its 3D world is being generated.
    RouteLoaded(RouteSummary),
    /// The 3D world for the loaded route is ready.
    WorldReady {
        /// Number of terrain chunks.
        chunks: usize,
        /// Terrain samples without elevation data (terrain follows the road there).
        fallback_samples: usize,
    },
    /// A device connected (also after a reconnect).
    Connected(String),
    /// Devices used last that a reconnect at start did not find (R41), by name: the rider
    /// should wake them and scan.
    RememberedMissing(Vec<String>),
    /// A device lost its connection; it reconnects automatically.
    Disconnected(String),
    /// The rider reached the finish.
    RideFinished,
    /// The ride was saved as a FIT file.
    RideSaved(PathBuf),
    /// A course was saved or imported into the library.
    CourseAdded(PathBuf),
    /// The rider reached the top of a climb.
    ClimbCompleted {
        /// Index in [`Route::climbs`].
        index: usize,
        /// Time from foot to top.
        elapsed: Duration,
        /// The best time before this ride, if any.
        previous_best: Option<Duration>,
    },
    /// The rider reached the finish.
    RouteCompleted {
        /// Time for the whole route.
        elapsed: Duration,
        /// The best time before this ride, if any.
        previous_best: Option<Duration>,
    },
    /// A background operation failed.
    Error(String),
}

/// Which trainer to connect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrainerChoice {
    /// The simulated trainer.
    Fake(FakeRider),
    /// A device from the last scan, by [`DeviceInfo::index`].
    Discovered(usize),
}

enum JobResult {
    Scan(Result<(Bluetooth, Vec<DiscoveredDevice>), String>),
    Route(Result<Box<Imported>, String>),
    World(Box<World>),
    Progress(LoadStage, usize, usize),
    CourseAdded(Result<PathBuf, String>),
    Failed(String),
}

/// Forwards load progress to the frame loop, at most once per percent per stage.
struct Reporter {
    tx: mpsc::Sender<JobResult>,
    last: Option<(LoadStage, usize)>,
}

impl Reporter {
    fn report(&mut self, stage: LoadStage, done: usize, total: usize) {
        let percent = done * 100 / total.max(1);
        if self.last != Some((stage, percent)) {
            self.last = Some((stage, percent));
            let _ = self.tx.send(JobResult::Progress(stage, done, total));
        }
    }
}

struct ActiveRide {
    ride: Ride,
    /// Wall-clock start, set when the trainer first connects.
    started: Option<SystemTime>,
    finished: bool,
    /// Best times before this ride, to compare against.
    records: RouteRecords,
    /// The next climb whose top has not been reached.
    next_climb: usize,
    /// Summary of the samples so far and how many it covers, refreshed once per new sample
    /// rather than every frame.
    summary: (usize, RideSummary),
    ghost: Option<Ghost>,
}

/// The application state.
pub struct App {
    runtime: tokio::runtime::Runtime,
    jobs_tx: mpsc::Sender<JobResult>,
    jobs_rx: mpsc::Receiver<JobResult>,
    bluetooth: Option<Bluetooth>,
    discovered: Vec<DiscoveredDevice>,
    trainer: Option<DeviceHandle>,
    sensor: Option<DeviceHandle>,
    /// Identifiers of the connected Bluetooth trainer and sensor, to avoid reconnecting them.
    trainer_id: Option<String>,
    sensor_id: Option<String>,
    /// The running scan was started to reconnect the remembered devices.
    reconnecting: bool,
    /// The loaded route came from a course file (and is in the library already).
    from_course: bool,
    route: Option<Route>,
    world: Option<Arc<World>>,
    offline: bool,
    /// Name and GPX of the loaded route, for saving it as a course.
    loaded: Option<(String, String)>,
    /// Cached files the loaded route and its world were built from.
    used: UsedFiles,
    ride: Option<ActiveRide>,
    profile: StoredProfile,
    data_dir: PathBuf,
    cache_dir: PathBuf,
}

impl App {
    /// Creates the application with rides saved under `data_dir` and downloads cached under
    /// `cache_dir` (see [`paths`] for platform defaults).
    ///
    /// # Errors
    /// [`AppError::Runtime`] if the async runtime cannot start.
    pub fn new(data_dir: PathBuf, cache_dir: PathBuf) -> Result<Self, AppError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let (jobs_tx, jobs_rx) = mpsc::channel();
        Ok(Self {
            runtime,
            jobs_tx,
            jobs_rx,
            bluetooth: None,
            discovered: Vec::new(),
            trainer: None,
            sensor: None,
            trainer_id: None,
            sensor_id: None,
            reconnecting: false,
            from_course: false,
            route: None,
            world: None,
            offline: false,
            loaded: None,
            used: UsedFiles::default(),
            ride: None,
            profile: initial_profile(&data_dir),
            data_dir,
            cache_dir,
        })
    }

    /// All rider profiles, by name.
    #[must_use]
    pub fn profiles(&self) -> Vec<StoredProfile> {
        let listed = profiles::list(&self.data_dir);
        if listed.is_empty() {
            // Not saved yet, e.g. a read-only data directory: still offer the rider in use.
            vec![self.profile.clone()]
        } else {
            listed
        }
    }

    /// The rider riding now.
    #[must_use]
    pub fn profile(&self) -> &StoredProfile {
        &self.profile
    }

    /// Switches to another rider, remembered for the next start.
    ///
    /// # Errors
    /// [`AppError::UnknownProfile`] if there is no such profile.
    pub fn select_profile(&mut self, id: &str) -> Result<(), AppError> {
        let profile = profiles::load(&self.data_dir, id).map_err(|_| AppError::UnknownProfile)?;
        self.profile = StoredProfile {
            id: id.to_owned(),
            profile,
        };
        if let Err(error) = profiles::set_active(&self.data_dir, id) {
            warn!(%error, "cannot remember the active profile");
        }
        Ok(())
    }

    /// Saves a profile (a new one if `id` is `None`) and makes it the active one; returns its id.
    ///
    /// # Errors
    /// [`AppError::Storage`] if it cannot be written.
    pub fn save_profile(&mut self, id: Option<&str>, profile: Profile) -> Result<String, AppError> {
        let id = id.map_or_else(
            || profiles::new_id(&self.data_dir, &profile.name),
            ToOwned::to_owned,
        );
        profiles::save(&self.data_dir, &id, &profile)
            .map_err(|e| AppError::Storage(format!("cannot save profile: {e}")))?;
        if let Err(error) = profiles::set_active(&self.data_dir, &id) {
            warn!(%error, "cannot remember the active profile");
        }
        self.profile = StoredProfile {
            id: id.clone(),
            profile,
        };
        Ok(id)
    }

    /// Scans for trainers and heart-rate sensors; reports [`AppEvent::DevicesFound`].
    pub fn scan(&mut self, duration: Duration) {
        let tx = self.jobs_tx.clone();
        let existing = self.bluetooth.clone();
        self.runtime.spawn(async move {
            let result = async {
                let bluetooth = match existing {
                    Some(bluetooth) => bluetooth,
                    None => Bluetooth::new().await?,
                };
                let devices = bluetooth.scan(duration).await?;
                Ok((bluetooth, devices))
            }
            .await
            .map_err(|e: torqa_devices::DeviceError| e.to_string());
            let _ = tx.send(JobResult::Scan(result));
        });
    }

    /// Imports a GPX route with terrain-corrected elevations; reports [`AppEvent::RouteLoaded`].
    pub fn load_route(&mut self, path: PathBuf, offline: bool) {
        let used = self.start_loading(offline);
        let tx = self.jobs_tx.clone();
        let cache = self.cache_dir.clone();
        self.runtime.spawn(async move {
            let mut reporter = Reporter {
                tx: tx.clone(),
                last: None,
            };
            let result = import_route(&path, &cache, offline, &used, &mut |stage, done, total| {
                reporter.report(stage, done, total);
            })
            .await
            .map(Box::new);
            let _ = tx.send(JobResult::Route(result));
        });
    }

    /// Opens a course file: its data goes back into the cache and the course is built offline;
    /// reports [`AppEvent::RouteLoaded`] like [`App::load_route`].
    pub fn open_course(&mut self, path: PathBuf) {
        let used = self.start_loading(true);
        self.from_course = true;
        let tx = self.jobs_tx.clone();
        let cache = self.cache_dir.clone();
        self.runtime.spawn(async move {
            let mut reporter = Reporter {
                tx: tx.clone(),
                last: None,
            };
            reporter.report(LoadStage::Route, 0, 1);
            let unpack_from = path.clone();
            let unpack_to = cache.clone();
            let unpacked =
                tokio::task::spawn_blocking(move || course::unpack(&unpack_from, &unpack_to))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e| format!("cannot open {}: {e}", path.display())));
            let result = match unpacked {
                Ok(unpacked) => import_gpx(
                    unpacked.gpx,
                    &unpacked.manifest.name,
                    &cache,
                    true,
                    &used,
                    &mut |stage, done, total| reporter.report(stage, done, total),
                )
                .await
                .map(|mut imported| {
                    imported.name = unpacked.manifest.name;
                    Box::new(imported)
                }),
                Err(message) => Err(message),
            };
            let _ = tx.send(JobResult::Route(result));
        });
    }

    /// Saves the loaded route with everything needed to ride it offline as a course in the
    /// library; reports [`AppEvent::CourseAdded`].
    ///
    /// # Errors
    /// [`AppError::CourseNotReady`] until the route's world has been built.
    pub fn save_course(&mut self) -> Result<(), AppError> {
        let (Some(route), Some(_), Some((name, gpx))) = (&self.route, &self.world, &self.loaded)
        else {
            return Err(AppError::CourseNotReady);
        };
        let manifest = Manifest {
            format: course::FORMAT_VERSION,
            generator: format!("Torqa {}", torqa_domain::version()),
            name: name.clone(),
            length_m: route.length().0,
            elevation_gain_m: route.elevation_gain().0,
            max_grade_percent: route.max_grade().0,
            created_unix_s: SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            attribution: ATTRIBUTION.map(ToOwned::to_owned).to_vec(),
            route_key: Some(route.key()),
            track: thin(&view::track(route, PREVIEW_POINTS)),
            profile: thin(&view::elevation_profile(route, PREVIEW_POINTS)),
        };
        // Preparing the same course again must not fill the library with copies.
        if let Some(existing) = self
            .courses()
            .into_iter()
            .find(|c| c.manifest.route_key == manifest.route_key)
        {
            let _ = self.jobs_tx.send(JobResult::CourseAdded(Ok(existing.path)));
            return Ok(());
        }
        let gpx = gpx.clone();
        let data = self.used.paths();
        let cache = self.cache_dir.clone();
        let library = self.courses_dir();
        let tx = self.jobs_tx.clone();
        self.runtime.spawn_blocking(move || {
            let result = std::fs::create_dir_all(&library)
                .map_err(course::CourseError::from)
                .and_then(|()| {
                    let path = unique_course_path(&library, &manifest.name);
                    course::write(&path, &manifest, &gpx, &cache, &data).map(|()| path)
                })
                .map_err(|e| format!("cannot save course: {e}"));
            let _ = tx.send(JobResult::CourseAdded(result));
        });
        Ok(())
    }

    /// Copies a course file into the library; reports [`AppEvent::CourseAdded`].
    pub fn import_course(&mut self, path: PathBuf) {
        let library = self.courses_dir();
        let tx = self.jobs_tx.clone();
        self.runtime.spawn_blocking(move || {
            let result = course::read_manifest(&path)
                .and_then(|manifest| {
                    std::fs::create_dir_all(&library)?;
                    let target = unique_course_path(&library, &manifest.name);
                    let partial = target.with_extension("part");
                    std::fs::copy(&path, &partial)?;
                    std::fs::rename(&partial, &target)?;
                    Ok(target)
                })
                .map_err(|e| format!("cannot import {}: {e}", path.display()));
            let _ = tx.send(JobResult::CourseAdded(result));
        });
    }

    /// The courses in the library, by name. Unreadable files are skipped.
    #[must_use]
    pub fn courses(&self) -> Vec<CourseEntry> {
        let Ok(entries) = std::fs::read_dir(self.courses_dir()) else {
            return Vec::new();
        };
        let mut courses: Vec<CourseEntry> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == course::EXTENSION))
            .filter_map(|path| match course::read_manifest(&path) {
                Ok(manifest) => Some(CourseEntry { path, manifest }),
                Err(error) => {
                    warn!(path = %path.display(), %error, "skipping course");
                    None
                }
            })
            .collect();
        courses.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        courses
    }

    /// The course library: `courses/` in the data directory (R34).
    #[must_use]
    pub fn courses_dir(&self) -> PathBuf {
        self.data_dir.join("courses")
    }

    /// Forgets the current route and starts recording the files a new one uses.
    fn start_loading(&mut self, offline: bool) -> UsedFiles {
        self.offline = offline;
        // Opening a course sets this; a GPX import goes into the library when ready.
        self.from_course = false;
        self.world = None;
        self.route = None;
        self.loaded = None;
        self.used = UsedFiles::default();
        self.used.clone()
    }

    /// Generates the 3D world for `route` in the background; reports [`AppEvent::WorldReady`].
    fn generate_world(&mut self, route: Route, map: torqa_osm::MapData) {
        let tx = self.jobs_tx.clone();
        let mut terrain = Terrain::new(TileSource::defaults(), self.cache_dir.join("terrain"))
            .recording(self.used.clone());
        if self.offline {
            terrain = terrain.offline();
        }
        self.runtime.spawn(async move {
            let mut reporter = Reporter {
                tx: tx.clone(),
                last: None,
            };
            let world = torqa_world::generate(&route, &mut terrain, &map, &mut |done, total| {
                reporter.report(LoadStage::World, done, total);
            })
            .await;
            let _ = tx.send(JobResult::World(Box::new(world)));
        });
    }

    /// Connects the trainer, replacing any previous one.
    ///
    /// # Errors
    /// [`AppError::UnknownDevice`] if the index is not a trainer from the last scan.
    pub fn connect_trainer(&mut self, choice: TrainerChoice) -> Result<(), AppError> {
        let _runtime = self.runtime.enter();
        let handle = match choice {
            TrainerChoice::Fake(rider) => {
                self.trainer_id = None;
                fake::spawn(rider, Duration::from_millis(250))
            }
            TrainerChoice::Discovered(index) => {
                let device = self.discovered_device(index, DeviceKind::Trainer)?;
                let id = device.id();
                // Already connected (e.g. reconnected at start): keep the link.
                if self.trainer.is_some() && self.trainer_id.as_deref() == Some(id.as_str()) {
                    return Ok(());
                }
                self.remember(true, &device);
                self.trainer_id = Some(id);
                self.bluetooth()?.connect(device)
            }
        };
        self.trainer = Some(handle);
        Ok(())
    }

    /// Reconnects the trainer and heart-rate sensor used last (R41): scans in the background
    /// and connects them when found, reporting [`AppEvent::DevicesFound`] (with the remembered
    /// devices marked) and [`AppEvent::RememberedMissing`] for those not found. Returns false,
    /// without touching Bluetooth, if no device is remembered.
    pub fn reconnect_remembered(&mut self) -> bool {
        let remembered = profiles::remembered_devices(&self.data_dir);
        if remembered.trainer.is_none() && remembered.heart_rate.is_none() {
            return false;
        }
        self.reconnecting = true;
        self.scan(RECONNECT_SCAN);
        true
    }

    fn remember(&self, trainer: bool, device: &DiscoveredDevice) {
        let remembered = profiles::RememberedDevice {
            id: device.id(),
            name: device.name.clone(),
        };
        if let Err(error) = profiles::remember_device(&self.data_dir, trainer, remembered) {
            warn!(%error, "cannot remember the device");
        }
    }

    /// Connects the remembered devices found by a reconnect scan; returns the names of those
    /// not found.
    fn connect_remembered(&mut self) -> Vec<String> {
        let remembered = profiles::remembered_devices(&self.data_dir);
        let mut missing = Vec::new();
        for (wanted, kind) in [
            (remembered.trainer, DeviceKind::Trainer),
            (remembered.heart_rate, DeviceKind::HeartRateSensor),
        ] {
            let Some(wanted) = wanted else { continue };
            let found = self
                .discovered
                .iter()
                .position(|d| d.kind == kind && d.id() == wanted.id)
                .or_else(|| {
                    self.discovered
                        .iter()
                        .position(|d| d.kind == kind && d.name == wanted.name)
                });
            let connected = match (found, kind) {
                (Some(index), DeviceKind::Trainer) => {
                    self.connect_trainer(TrainerChoice::Discovered(index))
                }
                (Some(index), DeviceKind::HeartRateSensor) => self.connect_heart_rate(index),
                (None, _) => Err(AppError::UnknownDevice),
            };
            if connected.is_err() {
                missing.push(wanted.name);
            }
        }
        missing
    }

    /// Whether a scanned device is one of the remembered ones.
    fn is_remembered(&self, device: &DiscoveredDevice) -> bool {
        let remembered = profiles::remembered_devices(&self.data_dir);
        let wanted = match device.kind {
            DeviceKind::Trainer => remembered.trainer,
            DeviceKind::HeartRateSensor => remembered.heart_rate,
        };
        wanted.is_some_and(|w| w.id == device.id() || w.name == device.name)
    }

    /// Connects a heart-rate sensor, replacing any previous one.
    ///
    /// # Errors
    /// [`AppError::UnknownDevice`] if the index is not a heart-rate sensor from the last scan.
    pub fn connect_heart_rate(&mut self, index: usize) -> Result<(), AppError> {
        let device = self.discovered_device(index, DeviceKind::HeartRateSensor)?;
        let id = device.id();
        if self.sensor.is_some() && self.sensor_id.as_deref() == Some(id.as_str()) {
            return Ok(());
        }
        let _runtime = self.runtime.enter();
        self.remember(false, &device);
        self.sensor_id = Some(id);
        self.sensor = Some(self.bluetooth()?.connect(device));
        Ok(())
    }

    /// Starts riding the loaded route as the active rider, whose profile sets the mass. The
    /// clock starts once the trainer is connected.
    ///
    /// # Errors
    /// [`AppError::NoRoute`] or [`AppError::NoTrainer`] if either is missing.
    pub fn start_ride(
        &mut self,
        difficulty: Percent,
        descent: DescentMode,
        ghost: &GhostChoice,
    ) -> Result<(), AppError> {
        let route = self.route.clone().ok_or(AppError::NoRoute)?;
        if self.trainer.is_none() {
            return Err(AppError::NoTrainer);
        }
        let ghost = self.ghost_for(&route, descent, ghost)?;
        let config = RideConfig {
            setup: RiderSetup {
                mass: self.profile.profile.system_mass(),
                ..RiderSetup::default()
            },
            difficulty,
            descent,
        };
        let records = self.records_for(&route);
        self.ride = Some(ActiveRide {
            ride: Ride::new(route, config),
            started: None,
            finished: false,
            records,
            next_climb: 0,
            summary: (0, RideSummary::default()),
            ghost,
        });
        Ok(())
    }

    /// Changes trainer difficulty and descent mode of the current ride (R48).
    pub fn adjust_ride(&mut self, difficulty: Percent, descent: DescentMode) {
        if let Some(active) = &mut self.ride {
            active.ride.adjust(difficulty, descent);
        }
    }

    /// Ends the ride without saving anything (R49).
    pub fn abort_ride(&mut self) {
        self.ride = None;
    }

    /// Ends the ride and saves it; reports [`AppEvent::RideSaved`] on the next update.
    pub fn finish_ride(&mut self) -> Vec<AppEvent> {
        let Some(active) = self.ride.take() else {
            return Vec::new();
        };
        let Some(start) = active.started else {
            return Vec::new();
        };
        if active.ride.samples().is_empty() {
            return Vec::new();
        }
        let path = profiles::rides_dir(&self.data_dir, &self.profile.id)
            .join(paths::activity_file_name(start));
        let samples = active.ride.samples();
        let saved = torqa_storage::encode_fit(start, samples)
            .map_err(|e| e.to_string())
            .and_then(|fit| {
                std::fs::create_dir_all(path.parent().unwrap_or(&self.data_dir))
                    .and_then(|()| std::fs::write(&path, fit))
                    .map_err(|e| format!("cannot save {}: {e}", path.display()))
            });
        if saved.is_ok() {
            let route = active.ride.route();
            let finished = samples
                .last()
                .is_some_and(|s| s.distance.0 >= route.length().0 - 1.0);
            let record = RideRecord {
                route: self
                    .loaded
                    .as_ref()
                    .map_or_else(|| "Ride".to_owned(), |(name, _)| name.clone()),
                start,
                summary: summarize(samples, self.profile.profile.ftp),
                route_key: Some(route.key()),
                route_time: finished
                    .then(|| effort(samples, Meters(0.0), route.length()))
                    .flatten()
                    .map(|e| e.elapsed),
                climbs: route
                    .climbs()
                    .iter()
                    .filter_map(|c| {
                        effort(samples, c.start, c.end).map(|e| ClimbTime {
                            start: c.start,
                            end: c.end,
                            elapsed: e.elapsed,
                            avg_power: e.avg_power,
                        })
                    })
                    .collect(),
                name: None,
            };
            // The FIT file is what counts; the history rebuilds missing metadata from it.
            if let Err(error) = rides::save(&path, &record) {
                warn!(%error, "cannot save ride metadata");
            }
        }
        vec![match saved {
            Ok(()) => AppEvent::RideSaved(path),
            Err(message) => AppEvent::Error(message),
        }]
    }

    /// The active rider's rides, newest first (R31). Rides without metadata, e.g. FIT files
    /// copied in by hand, are analysed once and get it written.
    #[must_use]
    pub fn history(&self) -> Vec<HistoryEntry> {
        let records: Vec<(PathBuf, RideRecord)> = rides::fit_files(&self.rides_dir())
            .into_iter()
            .filter_map(|fit| {
                let record = rides::load(&fit).or_else(|_| self.rebuild_metadata(&fit));
                match record {
                    Ok(record) => Some((fit, record)),
                    Err(error) => {
                        warn!(path = %fit.display(), %error, "skipping ride");
                        None
                    }
                }
            })
            .collect();
        let all: Vec<&RideRecord> = records.iter().map(|(_, r)| r).collect();
        records
            .iter()
            .map(|(fit, record)| {
                let same_route: Vec<&RideRecord> = all
                    .iter()
                    .copied()
                    .filter(|other| {
                        other.route_key.is_some() && other.route_key == record.route_key
                    })
                    .collect();
                let best_route = same_route.iter().filter_map(|r| r.route_time).min();
                HistoryEntry {
                    fit: fit.clone(),
                    route_record: record.route_time.is_some() && record.route_time == best_route,
                    climb_records: record
                        .climbs
                        .iter()
                        .map(|climb| Some(climb.elapsed) == best_climb_time(&same_route, climb))
                        .collect(),
                    record: record.clone(),
                }
            })
            .collect()
    }

    /// The active rider's best times on `route`, from earlier rides on the same course.
    #[must_use]
    pub fn records_for(&self, route: &Route) -> RouteRecords {
        let key = route.key();
        let history = self.history();
        let same_route: Vec<&RideRecord> = history
            .iter()
            .map(|entry| &entry.record)
            .filter(|record| record.route_key.as_deref() == Some(key.as_str()))
            .collect();
        RouteRecords {
            route: same_route.iter().filter_map(|r| r.route_time).min(),
            climbs: route
                .climbs()
                .iter()
                .map(|c| {
                    let probe = ClimbTime {
                        start: c.start,
                        end: c.end,
                        elapsed: Duration::ZERO,
                        avg_power: None,
                    };
                    best_climb_time(&same_route, &probe)
                })
                .collect(),
        }
    }

    /// The active rider's HUD metrics, in order (R23); the first is shown large.
    #[must_use]
    pub fn hud_layout(&self) -> Vec<String> {
        hud::sanitize(&profiles::load_hud(&self.data_dir, &self.profile.id).unwrap_or_default())
    }

    /// Saves the active rider's HUD metrics; unknown or repeated ones are dropped. Returns the
    /// layout as saved.
    ///
    /// # Errors
    /// [`AppError::Storage`] if it cannot be written.
    pub fn set_hud_layout(&mut self, layout: &[String]) -> Result<Vec<String>, AppError> {
        let layout = hud::sanitize(layout);
        profiles::save_hud(&self.data_dir, &self.profile.id, &layout)
            .map_err(|e| AppError::Storage(format!("cannot save HUD layout: {e}")))?;
        Ok(layout)
    }

    /// Live values of all HUD metrics while riding (see [`hud::values`]).
    #[must_use]
    pub fn hud_values(&self) -> Vec<(&'static str, Option<f64>)> {
        let Some(active) = &self.ride else {
            return Vec::new();
        };
        hud::values(
            &active.ride.state(),
            active.ride.samples(),
            &active.summary.1,
            active.ride.route(),
            &self.profile.profile,
        )
    }

    /// Tells the music app to play/pause or skip (R26), in the background; failures are
    /// reported as [`AppEvent::Error`].
    pub fn control_music(&mut self, command: media::MediaCommand) {
        let tx = self.jobs_tx.clone();
        self.runtime.spawn_blocking(move || {
            if let Err(message) = media::send(command) {
                let _ = tx.send(JobResult::Failed(message));
            }
        });
    }

    /// The ghost of the current ride, if any.
    #[must_use]
    pub fn ghost_state(&self) -> Option<GhostState> {
        let active = self.ride.as_ref()?;
        let ghost = active.ghost.as_ref()?;
        let state = active.ride.state();
        Some(GhostState {
            name: ghost.name.clone(),
            distance: ghost.distance_at(state.elapsed),
            gap: ghost.gap(state.distance, state.elapsed),
        })
    }

    fn ghost_for(
        &self,
        route: &Route,
        descent: DescentMode,
        choice: &GhostChoice,
    ) -> Result<Option<Ghost>, AppError> {
        let profile = &self.profile.profile;
        let setup = RiderSetup {
            mass: profile.system_mass(),
            ..RiderSetup::default()
        };
        let unavailable = |why: String| AppError::GhostUnavailable(why);
        let ghost = match choice {
            GhostChoice::None => return Ok(None),
            GhostChoice::PersonalBest => {
                let key = route.key();
                let best = self
                    .history()
                    .into_iter()
                    .filter(|h| h.record.route_key.as_deref() == Some(key.as_str()))
                    .filter_map(|h| h.record.route_time.map(|t| (t, h.fit)))
                    .min_by_key(|(t, _)| *t)
                    .ok_or_else(|| unavailable("no finished ride on this route yet".to_owned()))?;
                let (_, samples) = read_fit(&best.1).map_err(unavailable)?;
                Ghost::from_samples("Your best", &samples)
            }
            GhostChoice::Power(watts) => Some(Ghost::pacer(
                &format!("Pacer {:.0} W", watts.0),
                route,
                &setup,
                descent,
                *watts,
            )),
            GhostChoice::WattsPerKg(ratio) => Some(Ghost::pacer(
                &format!("Pacer {ratio:.1} W/kg"),
                route,
                &setup,
                descent,
                Watts(ratio * profile.rider_mass.0),
            )),
            GhostChoice::Activity(path) => {
                let points = activity_points(path).map_err(unavailable)?;
                let name = path
                    .file_stem()
                    .map_or_else(|| "Ghost".to_owned(), |s| s.to_string_lossy().into_owned());
                Some(Ghost::from_activity(&name, route, &points).ok_or_else(|| {
                    unavailable(format!("{} does not follow this route", path.display()))
                })?)
            }
        };
        Ok(ghost)
    }

    /// The climb the rider is on now, if any.
    #[must_use]
    pub fn current_climb(&self) -> Option<ClimbProgress> {
        let active = self.ride.as_ref()?;
        let state = active.ride.state();
        let (index, climb) = active
            .ride
            .route()
            .climbs()
            .iter()
            .enumerate()
            .find(|(_, c)| c.start.0 <= state.distance.0 && state.distance.0 < c.end.0)?;
        let started = time_at(active.ride.samples(), climb.start).unwrap_or(state.elapsed);
        Some(ClimbProgress {
            index,
            climb: *climb,
            ridden: Meters(state.distance.0 - climb.start.0),
            elapsed: state.elapsed.saturating_sub(started),
            best: active.records.climbs.get(index).copied().flatten(),
        })
    }

    /// The recorded data of one ride, with zones of the active rider.
    ///
    /// # Errors
    /// [`AppError::Storage`] if the FIT file cannot be read.
    pub fn ride_detail(&self, fit: &Path) -> Result<RideDetail, AppError> {
        let (_, samples) = read_fit(fit).map_err(AppError::Storage)?;
        let profile = &self.profile.profile;
        Ok(RideDetail {
            power_zones: time_in_power_zones(&samples, profile),
            heart_rate_zones: time_in_heart_rate_zones(&samples, profile),
            samples,
        })
    }

    /// Names a ride (R50); an empty name goes back to the route and date. Only the metadata
    /// changes, so file names stay stable for syncing.
    ///
    /// # Errors
    /// [`AppError::Storage`] if the metadata cannot be read or written.
    pub fn rename_ride(&self, fit: &Path, name: &str) -> Result<(), AppError> {
        let storage = |e: rides::RideError| AppError::Storage(format!("cannot rename ride: {e}"));
        let mut record = rides::load(fit)
            .or_else(|_| self.rebuild_metadata(fit).map_err(AppError::Storage))
            .map_err(|e| AppError::Storage(e.to_string()))?;
        let name = name.trim();
        record.name = (!name.is_empty()).then(|| name.to_owned());
        rides::save(fit, &record).map_err(storage)
    }

    /// Deletes a ride: its FIT file and metadata.
    ///
    /// # Errors
    /// [`AppError::Storage`] if the FIT file cannot be removed.
    pub fn delete_ride(&self, fit: &Path) -> Result<(), AppError> {
        std::fs::remove_file(fit)
            .map_err(|e| AppError::Storage(format!("cannot delete {}: {e}", fit.display())))?;
        let _ = std::fs::remove_file(rides::metadata_path(fit));
        Ok(())
    }

    fn rides_dir(&self) -> PathBuf {
        profiles::rides_dir(&self.data_dir, &self.profile.id)
    }

    fn rebuild_metadata(&self, fit: &Path) -> Result<RideRecord, String> {
        let (start, samples) = read_fit(fit)?;
        let record = RideRecord {
            route: fit
                .file_stem()
                .map_or_else(|| "Ride".to_owned(), |s| s.to_string_lossy().into_owned()),
            start,
            summary: summarize(&samples, self.profile.profile.ftp),
            // The route is unknown, so this ride counts towards no records.
            route_key: None,
            route_time: None,
            climbs: Vec::new(),
            name: None,
        };
        if let Err(error) = rides::save(fit, &record) {
            warn!(%error, "cannot save rebuilt ride metadata");
        }
        Ok(record)
    }

    /// Processes everything that happened since the last call and advances the ride by `dt`.
    pub fn update(&mut self, dt: Duration) -> Vec<AppEvent> {
        let mut events = Vec::new();
        self.poll_jobs(&mut events);
        self.poll_trainer(&mut events);
        self.poll_sensor(&mut events);

        if let Some(active) = &mut self.ride
            && active.started.is_some()
            && !active.finished
        {
            if let Some(control) = active.ride.tick(dt)
                && let Some(trainer) = &self.trainer
                && let Err(error) = trainer.try_control(control)
            {
                warn!(%error, "cannot control trainer");
            }
            let samples = active.ride.samples();
            if samples.len() != active.summary.0 {
                active.summary = (samples.len(), summarize(samples, self.profile.profile.ftp));
            }
            // Samples arrive once a second, so a climb is timed once a sample lies past its top.
            let climbs = active.ride.route().climbs();
            while let Some(climb) = climbs.get(active.next_climb)
                && let Some(done) = effort(active.ride.samples(), climb.start, climb.end)
            {
                events.push(AppEvent::ClimbCompleted {
                    index: active.next_climb,
                    elapsed: done.elapsed,
                    previous_best: active
                        .records
                        .climbs
                        .get(active.next_climb)
                        .copied()
                        .flatten(),
                });
                active.next_climb += 1;
            }
            if active.ride.is_finished() {
                active.finished = true;
                events.push(AppEvent::RouteCompleted {
                    elapsed: active.ride.state().elapsed,
                    previous_best: active.records.route,
                });
                events.push(AppEvent::RideFinished);
            }
        }
        events
    }

    /// The current ride's state, if riding.
    #[must_use]
    pub fn ride_state(&self) -> Option<RideState> {
        self.ride.as_ref().map(|active| active.ride.state())
    }

    /// The loaded route.
    #[must_use]
    pub fn route(&self) -> Option<&Route> {
        self.route.as_ref()
    }

    /// The 3D world of the loaded route, once generated.
    #[must_use]
    pub fn world(&self) -> Option<&World> {
        self.world.as_deref()
    }

    /// Disconnects all devices, waiting at most a few seconds.
    pub fn shutdown(&mut self) {
        let trainer = self.trainer.take();
        let sensor = self.sensor.take();
        self.runtime.block_on(async {
            if let Some(trainer) = trainer {
                trainer.close(CLOSE_TIMEOUT).await;
            }
            if let Some(sensor) = sensor {
                sensor.close(CLOSE_TIMEOUT).await;
            }
        });
    }

    fn bluetooth(&self) -> Result<&Bluetooth, AppError> {
        self.bluetooth.as_ref().ok_or(AppError::UnknownDevice)
    }

    fn discovered_device(
        &self,
        index: usize,
        kind: DeviceKind,
    ) -> Result<DiscoveredDevice, AppError> {
        self.discovered
            .get(index)
            .filter(|d| d.kind == kind)
            .cloned()
            .ok_or(AppError::UnknownDevice)
    }

    fn poll_jobs(&mut self, events: &mut Vec<AppEvent>) {
        while let Ok(result) = self.jobs_rx.try_recv() {
            match result {
                JobResult::Scan(Ok((bluetooth, devices))) => {
                    let infos = devices
                        .iter()
                        .enumerate()
                        .map(|(index, d)| DeviceInfo {
                            index,
                            name: d.name.clone(),
                            kind: d.kind,
                            rssi: d.rssi,
                            remembered: self.is_remembered(d),
                        })
                        .collect();
                    self.bluetooth = Some(bluetooth);
                    self.discovered = devices;
                    events.push(AppEvent::DevicesFound(infos));
                    if std::mem::take(&mut self.reconnecting) {
                        let missing = self.connect_remembered();
                        if !missing.is_empty() {
                            events.push(AppEvent::RememberedMissing(missing));
                        }
                    }
                }
                JobResult::Route(Ok(imported)) => {
                    let Imported {
                        route,
                        map,
                        name,
                        gpx,
                    } = *imported;
                    self.loaded = Some((name.clone(), gpx));
                    events.push(AppEvent::RouteLoaded(RouteSummary {
                        name,
                        length: route.length().0,
                        elevation_gain: route.elevation_gain().0,
                        max_grade: route.max_grade().0,
                        elevation_source: route.elevation_source(),
                    }));
                    self.generate_world(route.clone(), map);
                    self.route = Some(route);
                }
                JobResult::Progress(stage, done, total) => {
                    events.push(AppEvent::LoadProgress { stage, done, total });
                }
                JobResult::World(world) => {
                    events.push(AppEvent::WorldReady {
                        chunks: world.chunks.len(),
                        fallback_samples: world.fallback_samples,
                    });
                    self.world = Some(Arc::from(world));
                    // A prepared GPX goes into the course library (R39).
                    if !self.from_course
                        && let Err(error) = self.save_course()
                    {
                        warn!(%error, "cannot add the course to the library");
                    }
                }
                JobResult::CourseAdded(Ok(path)) => events.push(AppEvent::CourseAdded(path)),
                JobResult::Scan(Err(message)) if std::mem::take(&mut self.reconnecting) => {
                    // No Bluetooth (or no permission): the rider sees it when scanning.
                    warn!(%message, "cannot reconnect the devices used last");
                }
                JobResult::Scan(Err(message))
                | JobResult::Route(Err(message))
                | JobResult::CourseAdded(Err(message))
                | JobResult::Failed(message) => {
                    events.push(AppEvent::Error(message));
                }
            }
        }
    }

    fn poll_trainer(&mut self, events: &mut Vec<AppEvent>) {
        let Some(trainer) = &mut self.trainer else {
            return;
        };
        loop {
            match trainer.try_next_event() {
                Ok(Some(DeviceEvent::Connected)) => {
                    if let Some(active) = &mut self.ride
                        && active.started.is_none()
                    {
                        active.started = Some(SystemTime::now());
                    }
                    events.push(AppEvent::Connected(trainer.name().to_owned()));
                }
                Ok(Some(DeviceEvent::Disconnected)) => {
                    if let Some(active) = &mut self.ride {
                        active.ride.on_power_source_lost();
                    }
                    events.push(AppEvent::Disconnected(trainer.name().to_owned()));
                }
                Ok(Some(DeviceEvent::Telemetry(telemetry))) => {
                    if let Some(active) = &mut self.ride {
                        active.ride.on_telemetry(&telemetry);
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    events.push(AppEvent::Error(format!("trainer: {error}")));
                    self.trainer = None;
                    break;
                }
            }
        }
    }

    fn poll_sensor(&mut self, events: &mut Vec<AppEvent>) {
        let Some(sensor) = &mut self.sensor else {
            return;
        };
        loop {
            match sensor.try_next_event() {
                Ok(Some(DeviceEvent::Connected)) => {
                    events.push(AppEvent::Connected(sensor.name().to_owned()));
                }
                Ok(Some(DeviceEvent::Disconnected)) => {
                    events.push(AppEvent::Disconnected(sensor.name().to_owned()));
                }
                Ok(Some(DeviceEvent::Telemetry(telemetry))) => {
                    if let Some(active) = &mut self.ride {
                        active.ride.on_telemetry(&telemetry);
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    events.push(AppEvent::Error(format!("heart rate: {error}")));
                    self.sensor = None;
                    break;
                }
            }
        }
    }
}

/// Single-precision points for course previews, which need no more precision.
#[allow(clippy::cast_possible_truncation)] // metres in a course fit f32 easily
fn thin(points: &[(f64, f64)]) -> Vec<[f32; 2]> {
    points.iter().map(|&(a, b)| [a as f32, b as f32]).collect()
}

/// The fastest time on `climb` among `records`.
fn best_climb_time(records: &[&RideRecord], climb: &ClimbTime) -> Option<Duration> {
    records
        .iter()
        .flat_map(|r| r.climbs.iter())
        .filter(|other| other.same_climb(climb))
        .map(|other| other.elapsed)
        .min()
}

/// The timed positions of a recorded activity: a GPX file with times, or a FIT file.
fn activity_points(path: &Path) -> Result<Vec<torqa_routes::TimedPoint>, String> {
    let is_fit = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("fit"));
    if is_fit {
        let (start, samples) = read_fit(path)?;
        let start = start
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        return Ok(samples
            .iter()
            .map(|s| torqa_routes::TimedPoint {
                lat: s.lat,
                lon: s.lon,
                time: start + s.elapsed.as_secs_f64(),
            })
            .collect());
    }
    let xml = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let points = torqa_routes::timed_points(&xml).map_err(|e| e.to_string())?;
    if points.is_empty() {
        return Err(format!("{} has no times to race against", path.display()));
    }
    Ok(points)
}

fn read_fit(fit: &Path) -> Result<(SystemTime, Vec<Sample>), String> {
    let bytes = std::fs::read(fit).map_err(|e| format!("cannot read {}: {e}", fit.display()))?;
    torqa_storage::decode_fit(&bytes).map_err(|e| format!("{}: {e}", fit.display()))
}

/// The profile chosen last, else the first one, else a new default profile (saved, so it shows
/// up in the data directory to be edited).
fn initial_profile(data_dir: &Path) -> StoredProfile {
    let listed = profiles::list(data_dir);
    if let Some(found) = profiles::active(data_dir)
        .and_then(|id| listed.iter().find(|p| p.id == id).cloned())
        .or_else(|| listed.into_iter().next())
    {
        return found;
    }
    let profile = Profile::default();
    let id = profiles::new_id(data_dir, &profile.name);
    if let Err(error) = profiles::save(data_dir, &id, &profile) {
        warn!(%error, "cannot save the default profile");
    }
    StoredProfile { id, profile }
}

/// A free file name in `library` for a course called `name`.
fn unique_course_path(library: &Path, name: &str) -> PathBuf {
    let slug = torqa_storage::slug(name, "course");
    let mut path = library.join(format!("{slug}.{}", course::EXTENSION));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = library.join(format!("{slug}-{n}.{}", course::EXTENSION));
    }
    path
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use torqa_domain::units::{Rpm, Watts};

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("torqa-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A flat 400 m route with elevations, so no terrain download is needed.
    fn write_route(dir: &std::path::Path) -> PathBuf {
        let mut xml = String::from("<gpx><trk><name>Test loop</name><trkseg>");
        for i in 0..=40 {
            let lat = 46.0 + f64::from(i) * 10.0 / 111_195.0;
            let _ = write!(xml, r#"<trkpt lat="{lat}" lon="7"><ele>500</ele></trkpt>"#);
        }
        xml.push_str("</trkseg></trk></gpx>");
        let path = dir.join("route.gpx");
        std::fs::write(&path, xml).unwrap();
        path
    }

    /// Calls `update` like a 60 fps frame loop until `done` returns true (or 30 s pass).
    fn run_until(app: &mut App, mut done: impl FnMut(&AppEvent) -> bool) -> Vec<AppEvent> {
        let mut seen = Vec::new();
        for _ in 0..1800 {
            std::thread::sleep(Duration::from_millis(16));
            for event in app.update(Duration::from_millis(16)) {
                let stop = done(&event);
                seen.push(event);
                if stop {
                    return seen;
                }
            }
        }
        panic!("timed out; events so far: {seen:?}");
    }

    #[test]
    fn loads_a_route_in_the_background() {
        let dir = temp_dir("load");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();

        app.load_route(write_route(&dir), true);
        let events = run_until(&mut app, |e| matches!(e, AppEvent::RouteLoaded(_)));

        let Some(AppEvent::RouteLoaded(summary)) = events.last() else {
            unreachable!()
        };
        assert_eq!(summary.name, "Test loop");
        assert!((summary.length - 400.0).abs() < 1.0);
        assert!(app.route().is_some());
    }

    #[test]
    fn rides_with_the_fake_trainer_and_saves_a_fit_file() {
        let dir = temp_dir("ride");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.load_route(write_route(&dir), true);
        run_until(&mut app, |e| matches!(e, AppEvent::RouteLoaded(_)));

        app.connect_trainer(TrainerChoice::Fake(FakeRider {
            power: Watts(250.0),
            cadence: Rpm(90.0),
        }))
        .unwrap();
        app.start_ride(Percent(50.0), DescentMode::Coast, &GhostChoice::None)
            .unwrap();
        run_until(&mut app, |e| matches!(e, AppEvent::Connected(_)));
        // Ride three seconds of frames.
        for _ in 0..180 {
            std::thread::sleep(Duration::from_millis(16));
            app.update(Duration::from_millis(16));
        }
        let state = app.ride_state().unwrap();
        assert!(state.distance.0 > 1.0, "rider should be moving: {state:?}");
        assert_eq!(state.telemetry.power, Some(Watts(250.0)));
        let values = app.hud_values();
        assert_eq!(values.len(), hud::METRICS.len());
        let value = |id: &str| values.iter().find(|(m, _)| *m == id).and_then(|(_, v)| *v);
        assert_eq!(value("power_3s"), Some(250.0));
        assert_eq!(value("heart_rate"), None);
        assert!(value("distance").is_some_and(|km| km > 0.0));

        app.adjust_ride(Percent(80.0), DescentMode::Flat);
        let events = app.finish_ride();
        let Some(AppEvent::RideSaved(path)) = events.first() else {
            panic!("not saved: {events:?}");
        };
        assert!(path.starts_with(dir.join("data/profiles/rider/rides")));
        assert!(std::fs::metadata(path).unwrap().len() > 100);

        let history = app.history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].fit, *path);
        assert_eq!(history[0].record.route, "Test loop");
        assert!(history[0].record.summary.distance.0 > 1.0);
        let detail = app.ride_detail(path).unwrap();
        assert!(detail.samples.len() > 1);
        assert!(detail.power_zones.iter().sum::<Duration>() > Duration::ZERO);

        // Without metadata, e.g. a FIT file copied in by hand, the history rebuilds it.
        std::fs::remove_file(rides::metadata_path(path)).unwrap();
        assert_eq!(app.history().len(), 1);
        assert!(rides::metadata_path(path).exists());

        app.rename_ride(path, "  Lunch spin ").unwrap();
        assert_eq!(app.history()[0].record.name.as_deref(), Some("Lunch spin"));
        app.rename_ride(path, "").unwrap();
        assert_eq!(app.history()[0].record.name, None);

        app.delete_ride(path).unwrap();
        assert_eq!(app.history().len(), 0);
        app.shutdown();
    }

    #[test]
    fn a_saved_course_rides_on_another_machine() {
        let dir = temp_dir("course");
        let mut prepared = App::new(dir.join("a/data"), dir.join("a/cache")).unwrap();
        prepared.load_route(write_route(&dir), true);
        assert!(matches!(
            prepared.save_course(),
            Err(AppError::CourseNotReady)
        ));
        run_until(&mut prepared, |e| matches!(e, AppEvent::WorldReady { .. }));

        prepared.save_course().unwrap();
        let events = run_until(&mut prepared, |e| matches!(e, AppEvent::CourseAdded(_)));
        let Some(AppEvent::CourseAdded(file)) = events.last() else {
            unreachable!()
        };
        assert_eq!(*file, dir.join("a/data/courses/test-loop.tqc"));
        let listed = prepared.courses();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].manifest.name, "Test loop");
        assert!((listed[0].manifest.length_m - 400.0).abs() < 1.0);
        // Prepared once, listed once — the import added it, saving again found it — and with a
        // preview for its card.
        assert!(listed[0].manifest.track.len() > 2);
        assert!(listed[0].manifest.profile.len() > 2);

        let mut other = App::new(dir.join("b/data"), dir.join("b/cache")).unwrap();
        other.import_course(file.clone());
        let events = run_until(&mut other, |e| matches!(e, AppEvent::CourseAdded(_)));
        let Some(AppEvent::CourseAdded(imported)) = events.last() else {
            unreachable!()
        };
        assert!(imported.starts_with(dir.join("b/data/courses")));
        other.open_course(imported.clone());
        let events = run_until(&mut other, |e| matches!(e, AppEvent::WorldReady { .. }));

        let summary = events.iter().find_map(|e| match e {
            AppEvent::RouteLoaded(summary) => Some(summary),
            _ => None,
        });
        assert!(summary.is_some_and(|s| s.name == "Test loop" && (s.length - 400.0).abs() < 1.0));
        assert!(other.world().is_some());
    }

    #[test]
    fn saving_a_course_twice_keeps_both() {
        let library = temp_dir("unique");
        std::fs::write(library.join("lake-biel.tqc"), b"").unwrap();

        assert_eq!(
            unique_course_path(&library, "Lake Biel"),
            library.join("lake-biel-2.tqc")
        );
        assert_eq!(
            unique_course_path(&library, "Gurten / Bern!"),
            library.join("gurten-bern.tqc")
        );
        assert_eq!(
            unique_course_path(&library, "??"),
            library.join("course.tqc")
        );
    }

    #[test]
    fn generates_the_world_after_loading_a_route() {
        let dir = temp_dir("world");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();

        app.load_route(write_route(&dir), true);
        let events = run_until(&mut app, |e| matches!(e, AppEvent::WorldReady { .. }));

        assert!(matches!(events.last(), Some(AppEvent::WorldReady { chunks, .. }) if *chunks > 0));
        assert!(app.world().is_some_and(|w| !w.road.vertices.is_empty()));
    }

    #[test]
    fn reports_progress_through_all_stages() {
        let dir = temp_dir("progress");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();

        app.load_route(write_route(&dir), true);
        let events = run_until(&mut app, |e| matches!(e, AppEvent::WorldReady { .. }));

        for stage in [
            LoadStage::Route,
            LoadStage::Map,
            LoadStage::Elevation,
            LoadStage::World,
        ] {
            let finished = events.iter().any(|e| {
                matches!(e, AppEvent::LoadProgress { stage: s, done, total } if *s == stage && done == total)
            });
            assert!(finished, "{stage:?} not completed: {events:?}");
        }
    }

    /// 300 m flat, 600 m at 6 %, 300 m flat, due north.
    fn write_climb_route(dir: &std::path::Path) -> PathBuf {
        let mut xml = String::from("<gpx><trk><name>Hill</name><trkseg>");
        for i in 0..=120 {
            let lat = 46.0 + f64::from(i) * 10.0 / 111_195.0;
            let ele = 500.0 + f64::from((i - 30).clamp(0, 60)) * 0.6;
            let _ = write!(
                xml,
                r#"<trkpt lat="{lat}" lon="7"><ele>{ele}</ele></trkpt>"#
            );
        }
        xml.push_str("</trkseg></trk></gpx>");
        let path = dir.join("hill.gpx");
        std::fs::write(&path, xml).unwrap();
        path
    }

    /// Rides the loaded route with the fake trainer in fast-forward and saves it.
    fn ride_to_the_finish(app: &mut App, ghost: &GhostChoice) -> Vec<AppEvent> {
        app.connect_trainer(TrainerChoice::Fake(FakeRider {
            power: Watts(400.0),
            cadence: Rpm(90.0),
        }))
        .unwrap();
        app.start_ride(Percent(50.0), DescentMode::Coast, ghost)
            .unwrap();
        run_until(app, |e| matches!(e, AppEvent::Connected(_)));
        let mut events = Vec::new();
        for _ in 0..2000 {
            std::thread::sleep(Duration::from_millis(2));
            events.extend(app.update(Duration::from_millis(500)));
            if events.iter().any(|e| matches!(e, AppEvent::RideFinished)) {
                app.finish_ride();
                return events;
            }
        }
        panic!("did not finish: {events:?}");
    }

    #[test]
    fn climbs_and_routes_are_timed_against_the_riders_records() {
        let dir = temp_dir("records");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.load_route(write_climb_route(&dir), true);
        run_until(&mut app, |e| matches!(e, AppEvent::RouteLoaded(_)));
        assert_eq!(app.route().unwrap().climbs().len(), 1);

        // Nothing to race on a first ride.
        assert!(matches!(
            app.ghost_for(
                app.route().unwrap(),
                DescentMode::Coast,
                &GhostChoice::PersonalBest
            ),
            Err(AppError::GhostUnavailable(_))
        ));
        let first = ride_to_the_finish(&mut app, &GhostChoice::None);
        let climb = first.iter().find_map(|e| match e {
            AppEvent::ClimbCompleted {
                index: 0,
                elapsed,
                previous_best: None,
            } => Some(*elapsed),
            _ => None,
        });
        assert!(climb.is_some_and(|t| t > Duration::ZERO), "{first:?}");
        assert!(first.iter().any(|e| matches!(
            e,
            AppEvent::RouteCompleted {
                previous_best: None,
                ..
            }
        )));

        let records = app.records_for(app.route().unwrap());
        assert_eq!(records.climbs, [climb]);
        assert!(records.route.is_some());

        // Riding it again compares with the first ride.
        std::thread::sleep(Duration::from_millis(1100)); // a new FIT file name
        let second = ride_to_the_finish(&mut app, &GhostChoice::PersonalBest);
        assert!(second.iter().any(|e| matches!(
            e,
            AppEvent::ClimbCompleted {
                previous_best: Some(best),
                ..
            } if Some(*best) == climb
        )));
        let history = app.history();
        assert_eq!(history.len(), 2);
        // Racing the first ride at the same power: the gap stays about zero.
        app.start_ride(
            Percent(50.0),
            DescentMode::Coast,
            &GhostChoice::PersonalBest,
        )
        .unwrap();
        let ghost = app.ghost_state().unwrap();
        assert_eq!(ghost.name, "Your best");
        assert_eq!(ghost.distance, Meters(0.0));
        app.finish_ride();
        app.start_ride(
            Percent(50.0),
            DescentMode::Coast,
            &GhostChoice::WattsPerKg(3.0),
        )
        .unwrap();
        assert_eq!(app.ghost_state().unwrap().name, "Pacer 3.0 W/kg");
        app.finish_ride();
        // Each record belongs to a ride; equal times would count for both.
        assert!(history.iter().any(|h| h.route_record));
        assert!(history.iter().any(|h| h.climb_records == [true]));
        app.shutdown();
    }

    #[test]
    fn riders_keep_their_own_hud_layout() {
        let dir = temp_dir("hud");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        assert_eq!(app.hud_layout(), hud::DEFAULT_LAYOUT);

        let saved = app
            .set_hud_layout(&["power_3s".to_owned(), "nonsense".to_owned()])
            .unwrap();

        assert_eq!(saved, ["power_3s"]);
        assert_eq!(app.hud_layout(), ["power_3s"]);
        app.save_profile(None, Profile::default()).unwrap();
        assert_eq!(app.hud_layout(), hud::DEFAULT_LAYOUT);
    }

    #[test]
    fn riders_have_their_own_profiles_and_rides() {
        let dir = temp_dir("profiles");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        assert_eq!(app.profile().id, "rider");

        let anna = Profile {
            name: "Anna".to_owned(),
            ftp: Watts(280.0),
            ..Profile::default()
        };
        let id = app.save_profile(None, anna.clone()).unwrap();

        assert_eq!(app.profile().profile, anna);
        let names: Vec<String> = app.profiles().into_iter().map(|p| p.profile.name).collect();
        assert_eq!(names, ["Anna", "Rider"]);
        // The choice survives a restart.
        let restarted = App::new(dir.join("data"), dir.join("cache")).unwrap();
        assert_eq!(restarted.profile().id, id);
        assert!(matches!(
            app.select_profile("nobody"),
            Err(AppError::UnknownProfile)
        ));
    }

    #[test]
    fn aborted_rides_leave_no_trace() {
        let dir = temp_dir("abort");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.load_route(write_route(&dir), true);
        run_until(&mut app, |e| matches!(e, AppEvent::RouteLoaded(_)));
        app.connect_trainer(TrainerChoice::Fake(FakeRider {
            power: Watts(250.0),
            cadence: Rpm(90.0),
        }))
        .unwrap();
        app.start_ride(Percent(50.0), DescentMode::Coast, &GhostChoice::None)
            .unwrap();
        run_until(&mut app, |e| matches!(e, AppEvent::Connected(_)));
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(16));
            app.update(Duration::from_millis(50));
        }

        app.abort_ride();

        assert!(app.ride_state().is_none());
        assert_eq!(app.finish_ride(), []);
        assert_eq!(app.history().len(), 0);
        app.shutdown();
    }

    #[test]
    fn without_remembered_devices_nothing_is_scanned() {
        let dir = temp_dir("reconnect");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();

        // No Bluetooth is touched (the container has none): nothing to reconnect.
        assert!(!app.reconnect_remembered());
        assert_eq!(app.update(Duration::ZERO), []);
    }

    #[test]
    fn ride_needs_route_and_trainer() {
        let dir = temp_dir("needs");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();

        assert!(matches!(
            app.start_ride(Percent(50.0), DescentMode::Coast, &GhostChoice::None),
            Err(AppError::NoRoute)
        ));
        assert!(matches!(
            app.connect_heart_rate(0),
            Err(AppError::UnknownDevice)
        ));
    }
}
