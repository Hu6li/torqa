//! A world build stopped before its world is complete keeps its store, and the course's next
//! build takes it up from there (ADR 0040, 2d step 4b), whether worlds are kept or not:
//! building is deterministic, so only the chunks not yet written are finished, and the world
//! comes out the same. The cost is preparing the route again.
//!
//! ORQWA holds on to one such store, the last stopped: another stopped replaces it, and it
//! goes with its course, when a world to be built needs its room (the course's own then built
//! afresh), or when ORQWA closes.
//! Only builds made offline, from the course's own data, leave one: a build that downloads
//! could find a tile the stopped one had to do without, and the two halves would not meet.

use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use orqwa_storage::course;
use orqwa_storage::worlds::Building;
use tracing::info;

use crate::App;
use crate::kept::Keeper;
use crate::world_chunks::WorldChunks;

/// The stores of world builds the app holds on to.
#[derive(Default)]
pub(crate) struct Builds {
    /// Where the store of the loaded course's world being built lands.
    pub(crate) building: Option<Slot>,
    /// The store of the last build stopped before its world was complete.
    stopped: Option<Stopped>,
}

/// What a world build starts from: where its store lands, and the store a stopped build of
/// the same world left, if one did.
pub(crate) struct Start {
    slot: Option<Slot>,
    stopped: Option<Stopped>,
}

impl Start {
    /// The chunks of the world to build, into a store named `name` if new, and which are
    /// in it already, by index: the store the stopped build left, or a new one, or none, with
    /// the chunks held in memory, if it cannot be made.
    pub(crate) async fn chunks(self, keeper: &Keeper, name: &str) -> (Arc<WorldChunks>, Vec<bool>) {
        let taken_up = match self.stopped {
            Some(stopped) => stopped.take_up().await,
            None => None,
        };
        let (store, written) = match taken_up {
            Some((store, written)) => (Some(store), written),
            None => (keeper.store(name).map(Arc::new), Vec::new()),
        };
        // A store that could not be taken up was noted in the slot already, so this build,
        // stopped, leaves nothing: the next one starts afresh.
        if let (Some(slot), Some(store)) = (self.slot, &store) {
            slot.set(store);
        }
        let chunks = WorldChunks::building(store);
        chunks.taken_up(&written);
        (Arc::new(chunks), written)
    }
}

/// Where the store of a world being built lands once made, so that it can be held on to if
/// the build is stopped. Holds no store itself: one let go of once its world is kept goes.
#[derive(Clone)]
pub(crate) struct Slot {
    /// The fingerprint of the course whose world is built.
    fingerprint: String,
    store: Arc<OnceLock<Weak<Building>>>,
    /// The jobs of stopped builds that wrote into the store before, when it is taken up.
    writers: Vec<tokio::task::AbortHandle>,
}

impl Slot {
    /// Notes `store` as the one the world is built into.
    fn set(&self, store: &Arc<Building>) {
        let _ = self.store.set(Arc::downgrade(store));
    }
}

/// The store of a build stopped before its world was complete.
pub(crate) struct Stopped {
    fingerprint: String,
    store: Arc<Building>,
    /// The jobs of the builds that wrote into it: until they have ended, one may still be
    /// writing a chunk.
    jobs: Vec<tokio::task::AbortHandle>,
}

impl Stopped {
    /// The store, and which chunks are in it by index, once the stopped build has ended.
    /// `None` if they cannot be told.
    async fn take_up(self) -> Option<(Arc<Building>, Vec<bool>)> {
        self.ended().await;
        let written = self.store.written().ok()?;
        info!(
            written = written.iter().filter(|&&w| w).count(),
            "taking up the world a stopped build left"
        );
        Some((self.store, written))
    }

    /// Once the builds that wrote into the store have ended: an aborted job lets go of it
    /// only when it next waits.
    async fn ended(&self) {
        while !self.jobs.iter().all(tokio::task::AbortHandle::is_finished) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl App {
    /// What the world about to be built for the loaded course starts from. Only an offline
    /// build of a course has its store held on to: a route not yet a course has no later build
    /// to find it.
    pub(crate) fn start_build(&mut self) -> Start {
        let mut slot = self
            .course
            .as_deref()
            .filter(|_| self.offline)
            .and_then(|course| course::fingerprint(course).ok())
            .map(|fingerprint| Slot {
                fingerprint,
                store: Arc::default(),
                writers: Vec::new(),
            });
        let stopped = self.take_stopped(slot.as_ref());
        // Noted at once: stopped again before it gets going, it is held on to as it was.
        if let (Some(slot), Some(stopped)) = (&mut slot, &stopped) {
            slot.set(&stopped.store);
            slot.writers.clone_from(&stopped.jobs);
        }
        self.builds.building.clone_from(&slot);
        Start { slot, stopped }
    }

    /// Stops the world build `job`, whose store lands in `slot`: unless its world is already
    /// built, or the store lacks a chunk, the store is held on to for the course's next build
    /// in place of one held before.
    pub(crate) fn stop_build(&mut self, job: tokio::task::AbortHandle, slot: Option<Slot>) {
        // A job that has ended built its world whole, or failed.
        let store = slot
            .filter(|_| !job.is_finished())
            .and_then(|slot| {
                let store = slot.store.get()?.upgrade()?;
                Some((slot.fingerprint, store, slot.writers))
            })
            .filter(|(_, store, _)| store.whole());
        job.abort();
        if let Some((fingerprint, store, mut jobs)) = store {
            info!("keeping what the stopped build wrote for the course's next build");
            jobs.retain(|job| !job.is_finished());
            jobs.push(job);
            if let Some(before) = self.builds.stopped.replace(Stopped {
                fingerprint,
                store,
                jobs,
            }) {
                self.drop_stopped(before);
            }
        }
    }

    /// The store a stopped build of the world `slot` is for left, if one did.
    fn take_stopped(&mut self, slot: Option<&Slot>) -> Option<Stopped> {
        let fingerprint = &slot?.fingerprint;
        if self.builds.stopped.as_ref()?.fingerprint == *fingerprint {
            self.builds.stopped.take()
        } else {
            None
        }
    }

    /// Lets go of the store a stopped build left, if one did, for the room it takes. True if
    /// it did.
    pub(crate) fn forget_any_stopped(&mut self) -> bool {
        let held = self.builds.stopped.take();
        held.map(|stopped| self.drop_stopped(stopped)).is_some()
    }

    /// Lets go of the store a stopped build of the course with `fingerprint` left: the course
    /// is gone.
    pub(crate) fn forget_stopped(&mut self, fingerprint: &str) {
        if self
            .builds
            .stopped
            .as_ref()
            .is_some_and(|stopped| stopped.fingerprint == fingerprint)
            && let Some(stopped) = self.builds.stopped.take()
        {
            self.drop_stopped(stopped);
        }
    }

    /// Lets go of `stopped` in the background, once the builds that wrote into it have ended,
    /// so that waiting for what is let go of waits for its store to be deleted.
    fn drop_stopped(&mut self, stopped: Stopped) {
        self.letting_go.retain(|deleting| !deleting.is_finished());
        let deleting = self.runtime.spawn(async move {
            stopped.ended().await;
            // Deleting a store takes a while.
            let _ = tokio::task::spawn_blocking(move || drop(stopped)).await;
        });
        self.letting_go.push(deleting);
    }
}

#[cfg(test)]
mod tests {
    use orqwa_domain::units::Meters;

    use super::*;
    use crate::AppEvent;
    use crate::leaving::tests::{course, kept, leaving, open, wait_for};
    use crate::tests::{run_until, temp_dir};

    /// The store held for the course's next build.
    fn held(app: &App) -> Arc<Building> {
        Arc::clone(&app.builds.stopped.as_ref().expect("a store held").store)
    }

    /// Waits until the world being built has some chunks written.
    fn some_written(app: &mut App) {
        assert!(wait_for(app, |app| app
            .chunks
            .as_ref()
            .is_some_and(|chunks| chunks.finished_count() > 0)));
    }

    #[test]
    fn a_build_stopped_with_keeping_off_is_taken_up_by_the_courses_next_build() {
        let dir = temp_dir("taken-up-off");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 1000, 0.0);
        let other = course(&mut app, &dir, 40, 2.0);
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        some_written(&mut app);

        // Stopped, as nothing would keep its world.
        open(&mut app, &other);
        assert!(app.background.is_none());
        let store = held(&app);

        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        assert!(app.builds.stopped.is_none(), "taken up");
        let events = run_until(&mut app, |e| {
            matches!(e, AppEvent::WorldReady { .. } | AppEvent::Error(_))
        });
        assert!(matches!(events.last(), Some(AppEvent::WorldReady { .. })));
        assert!(wait_for(&mut app, |app| app
            .chunks
            .as_ref()
            .is_some_and(|chunks| chunks.complete())));
        assert!(
            Arc::strong_count(&store) > 1,
            "built on into the same store"
        );
        app.shutdown();
    }

    #[test]
    fn a_kept_courses_build_stopped_in_the_background_is_taken_up_and_kept() {
        let (_dir, left, _, mut app) = leaving("taken-up-kept");
        assert!(wait_for(&mut app, |app| app
            .background
            .as_ref()
            .is_some_and(|background| background.loaded.is_some())));
        // Building another world stops it (R240).
        app.build_world(false, Meters(0.0));
        assert!(app.background.is_none());
        let _ = held(&app);
        run_until(&mut app, |e| matches!(e, AppEvent::WorldReady { .. }));

        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        assert!(app.builds.stopped.is_none(), "taken up");
        assert!(
            wait_for(&mut app, |app| kept(app, &left)),
            "kept once complete (R101)"
        );
        app.shutdown();
    }

    #[test]
    fn a_build_taking_up_a_store_stopped_before_it_gets_going_still_leaves_it() {
        let dir = temp_dir("taken-up-twice");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 1000, 0.0);
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        some_written(&mut app);
        app.cancel_loading();
        let store = held(&app);

        // Ride, and Cancel at once, before the build has taken the store up.
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        app.cancel_loading();
        assert!(
            Arc::ptr_eq(&held(&app), &store),
            "the same store, still held"
        );
        drop(store);

        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        assert!(app.builds.stopped.is_none(), "taken up");
        assert!(wait_for(&mut app, |app| app
            .chunks
            .as_ref()
            .is_some_and(|chunks| chunks.complete())));
        app.shutdown();
    }

    #[test]
    fn a_build_that_downloads_leaves_nothing_to_take_up() {
        let dir = temp_dir("taken-up-online");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 1000, 0.0);
        open(&mut app, &left);
        // As a video course's world is built.
        app.offline = false;
        app.build_world(false, Meters(0.0));
        some_written(&mut app);
        app.cancel_loading();
        assert!(app.builds.stopped.is_none());
        app.shutdown();
    }

    #[test]
    fn cancelling_a_build_keeps_what_it_wrote_for_the_next_try() {
        let dir = temp_dir("taken-up-cancel");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 1000, 0.0);
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        some_written(&mut app);

        app.cancel_loading();
        let _ = held(&app);
        app.shutdown();
    }

    #[test]
    fn what_a_stopped_build_left_gives_way_to_another_course_short_of_room() {
        let dir = temp_dir("taken-up-room");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 1000, 0.0);
        let other = course(&mut app, &dir, 40, 2.0);
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        some_written(&mut app);
        open(&mut app, &other);
        let _ = held(&app);
        // Room only once no store is left in the cache, as on a full Android device.
        let worlds = dir.join("cache").join(orqwa_storage::worlds::DIR);
        app.check_free_storage(Arc::new(move |_| {
            let stores = std::fs::read_dir(&worlds)
                .into_iter()
                .flatten()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().ends_with(".building"));
            Some(if stores { 0 } else { u64::MAX })
        }));

        app.build_world(false, Meters(0.0));
        let events = run_until(&mut app, |e| {
            matches!(e, AppEvent::WorldReady { .. } | AppEvent::Error(_))
        });
        assert!(
            matches!(events.last(), Some(AppEvent::WorldReady { .. })),
            "{events:?}"
        );
        assert!(app.builds.stopped.is_none());
        app.shutdown();
    }

    #[test]
    fn what_a_stopped_build_left_gives_way_to_its_own_course_short_of_room() {
        let dir = temp_dir("taken-up-room-same");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 1000, 0.0);
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        some_written(&mut app);
        app.cancel_loading();
        // Not held here: the store is deleted once nothing holds it.
        let store = Arc::downgrade(&held(&app));
        let worlds = dir.join("cache").join(orqwa_storage::worlds::DIR);
        app.check_free_storage(Arc::new(move |_| {
            let stores = std::fs::read_dir(&worlds)
                .into_iter()
                .flatten()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().ends_with(".building"));
            Some(if stores { 0 } else { u64::MAX })
        }));

        // The ride had room before it was cancelled: it is built afresh, not refused.
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        let events = run_until(&mut app, |e| {
            matches!(e, AppEvent::WorldReady { .. } | AppEvent::Error(_))
        });
        assert!(
            matches!(events.last(), Some(AppEvent::WorldReady { .. })),
            "{events:?}"
        );
        assert!(store.upgrade().is_none(), "not taken up but let go of");
        app.shutdown();
    }

    #[test]
    fn deleting_the_course_lets_go_of_what_its_stopped_build_wrote() {
        let dir = temp_dir("taken-up-deleted");
        let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
        let left = course(&mut app, &dir, 1000, 0.0);
        let other = course(&mut app, &dir, 40, 2.0);
        open(&mut app, &left);
        app.build_world(false, Meters(0.0));
        some_written(&mut app);
        open(&mut app, &other);
        let store = held(&app);

        app.delete_course(&left).unwrap();
        assert!(app.builds.stopped.is_none());
        assert!(
            wait_for(&mut app, |_| Arc::strong_count(&store) == 1),
            "nothing holds it but this test"
        );
        app.shutdown();
    }
}
