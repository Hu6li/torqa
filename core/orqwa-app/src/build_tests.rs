//! Storage errors and cancellation must preserve a tablet's memory and the rider's work.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use orqwa_devices::fake::FakeRider;
use orqwa_domain::units::{Meters, Percent, Rpm, Watts};
use orqwa_physics::DescentMode;
use orqwa_session::workout::Workout;

use crate::failure::{Failure, Kind};
use crate::leaving::tests::{course, open, wait_for};
use crate::memory::MemoryInfo;
use crate::tests::{run_until, temp_dir};
use crate::{App, AppError, AppEvent, GhostChoice, JobResult, TrainerChoice};

const MB: u64 = 1_000_000;

fn memory(available: u64) -> MemoryInfo {
    MemoryInfo {
        available,
        total: 8_000 * MB,
        threshold: 200 * MB,
        low: false,
    }
}

#[test]
fn memory_admission_refuses_before_building_and_reopening_rechecks_current_free_memory() {
    use std::sync::atomic::AtomicU64;
    let dir = temp_dir("memory-admission");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    let path = course(&mut app, &dir, 40, 0.0);
    open(&mut app, &path);
    let free = Arc::new(AtomicU64::new(900 * MB));
    let reading = Arc::clone(&free);
    app.check_free_memory(Arc::new(move || {
        Some(memory(reading.load(Ordering::SeqCst)))
    }));
    app.build_world(false, Meters(1000.0));
    let events = run_until(&mut app, |event| matches!(event, AppEvent::Error(_)));
    assert!(events.iter().any(|event| matches!(event,
        AppEvent::Error(failure) if failure.kind == Kind::WorldMemory)));
    assert!(!events.iter().any(|event| matches!(
        event,
        AppEvent::WorldReady { .. }
            | AppEvent::LoadProgress {
                stage: crate::LoadStage::World,
                ..
            }
    )));
    assert!(app.world().is_none());
    free.store(7_000 * MB, Ordering::SeqCst);
    open(&mut app, &path);
    app.build_world(false, Meters(1000.0));
    run_until(&mut app, |event| {
        matches!(event, AppEvent::WorldReady { .. })
    });
    app.shutdown();
}

#[test]
fn memory_is_read_after_cancelled_workers_release_their_buffers() {
    let dir = temp_dir("memory-after-cancel");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    let path = course(&mut app, &dir, 40, 0.0);
    open(&mut app, &path);
    let mut old = held(&app);
    let released = Arc::clone(&old.released);
    app.stop_build(old.job.clone(), None);
    app.check_free_memory(Arc::new(move || {
        assert!(
            released.load(Ordering::SeqCst),
            "do not reject based on memory about to be freed"
        );
        Some(memory(7_000 * MB))
    }));
    app.build_world(false, Meters(1000.0));
    old.release();
    run_until(&mut app, |event| {
        matches!(event, AppEvent::WorldReady { .. })
    });
    app.shutdown();
}

#[test]
fn whole_course_generation_uses_the_selected_view_distance_for_memory_admission() {
    let dir = temp_dir("memory-render-radius");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    let path = course(&mut app, &dir, 40, 0.0);
    open(&mut app, &path);
    let crate::WorldPlan::OnRequest(Some(map)) = &app.world_plan else {
        panic!("course map");
    };
    let cores = std::thread::available_parallelism().unwrap().get();
    let low = crate::memory::estimate(app.route().unwrap(), map, Meters(3925.0), cores);
    let high = crate::memory::estimate(app.route().unwrap(), map, Meters(6500.0), cores);
    let between = 800 * MB + u64::midpoint(low.build, high.build);
    app.check_free_memory(Arc::new(move || Some(memory(between))));
    app.set_world_upload_radius(Meters(6500.0));
    app.build_world(false, crate::WHOLE_WORLD);
    let events = run_until(&mut app, |event| matches!(event, AppEvent::Error(_)));
    assert!(events.iter().any(|event| matches!(event,
        AppEvent::Error(failure) if failure.kind == Kind::WorldMemory)));
    open(&mut app, &path);
    app.set_world_upload_radius(Meters(3925.0));
    app.build_world(false, crate::WHOLE_WORLD);
    run_until(&mut app, |event| {
        matches!(event, AppEvent::WorldReady { .. })
    });
    app.shutdown();
}

#[test]
fn a_kept_world_checks_rendering_memory_without_charging_for_another_build() {
    let dir = temp_dir("memory-kept-world");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    let path = course(&mut app, &dir, 40, 0.0);
    app.set_keeps_worlds(true).unwrap();
    assert!(wait_for(&mut app, |app| crate::leaving::tests::kept(
        app, &path
    )));
    open(&mut app, &path);
    let crate::WorldPlan::OnRequest(Some(map)) = &app.world_plan else {
        panic!("loaded course map");
    };
    let estimate = crate::memory::estimate(app.route().unwrap(), map, Meters(1000.0), 8);
    let between = u64::midpoint(estimate.ride, estimate.build);
    app.check_free_memory(Arc::new(move || Some(memory(800 * MB + between))));
    app.build_world(false, Meters(1000.0));
    let events = run_until(&mut app, |event| {
        matches!(event, AppEvent::WorldReady { .. })
    });
    assert!(!events.iter().any(|event| matches!(
        event,
        AppEvent::LoadProgress {
            stage: crate::LoadStage::World,
            ..
        }
    )));
    open(&mut app, &path);
    app.check_free_memory(Arc::new(move || Some(memory(800 * MB + estimate.ride - 1))));
    app.build_world(false, Meters(1000.0));
    let events = run_until(&mut app, |event| matches!(event, AppEvent::Error(_)));
    assert!(events.iter().any(|event| matches!(event,
        AppEvent::Error(failure) if failure.kind == Kind::WorldMemory)));
    assert!(
        app.world().is_none(),
        "kept meshes do not bypass rendering admission"
    );
    app.shutdown();
}

#[test]
fn memory_falling_during_generation_stops_before_world_ready() {
    use std::sync::atomic::AtomicUsize;
    let dir = temp_dir("memory-during-build");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    let path = course(&mut app, &dir, 40, 0.0);
    open(&mut app, &path);
    let reads = Arc::new(AtomicUsize::new(0));
    let reading = Arc::clone(&reads);
    app.check_free_memory(Arc::new(move || {
        let read = reading.fetch_add(1, Ordering::SeqCst);
        // Initial snapshot and admission succeed; memory runs short during the horizon.
        // The fresh checkpoint before chunk preparation must catch this without timing.
        Some(memory(if read < 2 { 7_000 * MB } else { 100 * MB }))
    }));
    app.build_world(false, Meters(1000.0));
    let events = run_until(&mut app, |event| matches!(event, AppEvent::Error(_)));
    assert!(events.iter().any(|event| matches!(event,
        AppEvent::Error(failure) if failure.kind == Kind::WorldMemory)));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AppEvent::WorldReady { .. }))
    );
    assert!(app.world_failed());
    app.shutdown();
}

#[test]
fn memory_pressure_during_a_ride_preserves_samples_and_blocks_further_uploads() {
    let dir = temp_dir("memory-during-ride");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    course(&mut app, &dir, 40, 0.0);
    app.connect_trainer(TrainerChoice::Fake(FakeRider {
        power: Watts(150.0),
        cadence: Rpm(90.0),
        heart: None,
    }))
    .unwrap();
    app.start_ride(Percent(50.0), DescentMode::Coast, &GhostChoice::None)
        .unwrap();
    assert!(wait_for(&mut app, |app| !app
        .ride
        .as_ref()
        .unwrap()
        .ride
        .samples()
        .is_empty()));
    let samples = app.ride.as_ref().unwrap().ride.samples().len();
    app.check_free_memory(Arc::new(|| Some(memory(100 * MB))));
    assert!(!app.check_world_memory());
    assert!(app.is_paused());
    assert!(
        !app.check_world_memory(),
        "no more chunks read or uploaded after failure"
    );
    // An aborted job or already queued progress cannot replace the useful memory error.
    app.jobs_tx
        .send(JobResult::Route(
            app.world_load,
            Err(Failure::new(Kind::PreparationStopped, "cancelled")),
        ))
        .unwrap();
    let events = app.update(Duration::from_millis(16));
    let failures: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AppEvent::Error(failure) => Some(failure.kind),
            _ => None,
        })
        .collect();
    assert_eq!(failures, [Kind::WorldMemory]);
    assert_eq!(app.ride.as_ref().unwrap().ride.samples().len(), samples);
    assert!(
        app.set_paused(false),
        "scene waits cannot automatically resume it"
    );
    assert!(
        app.save_ride(false)
            .iter()
            .any(|event| matches!(event, AppEvent::RideSaved(_)))
    );
    app.shutdown();
}

struct Buffers {
    released: Arc<AtomicBool>,
    _bytes: Vec<u8>,
}

impl Drop for Buffers {
    fn drop(&mut self) {
        self.released.store(true, Ordering::SeqCst);
    }
}

struct Held {
    release: Option<mpsc::Sender<()>>,
    released: Arc<AtomicBool>,
    job: tokio::task::AbortHandle,
}

impl Held {
    fn release(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.release();
    }
}

/// Holds a job inside synchronous work, after its buffers exist and before abort can drop
/// them. No timing or route complexity decides when cancellation takes effect.
fn held(app: &App) -> Held {
    let (started, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let released = Arc::new(AtomicBool::new(false));
    let buffers = Buffers {
        released: Arc::clone(&released),
        _bytes: vec![0; 4096],
    };
    let job = app.runtime.spawn(async move {
        let _buffers = buffers;
        started.send(()).unwrap();
        let _ = blocked.recv();
        std::future::pending::<()>().await;
    });
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    Held {
        release: Some(release),
        released,
        job: job.abort_handle(),
    }
}

#[test]
fn a_new_build_waits_for_all_stopped_buffers_even_after_cancel_and_retry() {
    let dir = temp_dir("builds-serialized");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    let path = course(&mut app, &dir, 40, 0.0);
    open(&mut app, &path);
    let mut old = held(&app);
    app.stop_build(old.job.clone(), None);
    for _ in 0..3 {
        app.build_world(false, Meters(0.0));
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(5));
            let events = app.update(Duration::from_millis(5));
            assert!(!events.iter().any(|event| matches!(
                event,
                AppEvent::WorldReady { .. } | AppEvent::LoadProgress { .. }
            )));
        }
        assert!(!old.released.load(Ordering::SeqCst));
        app.cancel_loading();
        open(&mut app, &path);
    }
    app.build_world(false, Meters(0.0));
    old.release();
    run_until(&mut app, |event| {
        matches!(event, AppEvent::WorldReady { .. })
    });
    assert!(old.released.load(Ordering::SeqCst));
    app.shutdown();
}

#[test]
fn a_ride_or_workout_waits_for_stopped_generation_without_blocking_the_frame() {
    let dir = temp_dir("ride-waits-for-build");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    course(&mut app, &dir, 40, 0.0);
    app.connect_trainer(TrainerChoice::Fake(FakeRider {
        power: Watts(150.0),
        cadence: Rpm(90.0),
        heart: None,
    }))
    .unwrap();
    let mut old = held(&app);
    app.stop_build(old.job.clone(), None);
    assert!(!app.prepare_ride());
    assert!(matches!(
        app.start_ride(Percent(50.0), DescentMode::Coast, &GhostChoice::None),
        Err(AppError::CourseNotReady)
    ));
    assert!(matches!(
        app.start_workout(
            Workout::ConstantPower(Watts(150.0)),
            false,
            DescentMode::Coast,
            "test"
        ),
        Err(AppError::CourseNotReady)
    ));
    assert!(app.ride_state().is_none());
    old.release();
    assert!(wait_for(&mut app, |app| app
        .builds
        .stopping
        .iter()
        .all(tokio::task::AbortHandle::is_finished)));
    assert!(app.prepare_ride());
    app.start_workout(
        Workout::ConstantPower(Watts(150.0)),
        false,
        DescentMode::Coast,
        "test",
    )
    .unwrap();
    app.shutdown();
}

#[test]
fn a_required_store_creation_failure_reports_an_error_and_can_be_retried() {
    let dir = temp_dir("required-store-create");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    let path = course(&mut app, &dir, 40, 0.0);
    open(&mut app, &path);
    app.require_world_store();
    app.check_free_storage(Arc::new(|_| None));
    let worlds = dir.join("cache/worlds");
    std::fs::rename(&worlds, dir.join("old-worlds")).unwrap();
    std::fs::write(&worlds, b"not a directory").unwrap();
    app.build_world(false, Meters(0.0));
    let events = run_until(&mut app, |event| matches!(event, AppEvent::Error(_)));
    assert!(events.iter().any(
        |event| matches!(event, AppEvent::Error(failure) if failure.kind == Kind::WriteWorld)
    ));
    assert!(app.world().is_none());
    std::fs::remove_file(&worlds).unwrap();
    open(&mut app, &path);
    app.build_world(false, Meters(0.0));
    run_until(&mut app, |event| {
        matches!(event, AppEvent::WorldReady { .. })
    });
    app.shutdown();
}

#[test]
fn a_world_failure_pauses_the_local_ride_preserving_its_recording_and_prevents_auto_resume() {
    let dir = temp_dir("riding-store-failure");
    let mut app = App::new(dir.join("data"), dir.join("cache")).unwrap();
    course(&mut app, &dir, 40, 0.0);
    app.connect_trainer(TrainerChoice::Fake(FakeRider {
        power: Watts(150.0),
        cadence: Rpm(90.0),
        heart: None,
    }))
    .unwrap();
    app.start_ride(Percent(50.0), DescentMode::Coast, &GhostChoice::None)
        .unwrap();
    assert!(wait_for(&mut app, |app| app
        .ride
        .as_ref()
        .is_some_and(|ride| !ride.ride.samples().is_empty())));
    let samples = app.ride.as_ref().unwrap().ride.samples().len();
    app.jobs_tx
        .send(JobResult::Route(
            app.world_load,
            Err(Failure::new(Kind::WriteWorld, "disk full")),
        ))
        .unwrap();
    let events = app.update(Duration::from_millis(16));
    assert!(events.iter().any(
        |event| matches!(event, AppEvent::Error(failure) if failure.kind == Kind::WriteWorld)
    ));
    assert!(app.is_paused());
    assert!(
        app.set_paused(false),
        "an automatic scene wait cannot resume a failed world"
    );
    assert_eq!(app.ride.as_ref().unwrap().ride.samples().len(), samples);
    assert!(
        app.save_ride(false)
            .iter()
            .any(|event| matches!(event, AppEvent::RideSaved(_)))
    );
    app.shutdown();
}
