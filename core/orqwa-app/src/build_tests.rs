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
use crate::tests::{run_until, temp_dir};
use crate::{App, AppError, AppEvent, GhostChoice, JobResult, TrainerChoice};

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
