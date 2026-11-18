//! The courses' worlds this computer keeps (R101, ADR 0025): once a library course's world is
//! built it is kept in the cache, and **Ride** reads it instead of building it again. Off until
//! the rider turns it on, as a world takes about 10 MB per kilometre of route. A kept world
//! stays across updates; one an older generator built is ridden until the rider has it built
//! again ([`App::regenerate_world`]).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use orqwa_domain::units::Meters;
use orqwa_routes::Route;
use orqwa_storage::computer;
use orqwa_storage::course;
use orqwa_storage::worlds::{Building, KeptWorlds};
use orqwa_world::{GENERATOR, World};
use tracing::{info, warn};

use crate::failure::{Failure, Kind};
use crate::world_chunks::WorldChunks;
use crate::{App, AppError, CourseEntry};

/// What a world's store takes at most per kilometre of route, above the densest course
/// measured: Wada's 20.1 MB (ADR 0040).
const STORE_PER_KM: f64 = 25_000_000.0;

/// How many bytes of storage are free on the filesystem that holds a directory, if that can be
/// told.
pub type FreeStorage = Arc<dyn Fn(&Path) -> Option<u64> + Send + Sync>;

/// A world this computer keeps, as the settings list it to delete (R136).
#[derive(Debug, Clone, PartialEq)]
pub struct KeptWorld {
    /// The library's courses it is the world of, by name: one, unless the same course is in
    /// the library twice.
    pub courses: Vec<CourseEntry>,
    /// How many bytes it takes on disk.
    pub size: u64,
}

/// What the cache holds of a course's world.
pub(crate) enum Found {
    /// Its world kept: the parts built whole, the version of the generator that built it, and
    /// its chunks to read when needed.
    World(Box<World>, u32, Box<WorldChunks>),
    /// None to ride: one built is kept under this fingerprint.
    Missing(String),
    /// Nothing that can be told: the course file cannot be read.
    Unknown,
}

/// Where kept worlds are: the cache, the setting to check once one is written (it may have
/// been turned off meanwhile) and the library whose courses' worlds stay.
#[derive(Clone)]
pub(crate) struct Keeper {
    kept: KeptWorlds,
    computer: PathBuf,
    library: PathBuf,
}

impl Keeper {
    /// The kept world of the library course at `course`, if there is one. One damaged or of
    /// another format is built again.
    pub(crate) fn find(&self, course: &Path) -> Found {
        let fingerprint = match course::fingerprint(course) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                warn!(%error, course = %course.display(), "cannot tell the course's kept world");
                return Found::Unknown;
            }
        };
        match self.kept.open(&fingerprint) {
            Ok(Some((world, generator, chunks))) => {
                info!(chunks = chunks.len(), generator, "riding the kept world");
                Found::World(
                    Box::new(world),
                    generator,
                    Box::new(WorldChunks::kept(chunks)),
                )
            }
            Ok(None) => Found::Missing(fingerprint),
            Err(error) => {
                warn!(%error, "the kept world cannot be read; building it again");
                Found::Missing(fingerprint)
            }
        }
    }

    /// A store to build a world into (ADR 0040). The caller chooses whether failure may
    /// fall back to memory: that is unsafe on Android.
    pub(crate) fn store(&self, name: &str) -> std::io::Result<Building> {
        self.kept.building(name).inspect(|store| {
            if !store.locked() {
                warn!("cannot lock the world's store; another ORQWA may tidy it once old");
            }
        })
    }

    /// Keeps the world whose parts built whole are `world` and whose chunks are `chunks`,
    /// built by the generator of version `generator`, as that of the course at `course`, and
    /// drops the worlds no course of the library has any more. Blocks for seconds.
    pub(crate) fn keep(&self, course: &Path, world: &World, chunks: &WorldChunks, generator: u32) {
        match course::fingerprint(course) {
            Ok(fingerprint) => self.keep_as(&fingerprint, world, chunks, generator),
            Err(error) => {
                warn!(%error, course = %course.display(), "cannot keep the course's world");
                self.tidy();
            }
        }
    }

    /// Keeps the world as [`keep`](Self::keep) does, as that of the course with
    /// `fingerprint`. Built into a store, its chunks are copied from it as they are, and read
    /// from the kept world from then on.
    pub(crate) fn keep_as(
        &self,
        fingerprint: &str,
        world: &World,
        chunks: &WorldChunks,
        generator: u32,
    ) {
        // Turned on again while it is still kept: writing it again only costs time.
        if self.kept.generator(fingerprint) != Some(generator) {
            chunks.keep(&self.kept, fingerprint, world, generator);
        }
        self.tidy();
    }

    /// Drops the worlds of courses gone from the library and those this ORQWA cannot read, or
    /// all of them once keeping them is turned off.
    pub(crate) fn tidy(&self) {
        let tidied = if computer::keep_worlds(&self.computer) {
            self.kept.keep_only(&library_fingerprints(&self.library))
        } else {
            self.kept.clear()
        };
        if let Err(error) = tidied {
            warn!(%error, "cannot tidy the kept worlds");
        }
    }
}

/// The fingerprints of the courses in `library`.
pub(crate) fn library_fingerprints(library: &Path) -> HashSet<String> {
    std::fs::read_dir(library)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == course::EXTENSION))
        .filter_map(|path| course::fingerprint(&path).ok())
        .collect()
}

impl App {
    /// Whether this computer keeps the courses' worlds once built (R101); no until turned on.
    #[must_use]
    pub fn keeps_worlds(&self) -> bool {
        computer::keep_worlds(&self.computer)
    }

    /// Turns keeping the courses' worlds on or off for this computer. On, the world of the
    /// course loaded is kept at once, or once complete if it is still being built; off, every
    /// kept world is deleted.
    ///
    /// # Errors
    /// [`AppError::Reported`] if it cannot be saved.
    pub fn set_keeps_worlds(&mut self, on: bool) -> Result<(), AppError> {
        computer::set_keep_worlds(&self.computer, on)
            .map_err(|e| AppError::Reported(Failure::new(Kind::SaveSetting, e)))?;
        match (on, &self.world, &self.chunks, &self.course) {
            // A world still being built is kept once complete.
            (true, Some(_), Some(chunks), Some(_)) if !chunks.complete() => {}
            (true, Some(world), Some(chunks), Some(course)) => {
                self.keep_world(course.clone(), Arc::clone(world), Arc::clone(chunks));
            }
            _ => self.tidy_kept_worlds(),
        }
        self.stop_background_unless_kept();
        Ok(())
    }

    /// How many bytes the kept worlds take on this computer.
    #[must_use]
    pub fn kept_worlds_size(&self) -> u64 {
        self.keeper().kept.size()
    }

    /// The worlds this computer keeps of the library's courses, by the name of their first
    /// course (R136); none while it keeps none, as turning it off deletes them all.
    #[must_use]
    pub fn kept_worlds(&self) -> Vec<KeptWorld> {
        if !self.keeps_worlds() {
            return Vec::new();
        }
        let kept = self.keeper().kept;
        let mut worlds: Vec<(String, KeptWorld)> = Vec::new();
        for entry in self.courses() {
            let Ok(fingerprint) = course::fingerprint(&entry.path) else {
                continue;
            };
            if let Some((_, world)) = worlds.iter_mut().find(|(f, _)| *f == fingerprint) {
                world.courses.push(entry);
            } else if let Some(size) = kept.size_of(&fingerprint) {
                let courses = vec![entry];
                worlds.push((fingerprint, KeptWorld { courses, size }));
            }
        }
        worlds.into_iter().map(|(_, world)| world).collect()
    }

    /// Deletes the kept world of the course at `course` (R136), and so of every course it is
    /// the world of: the course stays, and its next ride builds and keeps it again.
    ///
    /// # Errors
    /// [`AppError::Reported`] if the world cannot be deleted.
    pub fn delete_kept_world(&self, course: &Path) -> Result<(), AppError> {
        let fingerprint = course::fingerprint(course).map_err(|e| {
            AppError::Reported(Failure::new(Kind::OpenFile, e).arg(course.display()))
        })?;
        let kept = self.keeper().kept;
        kept.remove(&fingerprint).map_err(|e| {
            let file = kept.file(&fingerprint);
            AppError::Reported(Failure::new(Kind::DeleteFile, e).arg(file.display()))
        })
    }

    /// Whether the course at `course` has a world kept that an older generator built, which
    /// [`App::regenerate_world`] builds again. Courses with a video are ridden along it and
    /// never asked.
    #[must_use]
    pub fn kept_world_outdated(&self, course: &Path) -> bool {
        self.keeps_worlds()
            && course::read_manifest(course).is_ok_and(|manifest| manifest.video.is_none())
            && course::fingerprint(course).is_ok_and(|fingerprint| {
                self.keeper()
                    .kept
                    .generator(&fingerprint)
                    .is_some_and(|generator| generator < GENERATOR)
            })
    }

    /// Builds the world of the course loaded again with this ORQWA's generator, in place of the
    /// one kept (R101): the kept one is deleted, and the course opened anew and its world built
    /// and kept as for a ride, reported by [`AppEvent::WorldReady`](crate::AppEvent::WorldReady).
    /// False if no library course is loaded.
    ///
    /// # Errors
    /// [`Kind::WorldStorage`] where free storage is checked and the new world has no room, also
    /// counting what deleting the kept one gives back: the kept one then stays, to ride on.
    pub fn regenerate_world(&mut self) -> Result<bool, AppError> {
        let Some(course) = self.course.clone() else {
            return Ok(false);
        };
        let fingerprint = course::fingerprint(&course).ok();
        if let Some(length) = self.route.as_ref().map(Route::length) {
            let freed = fingerprint
                .as_deref()
                .and_then(|fingerprint| self.keeper().kept.size_of(fingerprint))
                .unwrap_or(0);
            self.room_giving_way(length, freed)
                .map_err(AppError::Reported)?;
        }
        if let Some(fingerprint) = &fingerprint
            && let Err(error) = self.keeper().kept.remove(fingerprint)
        {
            warn!(%error, "cannot delete the kept world");
        }
        self.open_course(course);
        self.build_world(false, crate::WHOLE_WORLD);
        Ok(true)
    }

    /// Drops, in the background, the kept worlds of courses gone from the library (also on
    /// another computer sharing it) and those this ORQWA cannot read, or all once keeping them
    /// is off.
    pub fn tidy_kept_worlds(&self) {
        let keeper = self.keeper();
        self.runtime.spawn_blocking(move || keeper.tidy());
    }

    /// Has every world built checked first for room for its store in the cache, `free` telling
    /// how much storage is free (ADR 0040). For Android: where a store cannot be written, the
    /// chunks are held in memory, which on a tablet is the low-memory kill the store is there to
    /// avoid, so the rider is told how much room the course needs instead. Desktops have the
    /// memory to fall back on and are not checked.
    pub fn check_free_storage(&mut self, free: FreeStorage) {
        self.free_storage = Some(free);
    }

    /// Requires a writable world store even when free-space checks succeed or cannot run.
    /// Android cannot safely hold an unstored world in memory (R100).
    pub fn require_world_store(&mut self) {
        self.builds.required = true;
    }

    /// Whether the cache has room for the store of `route`'s world, about to be built: where
    /// free storage is checked, refused with how much it needs and how much is free. A world
    /// ridden from where it is kept needs none.
    pub(crate) fn room_for_world(&mut self, route: &Route) -> Result<(), Failure> {
        if self.free_storage.is_none() {
            return Ok(());
        }
        let fingerprint = self
            .course
            .as_deref()
            .and_then(|course| course::fingerprint(course).ok());
        // One that can't be read is built again, so it takes room like any other.
        let kept = fingerprint
            .as_deref()
            .filter(|_| self.keeps_worlds())
            .is_some_and(|fingerprint| self.keeper().kept.generator(fingerprint).is_some());
        if kept {
            return Ok(());
        }
        self.room_giving_way(route.length(), 0)
    }

    /// [`room`](Self::room), the store a stopped build left giving way if short: it is not
    /// among the kept worlds the rider can delete, and the room it takes would otherwise refuse
    /// a ride that had room before its build was stopped. The check waits for it to be deleted.
    /// Left by the same course's build, the world is then built afresh.
    fn room_giving_way(&mut self, length: Meters, freed: u64) -> Result<(), Failure> {
        let room = self.room(length, freed);
        if room.is_err() && self.forget_any_stopped() {
            return self.room(length, freed);
        }
        room
    }

    /// Whether the cache has room for the store of the world of a route `length` long, once
    /// `freed` bytes are given back: refused with how much it needs and how much is free.
    fn room(&mut self, length: Meters, freed: u64) -> Result<(), Failure> {
        let Some(free) = self.free_storage.clone() else {
            return Ok(());
        };
        let needed = store_size(length);
        let short = |left: Option<u64>| {
            left.map(|left| left.saturating_add(freed))
                .filter(|left| *left < needed)
        };
        let mut left = short(free(&self.cache_dir));
        if left.is_some() && !self.letting_go.is_empty() {
            // The world of a course closed just before may still be being deleted: its room is
            // coming back, which the rider should not be told to make.
            let letting_go = std::mem::take(&mut self.letting_go);
            self.runtime.block_on(async {
                for deleting in letting_go {
                    let _ = deleting.await;
                }
            });
            left = short(free(&self.cache_dir));
        }
        match left {
            Some(left) => Err(Failure::new(
                Kind::WorldStorage,
                format!("{left} bytes free in the cache, the world's store needs {needed}"),
            )
            .arg(megabytes(needed.div_ceil(MEGABYTE)))
            .arg(megabytes(left / MEGABYTE))),
            None => Ok(()),
        }
    }

    pub(crate) fn keeper(&self) -> Keeper {
        Keeper {
            kept: KeptWorlds::new(&self.cache_dir),
            computer: self.computer.clone(),
            library: self.courses_dir(),
        }
    }

    /// Keeps the loaded world, its parts built whole `world` and its chunks `chunks`, as that
    /// of the library course at `course`, in the background.
    pub(crate) fn keep_world(&self, course: PathBuf, world: Arc<World>, chunks: Arc<WorldChunks>) {
        let keeper = self.keeper();
        let generator = self.world_generator;
        self.runtime
            .spawn_blocking(move || keeper.keep(&course, &world, &chunks, generator));
    }

    /// Keeps the world just built as that of the course with `fingerprint`, in the
    /// background, unless keeping worlds was turned off or the course closed meanwhile.
    pub(crate) fn keep_built_world(
        &self,
        fingerprint: String,
        world: Arc<World>,
        chunks: Arc<WorldChunks>,
    ) {
        if self.keeps_worlds() && self.course.is_some() {
            let keeper = self.keeper();
            let generator = self.world_generator;
            self.runtime.spawn_blocking(move || {
                keeper.keep_as(&fingerprint, &world, &chunks, generator);
            });
        }
    }
}

const MEGABYTE: u64 = 1_000_000;

/// At most how many bytes the store of the world of a route `length` long takes.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // positive, far below 2⁶⁴
fn store_size(length: Meters) -> u64 {
    (length.0.max(0.0) / 1000.0 * STORE_PER_KM).ceil() as u64
}

/// `count` megabytes, in the decimal megabytes Android's storage settings show, without
/// separators that differ between languages.
fn megabytes(count: u64) -> String {
    format!("{count} MB")
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::import::LoadStage;
    use crate::tests::{run_until, temp_dir, write_route};
    use crate::{AppEvent, WinterMask};

    /// Updates `app` until `done` holds, as the kept worlds are written in the background.
    fn wait_for(app: &mut App, mut done: impl FnMut(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !done(app) {
            assert!(Instant::now() < deadline, "waited in vain");
            std::thread::sleep(Duration::from_millis(16));
            app.update(Duration::from_millis(16));
        }
    }

    /// Prepares the test route as a course of `app`'s library.
    fn prepare(app: &mut App, dir: &Path) -> PathBuf {
        prepare_gpx(app, write_route(dir))
    }

    /// Prepares the route of the GPX file at `gpx` as a course of `app`'s library.
    fn prepare_gpx(app: &mut App, gpx: PathBuf) -> PathBuf {
        app.load_route(gpx, true);
        let events = run_until(app, |e| matches!(e, AppEvent::CourseAdded(_)));
        let Some(AppEvent::CourseAdded(course)) = events.last() else {
            unreachable!()
        };
        course.clone()
    }

    /// Opens `course` and builds its world for a ride; how many fallback samples it has, which
    /// tells a world marked in its kept file apart from one built again.
    fn ride(app: &mut App, course: &Path) -> usize {
        app.open_course(course.to_owned());
        app.build_world(false, crate::WHOLE_WORLD);
        let events = run_until(app, |e| {
            matches!(e, AppEvent::WorldReady { .. } | AppEvent::Error(_))
        });
        match events.last() {
            Some(AppEvent::WorldReady {
                fallback_samples, ..
            }) => *fallback_samples,
            other => panic!("no world: {other:?}"),
        }
    }

    const MARK: usize = 424_242;

    /// Marks the kept world of the course with `fingerprint`, as built by the generator of
    /// version `generator`.
    fn mark(keeper: &Keeper, fingerprint: &str, generator: u32) {
        let (mut world, _) = read(keeper, fingerprint);
        world.fallback_samples = MARK;
        keeper.kept.write(fingerprint, &world, generator).unwrap();
    }

    /// The kept world of the course with `fingerprint`, read whole, and the version of the
    /// generator that built it.
    fn read(keeper: &Keeper, fingerprint: &str) -> (World, u32) {
        let (mut world, generator, chunks) = keeper.kept.open(fingerprint).unwrap().expect("kept");
        world.chunks = (0..chunks.len())
            .map(|index| chunks.read(index).unwrap())
            .collect();
        (world, generator)
    }

    /// The entries of the kept worlds' directory under `dir`, by name.
    fn worlds_left(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(dir.join("cache/worlds"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The loaded world of `app`, read whole.
    pub(crate) fn loaded(app: &App) -> World {
        let chunks = app.world_chunks().expect("chunks");
        World {
            chunks: (0..chunks.len())
                .map(|index| chunks.get(index).expect("a chunk").into_owned())
                .collect(),
            ..app.world().expect("a world").clone()
        }
    }

    #[test]
    fn a_course_ridden_again_rides_its_kept_world() {
        let dir = temp_dir("kept-ride");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        assert!(!app.keeps_worlds(), "off until turned on (R101)");
        app.set_keeps_worlds(true).unwrap();
        // Prepared from its GPX: the world built for it is kept for the course.
        let course = prepare(&mut app, &dir);
        let keeper = app.keeper();
        let fingerprint = course::fingerprint(&course).unwrap();
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));
        mark(&keeper, &fingerprint, GENERATOR);

        assert_eq!(ride(&mut app, &course), MARK, "ridden from the kept world");
        assert!(!app.kept_world_outdated(&course), "built by this generator");
        // Renamed, it is still the same world.
        app.rename_course(&course, "Evening loop").unwrap();
        assert_eq!(ride(&mut app, &course), MARK);

        // Turned off, the kept worlds go and the course is built again.
        app.set_keeps_worlds(false).unwrap();
        wait_for(&mut app, |app| app.kept_worlds_size() == 0);
        assert_ne!(ride(&mut app, &course), MARK);
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(16));
            app.update(Duration::from_millis(16));
        }
        assert_eq!(app.kept_worlds_size(), 0, "and none is kept");
        app.shutdown();
    }

    #[test]
    fn turned_on_the_world_loaded_is_kept_until_its_course_is_deleted() {
        let dir = temp_dir("kept-delete");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let course = prepare(&mut app, &dir);
        let keeper = app.keeper();
        let fingerprint = course::fingerprint(&course).unwrap();
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(16));
            app.update(Duration::from_millis(16));
        }
        assert_eq!(app.kept_worlds_size(), 0, "nothing kept while off");

        app.set_keeps_worlds(true).unwrap();
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));
        assert!(app.kept_worlds_size() > 0);

        app.delete_course(&course).unwrap();
        wait_for(&mut app, |_| !keeper.kept.has(&fingerprint));
        app.shutdown();
    }

    #[test]
    fn an_older_generators_world_is_ridden_until_the_rider_has_it_built_again() {
        let dir = temp_dir("kept-older");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.set_keeps_worlds(true).unwrap();
        let course = prepare(&mut app, &dir);
        let keeper = app.keeper();
        let fingerprint = course::fingerprint(&course).unwrap();
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));
        assert!(!app.kept_world_outdated(&course));
        // Kept before an update that changed how worlds look.
        mark(&keeper, &fingerprint, GENERATOR - 1);
        assert!(
            app.kept_world_outdated(&course),
            "the course offers to build it again"
        );

        assert_eq!(
            ride(&mut app, &course),
            MARK,
            "still ridden at once, as it is"
        );
        // Turned off and on again, the world loaded is kept as the older generator built it.
        app.set_keeps_worlds(false).unwrap();
        wait_for(&mut app, |app| app.kept_worlds_size() == 0);
        app.set_keeps_worlds(true).unwrap();
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));
        assert!(app.kept_world_outdated(&course));

        assert!(app.regenerate_world().unwrap());
        let events = run_until(&mut app, |e| {
            matches!(e, AppEvent::WorldReady { .. } | AppEvent::Error(_))
        });
        assert!(
            matches!(events.last(), Some(AppEvent::WorldReady { fallback_samples, .. }) if *fallback_samples != MARK),
            "built anew: {events:?}"
        );
        wait_for(&mut app, |_| {
            keeper.kept.generator(&fingerprint) == Some(GENERATOR)
        });
        assert!(!app.kept_world_outdated(&course));
        assert_ne!(
            ride(&mut app, &course),
            MARK,
            "and kept in place of the older"
        );
        app.shutdown();
    }

    #[test]
    fn the_settings_list_each_kept_world_with_its_room_to_delete_it() {
        let dir = temp_dir("kept-list");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.set_keeps_worlds(true).unwrap();
        assert_eq!(app.kept_worlds(), Vec::new(), "none kept yet");
        let lakeside = prepare(&mut app, &dir);
        app.rename_course(&lakeside, "Lakeside").unwrap();
        // Another route, a kilometre east.
        let east = std::fs::read_to_string(write_route(&dir))
            .unwrap()
            .replace(r#"lon="7""#, r#"lon="7.013""#);
        let gpx = dir.join("east.gpx");
        std::fs::write(&gpx, east).unwrap();
        let hills = prepare_gpx(&mut app, gpx);
        app.rename_course(&hills, "Hills").unwrap();
        // The same course twice in the library has one world.
        let again = lakeside.with_file_name("again.oqc");
        std::fs::copy(&lakeside, &again).unwrap();
        app.rename_course(&again, "Lakeside again").unwrap();
        let keeper = app.keeper();
        let [lakeside_world, hills_world] =
            [&lakeside, &hills].map(|c| course::fingerprint(c).unwrap());
        assert_ne!(lakeside_world, hills_world);
        wait_for(&mut app, |_| {
            keeper.kept.has(&lakeside_world) && keeper.kept.has(&hills_world)
        });

        let names = |app: &App| -> Vec<Vec<String>> {
            app.kept_worlds()
                .iter()
                .map(|world| {
                    let courses = world.courses.iter();
                    courses.map(|c| c.manifest.name.clone()).collect()
                })
                .collect()
        };
        assert_eq!(
            names(&app),
            [vec!["Hills"], vec!["Lakeside", "Lakeside again"]],
            "by name, a world shared by two courses once"
        );
        let worlds = app.kept_worlds();
        assert_eq!(Some(worlds[0].size), keeper.kept.size_of(&hills_world));
        assert_eq!(Some(worlds[1].size), keeper.kept.size_of(&lakeside_world));
        assert_eq!(
            worlds.iter().map(|world| world.size).sum::<u64>(),
            app.kept_worlds_size(),
            "all the room the kept worlds take is listed"
        );

        app.delete_kept_world(&again).unwrap();
        assert!(
            !keeper.kept.has(&lakeside_world),
            "deleted for both its courses"
        );
        assert_eq!(names(&app), [vec!["Hills"]], "the other world stays");
        assert!(lakeside.exists() && again.exists(), "and the courses do");
        app.delete_kept_world(&again)
            .expect("a list shown before it went may delete it again");
        // Its next ride builds and keeps it again.
        ride(&mut app, &lakeside);
        wait_for(&mut app, |_| keeper.kept.has(&lakeside_world));
        assert_eq!(names(&app).len(), 2);

        // Turned off, they are all being deleted.
        app.set_keeps_worlds(false).unwrap();
        assert_eq!(app.kept_worlds(), Vec::new());
        app.shutdown();
    }

    /// The steps of making `course`'s world reported while it is opened, in order.
    fn steps(app: &mut App, course: &Path) -> Vec<(LoadStage, usize, usize)> {
        app.open_course(course.to_owned());
        app.build_world(false, crate::WHOLE_WORLD);
        run_until(app, |e| matches!(e, AppEvent::WorldReady { .. }))
            .into_iter()
            .filter_map(|e| match e {
                AppEvent::LoadProgress { stage, done, total }
                    if matches!(stage, LoadStage::KeptWorld | LoadStage::World) =>
                {
                    Some((stage, done, total))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn reading_a_kept_world_and_building_a_deleted_one_show_as_such() {
        let dir = temp_dir("kept-progress");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.set_keeps_worlds(true).unwrap();
        let course = prepare(&mut app, &dir);
        let keeper = app.keeper();
        let fingerprint = course::fingerprint(&course).unwrap();
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));

        let read = steps(&mut app, &course);
        assert_eq!(read.first(), Some(&(LoadStage::KeptWorld, 0, 1)));
        assert_eq!(read.last(), Some(&(LoadStage::KeptWorld, 1, 1)));
        assert!(
            read.iter()
                .all(|(stage, ..)| *stage == LoadStage::KeptWorld)
        );

        // Deleted (#566): looked for, then built with its chunks counted from the start.
        app.delete_kept_world(&course).unwrap();
        let built = steps(&mut app, &course);
        assert_eq!(built.first(), Some(&(LoadStage::KeptWorld, 0, 1)));
        let building = built.iter().find(|(stage, ..)| *stage == LoadStage::World);
        let Some(&(_, 0, chunks)) = building else {
            panic!("no building from the start: {built:?}");
        };
        assert!(
            chunks > 1,
            "the world's chunks, not a placeholder: {built:?}"
        );
        app.shutdown();
    }

    #[test]
    fn a_world_built_for_a_ride_is_kept_as_ridden_and_its_store_goes() {
        let dir = temp_dir("kept-store");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.set_keeps_worlds(true).unwrap();
        let course = prepare(&mut app, &dir);
        let keeper = app.keeper();
        let fingerprint = course::fingerprint(&course).unwrap();
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));
        keeper.kept.remove(&fingerprint).unwrap();

        // Built into a store while it is built (ADR 0040), and kept from it.
        ride(&mut app, &course);
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));
        let ridden = loaded(&app);
        assert!(ridden.chunks.len() > 1);
        assert_eq!(
            app.world().unwrap().chunks.len(),
            0,
            "the chunks stay on disk"
        );
        assert_eq!(read(&keeper, &fingerprint), (ridden.clone(), GENERATOR));
        // Publication precedes the background writer releasing its temporary store.
        wait_for(&mut app, |_| {
            worlds_left(&dir) == [format!("{fingerprint}.world")]
        });
        assert_eq!(worlds_left(&dir), [format!("{fingerprint}.world")]);
        assert_eq!(loaded(&app), ridden);
        app.shutdown();
    }

    #[test]
    fn a_damaged_kept_world_is_built_again() {
        let dir = temp_dir("kept-damaged");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.set_keeps_worlds(true).unwrap();
        let course = prepare(&mut app, &dir);
        let keeper = app.keeper();
        let fingerprint = course::fingerprint(&course).unwrap();
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));
        mark(&keeper, &fingerprint, GENERATOR);

        // No error to the rider: the world is built and kept again.
        let file = dir.join(format!("cache/worlds/{fingerprint}.world"));
        std::fs::write(&file, b"damaged").unwrap();
        assert!(!app.kept_world_outdated(&course), "nothing to offer");
        assert_ne!(ride(&mut app, &course), MARK);
        wait_for(&mut app, |_| {
            keeper.kept.generator(&fingerprint) == Some(GENERATOR)
        });
        app.shutdown();
    }

    /// The stores of worlds being ridden, in the kept worlds' directory under `dir`.
    fn stores(dir: &Path) -> Vec<String> {
        let mut names = worlds_left(dir);
        names.retain(|name| name.ends_with(".building"));
        names
    }

    #[test]
    fn every_world_is_ridden_from_a_store_that_goes_with_its_course() {
        let dir = temp_dir("kept-ridden");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        assert!(!app.keeps_worlds());
        // Built for a GPX route, before it is a course of the library.
        let course = prepare(&mut app, &dir);
        assert_eq!(stores(&dir).len(), 1, "built into a store (ADR 0040)");
        assert_eq!(app.world().unwrap().chunks.len(), 0, "no chunk held");
        let built = loaded(&app);
        assert!(built.chunks.len() > 1);
        assert_eq!(app.kept_worlds_size(), 0, "not kept");
        // Tidying the kept worlds leaves the world being ridden alone.
        app.tidy_kept_worlds();
        wait_for(&mut app, |_| true);
        app.set_keeps_worlds(false).unwrap();
        assert_eq!(loaded(&app), built);

        // Ridden again, it is built again into a store of its own, and the first goes.
        assert_ne!(ride(&mut app, &course), MARK);
        assert_eq!(loaded(&app), built, "the same world");
        wait_for(&mut app, |_| stores(&dir).len() == 1);
        // Closed, its store goes too.
        app.open_course(course);
        wait_for(&mut app, |_| stores(&dir).is_empty());
        app.shutdown();
    }

    #[test]
    fn a_world_the_cache_cannot_hold_is_ridden_from_memory() {
        let dir = temp_dir("kept-memory");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.set_keeps_worlds(true).unwrap();
        let course = prepare(&mut app, &dir);
        let built = loaded(&app);
        let keeper = app.keeper();
        let fingerprint = course::fingerprint(&course).unwrap();
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));
        // No store can be made, nor a world kept: something else is where they go.
        std::fs::remove_dir_all(dir.join("cache/worlds")).unwrap();
        std::fs::write(dir.join("cache/worlds"), b"in the way").unwrap();
        ride(&mut app, &course);
        assert_eq!(app.world().unwrap().chunks.len(), 0);
        assert_eq!(loaded(&app), built, "held in memory, as before ADR 0040");
        app.shutdown();
    }

    #[test]
    fn the_winter_ground_keeps_clear_of_every_chunks_ways_read_from_disk() {
        let dir = temp_dir("kept-snow");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let course = prepare(&mut app, &dir);
        let whole = orqwa_world::snow_ground_mask(&loaded(&app));
        assert!(whole.pixels.contains(&0), "the road is kept clear");
        // Made in the background, not on the caller's thread.
        wait_for(&mut app, |app| {
            app.snow_ground_mask() != WinterMask::Pending
        });
        assert_eq!(
            app.snow_ground_mask(),
            WinterMask::Made(&whole),
            "from the store"
        );
        app.set_keeps_worlds(true).unwrap();
        let keeper = app.keeper();
        let fingerprint = course::fingerprint(&course).unwrap();
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));
        ride(&mut app, &course);
        wait_for(&mut app, |app| {
            app.snow_ground_mask() != WinterMask::Pending
        });
        assert_eq!(
            app.snow_ground_mask(),
            WinterMask::Made(&whole),
            "from the kept world"
        );
        app.shutdown();
    }

    /// Tells `app` that `free` bytes of storage are free, as Android's does, until changed.
    fn free_storage(app: &mut App, free: u64) -> Arc<std::sync::atomic::AtomicU64> {
        let left = Arc::new(std::sync::atomic::AtomicU64::new(free));
        let told = Arc::clone(&left);
        app.check_free_storage(Arc::new(move |_| {
            Some(told.load(std::sync::atomic::Ordering::Relaxed))
        }));
        left
    }

    #[test]
    fn a_course_without_room_for_its_world_tells_the_rider_how_much_it_needs() {
        let dir = temp_dir("kept-room");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let course = prepare(&mut app, &dir);
        let left = free_storage(&mut app, 3_400_000);
        app.open_course(course);
        assert!(!app.build_world(false, crate::WHOLE_WORLD));
        let events = run_until(&mut app, |e| {
            matches!(e, AppEvent::WorldReady { .. } | AppEvent::Error(_))
        });
        let Some(AppEvent::Error(failure)) = events.last() else {
            panic!("built without room: {events:?}");
        };
        assert_eq!(failure.kind, Kind::WorldStorage);
        // 25 MB per kilometre, above the densest course measured (ADR 0040).
        let length = app.route().expect("a route").length().0;
        assert!(length > 0.0);
        let needed = (length / 1000.0 * 25.0).ceil();
        assert_eq!(failure.args, [format!("{needed} MB"), "3 MB".to_owned()]);
        assert!(app.world().is_none(), "nothing built");
        // The rider makes room and rides again.
        left.store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
        app.build_world(false, crate::WHOLE_WORLD);
        let events = run_until(&mut app, |e| {
            matches!(e, AppEvent::WorldReady { .. } | AppEvent::Error(_))
        });
        assert!(
            matches!(events.last(), Some(AppEvent::WorldReady { .. })),
            "{events:?}"
        );
        app.shutdown();
    }

    #[test]
    fn a_kept_world_is_ridden_without_room_for_another() {
        let dir = temp_dir("kept-no-room");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.set_keeps_worlds(true).unwrap();
        let course = prepare(&mut app, &dir);
        let keeper = app.keeper();
        let fingerprint = course::fingerprint(&course).unwrap();
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));
        free_storage(&mut app, 0);
        ride(&mut app, &course);
        assert!(app.world().is_some(), "read where it is kept");
        app.shutdown();
    }

    #[test]
    fn a_kept_world_without_room_to_build_it_again_stays_to_ride() {
        let dir = temp_dir("kept-regenerate-room");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.set_keeps_worlds(true).unwrap();
        let course = prepare(&mut app, &dir);
        let keeper = app.keeper();
        let fingerprint = course::fingerprint(&course).unwrap();
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));
        ride(&mut app, &course);
        free_storage(&mut app, 0);
        match app.regenerate_world() {
            Err(AppError::Reported(failure)) => assert_eq!(failure.kind, Kind::WorldStorage),
            other => panic!("not refused for room: {other:?}"),
        }
        assert!(keeper.kept.has(&fingerprint), "the kept world stays");
        ride(&mut app, &course);
        // Deleting the kept world gives its room back.
        let needed = store_size(app.route().expect("a route").length());
        let kept = keeper.kept.size_of(&fingerprint).unwrap();
        assert!(kept < needed);
        let file = keeper.kept.file(&fingerprint);
        app.check_free_storage(Arc::new(move |_| {
            Some(if file.is_file() {
                needed - kept
            } else {
                needed
            })
        }));
        assert!(
            app.regenerate_world()
                .is_ok_and(|regenerating| regenerating)
        );
        let events = run_until(&mut app, |e| {
            matches!(e, AppEvent::WorldReady { .. } | AppEvent::Error(_))
        });
        assert!(
            matches!(events.last(), Some(AppEvent::WorldReady { .. })),
            "{events:?}"
        );
        app.shutdown();
    }

    #[test]
    fn a_kept_world_that_cannot_be_read_needs_room_like_any_other() {
        let dir = temp_dir("kept-unreadable-room");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        app.set_keeps_worlds(true).unwrap();
        let course = prepare(&mut app, &dir);
        let keeper = app.keeper();
        let fingerprint = course::fingerprint(&course).unwrap();
        wait_for(&mut app, |_| keeper.kept.has(&fingerprint));
        // It would be built again, into a store.
        std::fs::write(keeper.kept.file(&fingerprint), b"not a world").unwrap();
        free_storage(&mut app, 0);
        app.open_course(course);
        app.build_world(false, crate::WHOLE_WORLD);
        let events = run_until(&mut app, |e| {
            matches!(e, AppEvent::WorldReady { .. } | AppEvent::Error(_))
        });
        assert!(
            matches!(events.last(), Some(AppEvent::Error(failure)) if failure.kind == Kind::WorldStorage),
            "{events:?}"
        );
        app.shutdown();
    }
}
