//! A world build stopped before its world is complete keeps its store, and the course's next
//! build takes it up from there (ADR 0040, 2d step 4b), whether worlds are kept or not:
//! building is deterministic, so only the chunks not yet written are finished, and the world
//! comes out the same. The cost is preparing the route again.
//!
//! ORQWA holds on to one such store, the last stopped: another stopped replaces it, and it
//! goes with its course, or when ORQWA closes.

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
    /// The stopped build's job: until it has ended, it may still be writing a chunk.
    job: tokio::task::AbortHandle,
}

impl Stopped {
    /// The store, and which chunks are in it by index, once the stopped build has ended.
    /// `None` if they cannot be told.
    async fn take_up(self) -> Option<(Arc<Building>, Vec<bool>)> {
        while !self.job.is_finished() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let written = self.store.written().ok()?;
        info!(
            written = written.iter().filter(|&&w| w).count(),
            "taking up the world a stopped build left"
        );
        Some((self.store, written))
    }
}

impl App {
    /// What the world about to be built for the loaded course starts from. Only a course's
    /// store is held on to: a route not yet one has no later build to find it.
    pub(crate) fn start_build(&mut self) -> Start {
        let slot = self
            .course
            .as_deref()
            .and_then(|course| course::fingerprint(course).ok())
            .map(|fingerprint| Slot {
                fingerprint,
                store: Arc::default(),
            });
        self.builds.building.clone_from(&slot);
        let stopped = self.take_stopped(slot.as_ref());
        Start { slot, stopped }
    }

    /// Stops the world build `job`, whose store lands in `slot`: unless its world is already
    /// built, or the store lacks a chunk, the store is held on to for the course's next build
    /// in place of one held before.
    pub(crate) fn stop_build(&mut self, job: tokio::task::AbortHandle, slot: Option<Slot>) {
        // A job that has ended built its world whole, or failed.
        let store = slot
            .filter(|_| !job.is_finished())
            .and_then(|slot| Some((slot.fingerprint, slot.store.get()?.upgrade()?)))
            .filter(|(_, store)| store.whole());
        job.abort();
        if let Some((fingerprint, store)) = store {
            info!("keeping what the stopped build wrote for the course's next build");
            if let Some(before) = self.builds.stopped.replace(Stopped {
                fingerprint,
                store,
                job,
            }) {
                // Deleting a store takes a while.
                self.drop_world(before.store);
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
            self.drop_world(stopped.store);
        }
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
