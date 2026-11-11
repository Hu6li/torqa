//! A ride left before its world is complete (ADR 0040, 2d step 4): a kept course's world is
//! built on in the background, also once another course is opened, and kept when complete
//! (R101). Opening the course again takes that build over, turned to where the ride enters;
//! building another world, starting a ride, or cancelling stops it, and the course's next
//! build takes up what it wrote (see [`crate::resuming`]).
//!
//! A build taken over keeps its pace's radius for the throttle; only where it turns to
//! follows the scene's radius now.

use std::sync::Arc;

use orqwa_domain::units::Meters;
use orqwa_storage::course;
use tracing::{info, warn};

use crate::{App, JobResult, LoadedWorld};

/// A kept course's world still being built after the rider opened another course.
pub(crate) struct Background {
    /// The load whose job builds it: the job's results carry it.
    pub(crate) load: u64,
    /// The course's fingerprint, which a course opened later shares if it is the same.
    fingerprint: String,
    job: tokio::task::AbortHandle,
    /// Where its store lands, to hold on to if it is stopped.
    slot: Option<crate::resuming::Slot>,
    pace: Arc<orqwa_world::Pace>,
    /// What was handed over when the world could first be ridden, once it was.
    pub(crate) loaded: Option<LoadedWorld>,
}

impl App {
    /// Lets the world being built go on in the background as the loaded course is left, if
    /// it is a course whose world is kept once built; otherwise the build stops as before.
    pub(crate) fn leave_world(&mut self) {
        let Some(pace) = self.pace.take() else {
            return;
        };
        // A kept world was found: nothing is being built.
        if self.chunks.as_ref().is_some_and(|chunks| chunks.complete()) {
            return;
        }
        // Only a library course's world is kept: any other would be tidied away at once.
        let fingerprint = self
            .course
            .as_deref()
            .filter(|_| self.keeps_worlds())
            .and_then(|course| course::fingerprint(course).ok())
            .filter(|fingerprint| {
                crate::kept::library_fingerprints(&self.courses_dir()).contains(fingerprint)
            });
        // Otherwise the job is left for the caller to stop. A job just finished has sent its
        // complete world, which is kept from here too.
        let Some(fingerprint) = fingerprint else {
            return;
        };
        let Some(job) = self.loading.take() else {
            return;
        };
        let loaded = self
            .world
            .take()
            .zip(self.chunks.take())
            .map(|(world, chunks)| LoadedWorld {
                world,
                chunks,
                generator: self.world_generator,
                keep_as: self.keep_as.take(),
            })
            // Taken over, and left again before the ride could enter it.
            .or_else(|| self.waiting_world.take().map(|waiting| *waiting));
        self.entering = None;
        info!("building the world left on in the background");
        // Nothing rides now: every core may build.
        pace.ride_at(None);
        self.stop_background();
        self.background = Some(Background {
            load: self.world_load,
            fingerprint,
            job,
            slot: self.builds.building.take(),
            pace,
            loaded,
        });
    }

    /// Stops the world built in the background; what it wrote is held on to for the course's
    /// next build.
    pub(crate) fn stop_background(&mut self) {
        if let Some(background) = self.background.take() {
            info!("stopping the world built in the background");
            self.stop_build(background.job, background.slot);
            if let Some(loaded) = background.loaded {
                self.drop_world(loaded);
            }
        }
    }

    /// Stops the world built in the background once nothing would keep it: keeping was
    /// turned off, or its course left the library.
    pub(crate) fn stop_background_unless_kept(&mut self) {
        let kept = self.background.as_ref().is_some_and(|background| {
            self.keeps_worlds()
                && crate::kept::library_fingerprints(&self.courses_dir())
                    .contains(&background.fingerprint)
        });
        if !kept {
            self.stop_background();
        }
    }

    /// The world being built for the loaded course failed: nothing more comes of it, so it is
    /// neither left to the background nor waited for; opening the course again builds afresh.
    pub(crate) fn world_build_failed(&mut self) {
        self.pace = None;
        self.builds.building = None;
        self.entering = None;
        if let Some(waiting) = self.waiting_world.take() {
            self.drop_world(waiting);
        }
    }

    /// Takes over the world built in the background if it is the loaded course's, turned to
    /// where the ride enters; any other is stopped, as another world is to be built. True if
    /// taken over.
    pub(crate) fn take_over_background(&mut self) -> bool {
        let Some(background) = self.background.take() else {
            return false;
        };
        let same = self
            .course
            .as_deref()
            .and_then(|course| course::fingerprint(course).ok())
            .is_some_and(|fingerprint| fingerprint == background.fingerprint);
        if !same {
            self.background = Some(background);
            self.stop_background();
            return false;
        }
        info!("taking over the world built in the background");
        let Background {
            load,
            job,
            slot,
            pace,
            loaded,
            ..
        } = background;
        self.world_load = load;
        self.loading = Some(job);
        self.builds.building = slot;
        self.pace = Some(pace);
        let entry = self.entry_distance();
        self.turn_build(entry);
        self.entering = Some(entry);
        if let Some(loaded) = loaded {
            self.enter_world(loaded);
        }
        true
    }

    /// Where the next ride enters the loaded course: its start, or where it is continued
    /// (R220).
    pub(crate) fn entry_distance(&self) -> Meters {
        Meters(
            self.continuation_point()
                .filter(|_| self.world_ahead.0)
                .map_or(0.0, |point| point.distance_m),
        )
    }

    /// The world `loaded` handed over by a build taken over: ready to ride once its chunks
    /// around where the ride enters are finished, which [`App::check_entering`] waits for.
    pub(crate) fn enter_world(&mut self, loaded: LoadedWorld) {
        self.waiting_world = Some(Box::new(loaded));
    }

    /// Hands the world waited for over to the ride once its surroundings are finished.
    pub(crate) fn check_entering(&mut self, events: &mut Vec<crate::AppEvent>) {
        let (Some(entry), Some(loaded)) = (self.entering, self.waiting_world.as_ref()) else {
            return;
        };
        if self.finished_around_in(&loaded.chunks, entry) {
            self.entering = None;
            if let Some(loaded) = self.waiting_world.take() {
                self.world_ready(*loaded, events);
            }
        }
    }

    /// Takes `result` of the world taken over while the ride waits to enter it.
    pub(crate) fn while_entering(&mut self, result: JobResult, events: &mut Vec<crate::AppEvent>) {
        match result {
            JobResult::Structures(_, bridges) => {
                if let Some(waiting) = &mut self.waiting_world {
                    Arc::make_mut(&mut waiting.world)
                        .structures
                        .append(*bridges);
                }
            }
            JobResult::Built(_, rest) => {
                // Complete, so finished around where the ride enters too.
                self.entering = None;
                if let Some(waiting) = self.waiting_world.take() {
                    self.world_ready(*waiting, events);
                }
                self.world_built(*rest);
            }
            _ => {}
        }
    }

    /// Whether `result` comes from the world built in the background.
    pub(crate) fn background_result(&self, result: &JobResult) -> bool {
        self.background.as_ref().is_some_and(|background| {
            matches!(result,
                JobResult::World(load, _)
                | JobResult::Structures(load, _)
                | JobResult::Built(load, _)
                | JobResult::Progress(load, ..)
                | JobResult::Route(load, _)
                    if *load == background.load)
        })
    }

    /// Takes `result` of the world built in the background.
    pub(crate) fn in_background(&mut self, result: JobResult) {
        let Some(background) = &mut self.background else {
            return;
        };
        match result {
            // A kept world was found after all: nothing more comes.
            JobResult::World(_, loaded) if loaded.chunks.complete() => {
                self.background = None;
                self.drop_world(loaded);
            }
            JobResult::World(_, loaded) => background.loaded = Some(*loaded),
            JobResult::Structures(_, bridges) => {
                if let Some(loaded) = &mut background.loaded {
                    Arc::make_mut(&mut loaded.world).structures.append(*bridges);
                }
            }
            JobResult::Built(_, rest) => self.background_built(*rest),
            JobResult::Route(_, Err(failure)) => {
                warn!(%failure, "the world built in the background failed");
                if let Some(loaded) = self.background.take().and_then(|b| b.loaded) {
                    self.drop_world(loaded);
                }
            }
            _ => {}
        }
    }

    /// The world built in the background is complete: kept, as R101 promises for a course
    /// first ridden.
    pub(crate) fn background_built(&mut self, rest: orqwa_world::Rest) {
        let Some(background) = self.background.take() else {
            return;
        };
        let Some(LoadedWorld {
            mut world,
            chunks,
            generator,
            ..
        }) = background.loaded
        else {
            warn!("the world built in the background was never handed over");
            return;
        };
        let parts = Arc::make_mut(&mut world);
        parts.structures = rest.structures;
        parts.fallback_samples = rest.fallback_samples;
        // Nothing rides it, so nothing asked for its winter ground's mask.
        let _ = chunks.completed();
        if self.keeps_worlds() {
            let keeper = self.keeper();
            let fingerprint = background.fingerprint;
            self.runtime.spawn_blocking(move || {
                keeper.keep_as(&fingerprint, &world, &chunks, generator);
            });
        } else {
            self.drop_world((world, chunks));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::AppEvent;
    use crate::tests::{run_until, temp_dir};

    /// A straight route `points` × 10 m long, climbing `grade` per cent, prepared as a course
    /// of `app`'s library from a GPX file in its own directory under `dir`.
    pub(crate) fn course(app: &mut App, dir: &Path, points: u32, grade: f64) -> PathBuf {
        let mut xml = String::from("<gpx><trk><name>Left early</name><trkseg>");
        for i in 0..=points {
            let lat = 46.0 + f64::from(i) * 10.0 / 111_195.0;
            let elevation = 500.0 + f64::from(i) * 10.0 * grade / 100.0;
            let _ = write!(
                xml,
                r#"<trkpt lat="{lat}" lon="7"><ele>{elevation}</ele></trkpt>"#
            );
        }
        xml.push_str("</trkseg></trk></gpx>");
        let gpx = dir.join(format!("{points}-{grade}"));
        std::fs::create_dir_all(&gpx).unwrap();
        let gpx = gpx.join("route.gpx");
        std::fs::write(&gpx, xml).unwrap();
        app.load_route(gpx, true);
        let events = run_until(app, |e| matches!(e, AppEvent::CourseAdded(_)));
        let Some(AppEvent::CourseAdded(course)) = events.last() else {
            unreachable!()
        };
        course.clone()
    }

    /// Opens `course` and waits for its route.
    pub(crate) fn open(app: &mut App, course: &Path) {
        app.open_course(course.to_owned());
        run_until(app, |e| matches!(e, AppEvent::RouteLoaded(_)));
    }

    /// Updates `app` for a while, or until `done` holds.
    pub(crate) fn wait_for(app: &mut App, mut done: impl FnMut(&App) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(120);
        while !done(app) {
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(16));
            app.update(Duration::from_millis(16));
        }
        true
    }

    pub(crate) fn kept(app: &App, course: &Path) -> bool {
        app.kept_worlds()
            .iter()
            .any(|world| world.courses.iter().any(|entry| entry.path == course))
    }

    #[test]
    fn a_kept_courses_world_left_unfinished_is_built_on_and_kept() {
        let dir = temp_dir("left-kept");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 500, 0.0);
        let other = course(&mut app, &dir, 40, 2.0);
        app.set_keeps_worlds(true).unwrap();

        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        // Left before its world is complete, for another course.
        open(&mut app, &other);

        assert!(
            wait_for(&mut app, |app| kept(app, &left)),
            "built on in the background and kept once complete (R101)"
        );
        app.shutdown();
    }

    #[test]
    fn with_keeping_off_a_world_left_unfinished_is_stopped() {
        let dir = temp_dir("left-off");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 3000, 0.0);
        let other = course(&mut app, &dir, 40, 2.0);
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        let build = app.loading.clone().expect("the build");

        open(&mut app, &other);
        assert!(app.background.is_none());
        // Far sooner than its 30 km could be built.
        let deadline = Instant::now() + Duration::from_secs(2);
        while !build.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(16));
            app.update(Duration::from_millis(16));
        }
        assert!(build.is_finished(), "nothing would keep what it builds");
        app.shutdown();
    }

    #[test]
    fn opening_the_course_again_takes_its_build_over() {
        let dir = temp_dir("left-again");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 500, 0.0);
        let other = course(&mut app, &dir, 40, 2.0);
        app.set_keeps_worlds(true).unwrap();
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        open(&mut app, &other);

        open(&mut app, &left);
        assert!(!kept(&app, &left), "the build is still under way");
        app.build_world(false, Meters(0.0));
        assert!(app.background.is_none());
        assert!(
            app.world_load < app.load,
            "the same build, not a second one"
        );
        let events = run_until(&mut app, |e| {
            matches!(e, AppEvent::WorldReady { .. } | AppEvent::Error(_))
        });
        assert!(matches!(events.last(), Some(AppEvent::WorldReady { .. })));
        assert!(app.world().is_some());
        assert!(wait_for(&mut app, |app| kept(app, &left)), "and kept");
        app.shutdown();
    }

    #[test]
    fn a_build_taken_over_and_left_again_is_still_kept() {
        let dir = temp_dir("left-twice");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 500, 0.0);
        let other = course(&mut app, &dir, 40, 2.0);
        app.set_keeps_worlds(true).unwrap();
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        open(&mut app, &other);
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));

        // Left again before the ride could enter it.
        open(&mut app, &other);
        assert!(app.background.is_some());
        assert!(wait_for(&mut app, |app| kept(app, &left)));
        app.shutdown();
    }

    /// `app` with keeping on, riding a 5 km library course it leaves at once for a short one:
    /// the first course, the second, and `app` with the first's build in the background.
    pub(crate) fn leaving(name: &str) -> (PathBuf, PathBuf, PathBuf, App) {
        let dir = temp_dir(name);
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 500, 0.0);
        let other = course(&mut app, &dir, 40, 2.0);
        app.set_keeps_worlds(true).unwrap();
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        open(&mut app, &other);
        assert!(app.background.is_some());
        (dir, left, other, app)
    }

    #[test]
    fn a_build_taken_over_that_fails_says_so_and_is_not_taken_over_again() {
        let (_dir, left, other, mut app) = leaving("left-failed");
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        assert!(app.world_load < app.load);

        // The build's job ends without a world, as one that panics does.
        app.loading.as_ref().unwrap().abort();
        let events = run_until(&mut app, |e| matches!(e, AppEvent::Error(_)));
        assert!(matches!(events.last(), Some(AppEvent::Error(_))));
        open(&mut app, &other);
        assert!(app.background.is_none(), "nothing is building it");
        app.shutdown();
    }

    #[test]
    fn cancelling_stops_the_build_rather_than_leaving_it() {
        let (_dir, left, _, mut app) = leaving("left-cancel");
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        let build = app.loading.clone().unwrap();

        app.cancel_loading();
        assert!(app.background.is_none());
        assert!(wait_for(&mut app, |_| build.is_finished()));
        app.shutdown();
    }

    #[test]
    fn a_build_nothing_would_keep_is_stopped() {
        // Its course deleted.
        let (_dir, left, _, mut app) = leaving("left-deleted");
        app.delete_course(&left).unwrap();
        assert!(app.background.is_none());
        app.shutdown();
        // Keeping turned off.
        let (_dir, _, _, mut app) = leaving("left-turned-off");
        app.set_keeps_worlds(false).unwrap();
        assert!(app.background.is_none());
        app.shutdown();
        // A course not in the library, such as a group's: its world would be tidied away.
        let (dir, left, other, mut app) = leaving("left-shared");
        let shared = dir.join("shared.oqc");
        std::fs::copy(&left, &shared).unwrap();
        app.delete_course(&left).unwrap();
        app.open_shared_course(shared);
        run_until(&mut app, |e| matches!(e, AppEvent::RouteLoaded(_)));
        app.build_world(false, Meters(0.0));
        open(&mut app, &other);
        assert!(app.background.is_none());
        app.shutdown();
    }

    #[test]
    fn a_group_course_with_the_same_route_and_data_takes_the_build_over() {
        let (dir, left, _, mut app) = leaving("left-group");
        let shared = dir.join("shared.oqc");
        std::fs::copy(&left, &shared).unwrap();

        app.open_shared_course(shared);
        run_until(&mut app, |e| matches!(e, AppEvent::RouteLoaded(_)));
        app.build_world(false, Meters(0.0));
        assert!(app.world_load < app.load, "the rider's own build of it");
        let events = run_until(&mut app, |e| {
            matches!(e, AppEvent::WorldReady { .. } | AppEvent::Error(_))
        });
        assert!(matches!(events.last(), Some(AppEvent::WorldReady { .. })));
        app.shutdown();
    }

    #[test]
    fn building_another_world_stops_the_one_left_in_the_background() {
        let dir = temp_dir("left-stopped");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 500, 0.0);
        let other = course(&mut app, &dir, 40, 2.0);
        app.set_keeps_worlds(true).unwrap();
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        open(&mut app, &other);
        assert!(app.background.is_some());

        app.build_world(false, Meters(0.0));
        assert!(
            app.background.is_none(),
            "stopped, so it shares no memory (R240)"
        );
        run_until(&mut app, |e| matches!(e, AppEvent::WorldReady { .. }));
        assert!(wait_for(&mut app, |app| kept(app, &other)));
        assert!(!kept(&app, &left), "its build was stopped");
        app.shutdown();
    }
}
