//! Worlds authored outside ORQWA (R266–R269, ADR 0042): world files in the library's
//! `worlds/` folder, each of their routes a ride of its own. A route is known to the rest of the
//! app by a path, `<world file>#<route id>`, so the library, the course page and the rides on it
//! need no second way to name what is ridden.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use orqwa_routes::{Route, Surface, WorldPoint};
use orqwa_storage::course::Manifest;
use orqwa_storage::world::{self, Description, DrivesOn, WorldFile};
use tracing::warn;

use crate::CourseEntry;

/// Between a world file's path and a route's id in the path naming a world's route.
const ROUTE_MARK: char = '#';
/// The road's width where the author gives none (docs/world-files.md).
const DEFAULT_WIDTH: f64 = 5.0;

/// A route of a world file, as the library names it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldRoute {
    /// The world file.
    pub file: PathBuf,
    /// The route's id in it.
    pub route: String,
}

impl WorldRoute {
    /// The world route `path` names, if it names one.
    #[must_use]
    pub fn parse(path: &Path) -> Option<Self> {
        let text = path.to_str()?;
        let (file, route) = text.rsplit_once(ROUTE_MARK)?;
        let file = PathBuf::from(file);
        (file
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(world::EXTENSION))
            && !route.is_empty())
        .then(|| Self {
            file,
            route: route.to_owned(),
        })
    }

    /// The path naming this route.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        let mut text = self.file.clone().into_os_string();
        text.push(format!("{ROUTE_MARK}{}", self.route));
        PathBuf::from(text)
    }
}

/// What the library tells of a world's route beyond a course's manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldInfo {
    /// The world's name.
    pub world: String,
    /// The author's version of the world.
    pub version: Option<String>,
    /// Who made or sells it.
    pub publisher: Option<String>,
    /// Its licence.
    pub licence: String,
    /// The names of all its routes, deleted with it.
    pub routes: Vec<String>,
}

/// A world's route opened for riding.
#[derive(Debug)]
pub struct Authored {
    /// The world file.
    pub file: WorldFile,
    /// The route's id.
    pub route: String,
    /// The road's width at each of the route's points, metres.
    pub widths: Vec<f64>,
}

impl Authored {
    /// Whether rides on it keep their positions (R267).
    #[must_use]
    pub fn located(&self) -> bool {
        self.file.description().origin.located
    }
}

/// A world's route opened: the route as authored, whether traffic keeps left at each of its
/// points, the world itself and the route's name in the library.
pub type Opened = (Route, Vec<Option<bool>>, Authored, String);

/// What the library has read of each world file: its modification time, size and routes.
type Read = HashMap<PathBuf, (SystemTime, u64, Vec<CourseEntry>)>;

/// Opens the route `which` of a world file for riding.
///
/// # Errors
/// A readable message if the file or its route cannot be read.
pub fn open(which: &WorldRoute) -> Result<Opened, String> {
    let file = WorldFile::open(&which.file).map_err(|e| e.to_string())?;
    let entry = file
        .description()
        .route(&which.route)
        .ok_or_else(|| format!("the world has no route {}", which.route))?
        .clone();
    let points = file.route(&which.route).map_err(|e| e.to_string())?;
    let origin = file.description().origin;
    let name = display_name(file.description(), &entry.name);
    let route = Route::from_world(
        Some(name.clone()),
        (origin.lat, origin.lon),
        &points.points.iter().map(world_point).collect::<Vec<_>>(),
    )
    .map_err(|e| e.to_string())?;
    let widths = widths(&route, &points.points);
    let left = file.description().drives_on == DrivesOn::Left;
    let driving_left = vec![Some(left); route.points().len()];
    Ok((
        route,
        driving_left,
        Authored {
            file,
            route: which.route.clone(),
            widths,
        },
        name,
    ))
}

fn world_point(point: &world::RoutePoint) -> WorldPoint {
    WorldPoint {
        east: point.east,
        north: point.north,
        elevation: point.elevation,
        surface: match point.surface {
            world::Surface::Ground => Surface::Ground,
            world::Surface::Bridge => Surface::Bridge,
            world::Surface::Tunnel => Surface::Tunnel,
        },
    }
}

/// The road's width at each point of `route`: the authored width of the stretch it lies on.
fn widths(route: &Route, points: &[world::RoutePoint]) -> Vec<f64> {
    let mut along = Vec::with_capacity(points.len());
    let mut travelled = 0.0;
    for (index, point) in points.iter().enumerate() {
        if index > 0 {
            let before = &points[index - 1];
            travelled += (point.east - before.east).hypot(point.north - before.north);
        }
        along.push((travelled, point.width.unwrap_or(DEFAULT_WIDTH)));
    }
    route
        .points()
        .iter()
        .map(|p| {
            let at = along.partition_point(|(d, _)| *d <= p.distance.0 + 1e-6);
            along[at.saturating_sub(1)].1
        })
        .collect()
}

/// A route's name in the library: the world's and the route's, or the route's alone where it
/// is the world's only one and named like it.
fn display_name(world: &Description, route: &str) -> String {
    if world.routes.len() == 1 && route == world.name {
        route.to_owned()
    } else {
        format!("{} – {route}", world.name)
    }
}

/// The world files in `dir`, each route of each an entry, read once per file version.
#[derive(Debug, Default)]
pub struct Library {
    read: Mutex<Read>,
}

impl Library {
    /// The routes of the world files in `dir`. Unreadable files are skipped.
    #[must_use]
    pub fn entries(&self, dir: &Path) -> Vec<CourseEntry> {
        let Ok(listing) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut read = self
            .read
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut entries = Vec::new();
        let mut present = Vec::new();
        for path in listing.filter_map(Result::ok).map(|entry| entry.path()) {
            if path.extension().is_none_or(|e| e != world::EXTENSION) {
                continue;
            }
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };
            let version = (
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                metadata.len(),
            );
            present.push(path.clone());
            if let Some((modified, size, known)) = read.get(&path)
                && (*modified, *size) == version
            {
                entries.extend(known.iter().cloned());
                continue;
            }
            match routes_of(&path) {
                Ok(known) => {
                    entries.extend(known.iter().cloned());
                    read.insert(path, (version.0, version.1, known));
                }
                Err(error) => warn!(path = %path.display(), %error, "skipping world"),
            }
        }
        read.retain(|path, _| present.contains(path));
        entries
    }
}

/// Each route of the world file at `path`, as the library lists it.
fn routes_of(path: &Path) -> Result<Vec<CourseEntry>, String> {
    let file = WorldFile::open(path).map_err(|e| e.to_string())?;
    let description = file.description();
    let info = WorldInfo {
        world: description.name.clone(),
        version: description.version.clone(),
        publisher: description.publisher.clone(),
        licence: description.licence.clone(),
        routes: description.routes.iter().map(|r| r.name.clone()).collect(),
    };
    description
        .routes
        .iter()
        .map(|entry| {
            let which = WorldRoute {
                file: path.to_owned(),
                route: entry.id.clone(),
            };
            let (route, _, _, name) = open(&which)?;
            Ok(CourseEntry {
                path: which.path(),
                manifest: manifest(description, &route, name),
                world: Some(info.clone()),
            })
        })
        .collect()
}

/// What a course's manifest would say of a world's route.
fn manifest(world: &Description, route: &Route, name: String) -> Manifest {
    Manifest {
        format: orqwa_storage::course::FORMAT_VERSION,
        generator: world
            .publisher
            .clone()
            .unwrap_or_else(|| "ORQWA World".to_owned()),
        name,
        length_m: route.length().0,
        elevation_gain_m: route.elevation_gain().0,
        max_grade_percent: route.max_grade().0,
        created_unix_s: world.created_unix_s,
        attribution: world.attribution.clone(),
        route_key: Some(route.key()),
        track: crate::thin(&crate::view::track(route, crate::PREVIEW_POINTS)),
        profile: crate::thin(&crate::view::elevation_profile(
            route,
            crate::PREVIEW_POINTS,
        )),
        video: None,
        // A world's place is the author's to tell; none is looked up (R269: nothing is fetched).
        countries: Some(Vec::new()),
        map_detail: Vec::new(),
    }
}

/// Copies a checked world file into `dir`: a new version of a world already there (by its id)
/// replaces it, under the same name. Returns where it is.
///
/// # Errors
/// A readable message if the file breaks the format or cannot be copied.
pub fn import(from: &Path, dir: &Path) -> Result<PathBuf, String> {
    let description = world::validate(from).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let existing = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|e| e == world::EXTENSION))
        .find(|p| world::read_description(p).is_ok_and(|d| d.id == description.id));
    let to = existing.unwrap_or_else(|| {
        let slug = orqwa_storage::slug(&description.name, "world");
        let mut path = dir.join(format!("{slug}.{}", world::EXTENSION));
        let mut n = 1;
        while path.exists() {
            n += 1;
            path = dir.join(format!("{slug}-{n}.{}", world::EXTENSION));
        }
        path
    });
    let partial = to.with_extension("oqx.part");
    std::fs::copy(from, &partial).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, &to).map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        e.to_string()
    })?;
    Ok(to)
}

/// The rider's position across a world's road: its width at each point of the route, along
/// the line down its middle.
#[must_use]
pub fn road_widths(route: &Route, widths: &[f64]) -> orqwa_world::RoadWidths {
    let projection = orqwa_routes::LocalProjection::for_route(route);
    let along: Vec<([f64; 3], f64)> = route
        .points()
        .iter()
        .zip(widths)
        .map(|(p, &width)| {
            let (east, north) = projection.project(p.lat, p.lon);
            ([east, p.elevation.0, -north], width)
        })
        .collect();
    orqwa_world::RoadWidths::along(&along)
}
