//! The Torqa application layer: devices, routes, the ride engine and storage behind a simple,
//! frame-driven API. Front ends (the Godot app, tests) call commands and [`App::update`] once per
//! frame; all asynchronous work runs on an internal runtime, so callers never block or await.

mod import;
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
use torqa_routes::{ElevationSource, Route};
use torqa_session::{Ride, RideConfig, RideState};
use torqa_storage::course::{self, Manifest};
use torqa_terrain::{Terrain, TileSource};
use torqa_world::World;
use tracing::warn;

/// Credits for the data a course bundles, stored in course files (ODbL, CC BY).
const ATTRIBUTION: [&str; 3] = [
    "© OpenFreeMap © OpenMapTiles · Data © OpenStreetMap contributors (ODbL)",
    "Terrain: Mapterhorn (CC BY 4.0)",
    "Terrain: AWS Terrain Tiles",
];

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
    /// A device lost its connection; it reconnects automatically.
    Disconnected(String),
    /// The rider reached the finish.
    RideFinished,
    /// The ride was saved as a FIT file.
    RideSaved(PathBuf),
    /// A course was saved or imported into the library.
    CourseAdded(PathBuf),
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
    route: Option<Route>,
    world: Option<Arc<World>>,
    offline: bool,
    /// Name and GPX of the loaded route, for saving it as a course.
    loaded: Option<(String, String)>,
    /// Cached files the loaded route and its world were built from.
    used: UsedFiles,
    ride: Option<ActiveRide>,
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
            route: None,
            world: None,
            offline: false,
            loaded: None,
            used: UsedFiles::default(),
            ride: None,
            data_dir,
            cache_dir,
        })
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
        };
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
            TrainerChoice::Fake(rider) => fake::spawn(rider, Duration::from_millis(250)),
            TrainerChoice::Discovered(index) => {
                let device = self.discovered_device(index, DeviceKind::Trainer)?;
                self.bluetooth()?.connect(device)
            }
        };
        self.trainer = Some(handle);
        Ok(())
    }

    /// Connects a heart-rate sensor, replacing any previous one.
    ///
    /// # Errors
    /// [`AppError::UnknownDevice`] if the index is not a heart-rate sensor from the last scan.
    pub fn connect_heart_rate(&mut self, index: usize) -> Result<(), AppError> {
        let device = self.discovered_device(index, DeviceKind::HeartRateSensor)?;
        let _runtime = self.runtime.enter();
        self.sensor = Some(self.bluetooth()?.connect(device));
        Ok(())
    }

    /// Starts riding the loaded route. The clock starts once the trainer is connected.
    ///
    /// # Errors
    /// [`AppError::NoRoute`] or [`AppError::NoTrainer`] if either is missing.
    pub fn start_ride(&mut self, config: RideConfig) -> Result<(), AppError> {
        let route = self.route.clone().ok_or(AppError::NoRoute)?;
        if self.trainer.is_none() {
            return Err(AppError::NoTrainer);
        }
        self.ride = Some(ActiveRide {
            ride: Ride::new(route, config),
            started: None,
            finished: false,
        });
        Ok(())
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
        let path = self
            .data_dir
            .join("rides")
            .join(paths::activity_file_name(start));
        let saved = torqa_storage::encode_fit(start, active.ride.samples())
            .map_err(|e| e.to_string())
            .and_then(|fit| {
                std::fs::create_dir_all(path.parent().unwrap_or(&self.data_dir))
                    .and_then(|()| std::fs::write(&path, fit))
                    .map_err(|e| format!("cannot save {}: {e}", path.display()))
            });
        vec![match saved {
            Ok(()) => AppEvent::RideSaved(path),
            Err(message) => AppEvent::Error(message),
        }]
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
            if active.ride.is_finished() {
                active.finished = true;
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
                        })
                        .collect();
                    self.bluetooth = Some(bluetooth);
                    self.discovered = devices;
                    events.push(AppEvent::DevicesFound(infos));
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
                }
                JobResult::CourseAdded(Ok(path)) => events.push(AppEvent::CourseAdded(path)),
                JobResult::Scan(Err(message))
                | JobResult::Route(Err(message))
                | JobResult::CourseAdded(Err(message)) => {
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

/// A free file name in `library` for a course called `name`.
fn unique_course_path(library: &Path, name: &str) -> PathBuf {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.is_empty() {
        "course".to_owned()
    } else {
        slug
    };
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
        app.start_ride(RideConfig::default()).unwrap();
        run_until(&mut app, |e| matches!(e, AppEvent::Connected(_)));
        // Ride three seconds of frames.
        for _ in 0..180 {
            std::thread::sleep(Duration::from_millis(16));
            app.update(Duration::from_millis(16));
        }
        let state = app.ride_state().unwrap();
        assert!(state.distance.0 > 1.0, "rider should be moving: {state:?}");
        assert_eq!(state.telemetry.power, Some(Watts(250.0)));

        let events = app.finish_ride();
        let Some(AppEvent::RideSaved(path)) = events.first() else {
            panic!("not saved: {events:?}");
        };
        assert!(path.starts_with(dir.join("data").join("rides")));
        assert!(std::fs::metadata(path).unwrap().len() > 100);
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

    #[test]
    fn ride_needs_route_and_trainer() {
        let dir = temp_dir("needs");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();

        assert!(matches!(
            app.start_ride(RideConfig::default()),
            Err(AppError::NoRoute)
        ));
        assert!(matches!(
            app.connect_heart_rate(0),
            Err(AppError::UnknownDevice)
        ));
    }
}
