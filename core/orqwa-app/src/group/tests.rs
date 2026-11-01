use super::*;
use crate::{App, TrainerChoice};
use orqwa_devices::fake::FakeRider;
use orqwa_domain::units::{Rpm, Watts};
use std::time::Duration;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let token = orqwa_net::code::RideId::generate().unwrap();
        let path = std::env::temp_dir().join(format!("orqwa-group-test-{:x?}", token.0));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn app(&self) -> App {
        App::new(self.0.join("data"), self.0.join("cache")).unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn options() -> ConnectOptions {
    let mut options = ConnectOptions::default();
    options.gather.policy.loopback = true;
    options.gather.stun_servers.clear();
    options
}

#[test]
fn independent_failures_keep_retry_state_and_each_occurrence_is_reported_once() {
    let directory = Directory::new();
    let mut app = directory.app();
    let idle_directory = Directory::new();
    let mut idle = idle_directory.app();
    app.group_open(None, options()).unwrap();
    wait(&mut app, &mut idle, |host, _| {
        !host.group().invite.is_empty()
    });
    let details = "publishing verified activity: Permission denied (os error 13)";
    for _ in 0..2 {
        app.group_command(Command::Failure(details.into())).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut received = None;
        while received.is_none() && Instant::now() < deadline {
            for event in app.update(Duration::from_millis(16)) {
                if let AppEvent::Error(failure) = event {
                    received = Some(failure);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let failure = received.expect("each new occurrence reaches the standard error history");
        assert_eq!(failure.kind, crate::Kind::GroupActivity);
        assert_eq!(failure.details, details);
        for _ in 0..10 {
            assert!(
                !app.update(Duration::from_millis(16))
                    .iter()
                    .any(|event| matches!(event, AppEvent::Error(_)))
            );
        }
    }
    app.group_leave();
    let mut snapshot = Snapshot::default();
    snapshot.activity_error(details);
    assert!(snapshot.can_retry());
    snapshot.report(
        crate::Kind::GroupVoice,
        "opening speakers: AudioManager unavailable",
    );
    assert!(
        snapshot.can_retry(),
        "a voice failure must not hide an activity retry"
    );
    assert_eq!(snapshot.failure_revision, 2);
}
#[track_caller]
fn wait(host: &mut App, peer: &mut App, condition: impl Fn(&App, &App) -> bool) {
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        host.update(Duration::from_millis(16));
        peer.update(Duration::from_millis(16));
        if condition(host, peer) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!(
        "group timed out: host {} {} handover {}, peer {} {} handover {}",
        host.group().phase.key(),
        host.group().error,
        host.group().handover.key(),
        peer.group().phase.key(),
        peer.group().error,
        peer.group().handover.key()
    );
}

#[test]
fn late_workout_entry_is_explicit_and_records_only_the_riders_own_effort() {
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut peer = to.app();
    for app in [&mut host, &mut peer] {
        app.connect_trainer(TrainerChoice::Fake(FakeRider {
            power: Watts(180.0),
            cadence: Rpm(85.0),
            heart: None,
        }))
        .unwrap();
    }
    peer.profile.profile.ftp = Watts(300.0);
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut peer, |host, _| {
        !host.group().invite.is_empty()
    });
    host.group_select(None, Some("builtin:recovery-30"))
        .unwrap();
    wait(&mut host, &mut peer, |host, _| {
        host.group().prepared && host.trainer_connected()
    });
    host.group_ready(true).unwrap();
    wait(&mut host, &mut peer, |host, _| host.group().can_start());
    host.group_start().unwrap();
    wait(&mut host, &mut peer, |host, _| host.ride_state().is_some());
    // The organizer advances the shared plan without manufacturing earlier local samples.
    wait(&mut host, &mut peer, |host, _| {
        host.group().can_control_workout()
    });
    assert!(host.skip_step());
    wait(&mut host, &mut peer, |host, _| {
        host.ride_state()
            .unwrap()
            .workout
            .unwrap()
            .progress
            .unwrap()
            .step
            > 0
    });
    peer.group_open(Some(host.group().invite), options())
        .unwrap();
    wait(&mut host, &mut peer, |_, peer| {
        !peer.group().reply.is_empty()
    });
    host.group_admit(peer.group().reply).unwrap();
    wait(&mut host, &mut peer, |_, peer| {
        peer.group().can_enter() && peer.trainer_connected()
    });
    assert!(
        peer.ride_state().is_none(),
        "preparing must not automatically enter"
    );
    let host_before = host.ride_state().unwrap().elapsed;
    for _ in 0..20 {
        host.update(Duration::from_millis(16));
        peer.update(Duration::from_millis(16));
    }
    assert!(
        host.ride_state().unwrap().elapsed > host_before,
        "late preparation never holds the group"
    );
    peer.group_enter().unwrap();
    wait(&mut host, &mut peer, |_, peer| peer.ride_state().is_some());
    let state = peer.ride_state().unwrap();
    assert!(state.elapsed < Duration::from_secs(1));
    assert!(state.ridden.0 < 10.0);
    let progress = state.workout.unwrap().progress.unwrap();
    assert!(
        progress.step > 0,
        "entry follows the current organizer step"
    );
    let plan = peer.group().plan.unwrap();
    assert_eq!(
        peer.ride_state().unwrap().workout.unwrap().target_power,
        plan.at(progress.elapsed, Watts(300.0)).unwrap().power
    );
    assert!(peer.group_enter().is_err(), "cannot enter a second time");
    let samples = peer.ride.as_ref().unwrap().ride.samples();
    assert!(samples.len() < 3);
    if let Some(sample) = samples.first() {
        assert_eq!(sample.elapsed, Duration::ZERO);
        assert_eq!(sample.distance, Meters(0.0));
    }
    host.group_leave();
    wait(&mut host, &mut peer, |_, peer| {
        peer.group().phase == Phase::Ended
    });
    assert!(
        peer.group().roster[usize::from(peer.group().organizer)].is_none(),
        "disconnected organizer is removed from the live roster"
    );
    let elapsed = peer.ride_state().unwrap().elapsed;
    peer.update(Duration::from_secs(1));
    assert!(
        peer.ride_state().unwrap().elapsed > elapsed,
        "host loss preserves local riding"
    );
}

fn start_shared_workout(host: &mut App, peer: &mut App) {
    for app in [&mut *host, &mut *peer] {
        app.connect_trainer(TrainerChoice::Fake(FakeRider {
            power: Watts(180.0),
            cadence: Rpm(85.0),
            heart: None,
        }))
        .unwrap();
    }
    host.group_open(None, options()).unwrap();
    wait(host, peer, |host, _| !host.group().invite.is_empty());
    peer.group_open(Some(host.group().invite), options())
        .unwrap();
    wait(host, peer, |_, peer| !peer.group().reply.is_empty());
    host.group_admit(peer.group().reply).unwrap();
    wait(host, peer, |host, peer| {
        host.group().roster.iter().flatten().count() == 2 && peer.group().phase == Phase::Waiting
    });
    host.group_select(None, Some("builtin:recovery-30"))
        .unwrap();
    wait(host, peer, |host, peer| {
        host.group().prepared
            && peer.group().prepared
            && host.trainer_connected()
            && peer.trainer_connected()
    });
    host.group_ready(true).unwrap();
    peer.group_ready(true).unwrap();
    wait(host, peer, |host, _| host.group().can_start());
    host.group_start().unwrap();
    wait(host, peer, |host, peer| {
        host.ride_state().is_some()
            && peer.ride_state().is_some()
            && host.group().can_control_workout()
            && peer.group().workout_clock.is_some()
    });
}

#[test]
#[allow(clippy::too_many_lines)] // consent, rollback, two authority changes and late entry share one real group lifecycle
fn planned_handover_requires_consent_and_preserves_riding_and_workout_authority() {
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut peer = to.app();
    start_shared_workout(&mut host, &mut peer);
    let root = host.group().root;
    let peer_id = peer.group().me;
    let generations = host.group().generations;
    let invitation = host.group().invite;
    assert_eq!(
        peer.step_intensity(4),
        Some(orqwa_domain::units::Percent(120.0))
    );
    host.group_handover(peer_id).unwrap();
    wait(&mut host, &mut peer, |_, p| {
        p.group().handover == Handover::Offered
    });
    assert!(host.group().host && !peer.group().host);
    peer.group_handover_answer(false).unwrap();
    wait(&mut host, &mut peer, |h, p| {
        h.group().handover == Handover::Idle && p.group().handover == Handover::Idle
    });
    assert_eq!(host.group().epoch, 1);
    host.group_handover(peer_id).unwrap();
    wait(&mut host, &mut peer, |_, p| {
        p.group().handover == Handover::Offered
    });
    host.group_handover_cancel().unwrap();
    wait(&mut host, &mut peer, |h, p| {
        h.group().handover == Handover::Idle && p.group().handover == Handover::Idle
    });
    host.group_workout_pause(true).unwrap();
    // The ride follows a received clock on a later update; compare from where it holds.
    wait(&mut host, &mut peer, |_, p| {
        p.group().workout_clock.is_some_and(|c| {
            c.paused && p.ride.as_ref().unwrap().ride.workout_elapsed() == Some(c.elapsed)
        })
    });
    let elapsed = peer.ride_state().unwrap().elapsed;
    let distance = peer.ride_state().unwrap().distance;
    let workout = peer.ride.as_ref().unwrap().ride.workout_elapsed();
    assert!(peer.set_paused(true));
    host.group_handover(peer_id).unwrap();
    wait(&mut host, &mut peer, |_, p| {
        p.group().handover == Handover::Offered
    });
    peer.group_handover_answer(true).unwrap();
    wait(&mut host, &mut peer, |h, p| {
        !h.group().host && p.group().host
    });
    for app in [&host, &peer] {
        let group = app.group();
        assert_eq!(group.root, root);
        assert_eq!(group.epoch, 4);
        assert_eq!(group.organizer, peer_id);
        assert_eq!(group.generations, generations);
        assert!(group.workout_clock.unwrap().paused);
    }
    assert_ne!(peer.group().invite, invitation);
    assert_eq!(host.group().me, 0);
    assert_eq!(peer.group().me, peer_id);
    assert!(peer.ride_state().unwrap().elapsed >= elapsed);
    assert!(peer.ride_state().unwrap().distance.0 >= distance.0);
    assert_eq!(peer.ride.as_ref().unwrap().ride.workout_elapsed(), workout);
    assert!(
        peer.is_paused(),
        "handover preserves a personal safety pause"
    );
    peer.set_paused(false);
    assert!(!peer.is_paused());
    assert_eq!(
        peer.step_intensity(0),
        Some(orqwa_domain::units::Percent(120.0))
    );
    assert!(host.group_workout_pause(false).is_err());
    peer.group_workout_pause(false).unwrap();
    wait(&mut host, &mut peer, |h, _| {
        h.group().workout_clock.is_some_and(|c| !c.paused)
    });
    assert!(peer.skip_step());
    assert!(!host.skip_step());
    let late_directory = Directory::new();
    let mut late = late_directory.app();
    late.connect_trainer(TrainerChoice::Fake(FakeRider {
        power: Watts(150.0),
        cadence: Rpm(80.0),
        heart: None,
    }))
    .unwrap();
    late.group_open(Some(peer.group().invite), options())
        .unwrap();
    wait(&mut peer, &mut late, |_, p| !p.group().reply.is_empty());
    peer.group_admit(late.group().reply).unwrap();
    wait(&mut peer, &mut late, |_, p| {
        p.group().can_enter() && p.trainer_connected()
    });
    assert_eq!(late.group().root, root);
    assert_eq!(late.group().epoch, 4);
    assert_eq!(late.group().organizer, peer_id);
    late.group_enter().unwrap();
    wait(&mut peer, &mut late, |_, p| p.ride_state().is_some());
    // A later handover can return authority to original rider zero without changing identity.
    peer.group_handover(0).unwrap();
    wait(&mut host, &mut peer, |h, _| {
        h.group().handover == Handover::Offered
    });
    host.group_handover_answer(true).unwrap();
    wait(&mut host, &mut peer, |h, p| {
        h.group().host && !p.group().host && late.group().organizer == 0
    });
    assert_eq!(host.group().epoch, 5);
    assert_eq!(peer.group().organizer, 0);
    assert_eq!(host.group().root, root);
    late.update(Duration::from_millis(16));
    assert!(late.ride_state().is_some());
    assert_eq!(late.group().epoch, 5);
    // Losing the new host preserves both the old host's local recording and independent timing.
    host.group_leave();
    wait(&mut host, &mut peer, |_, p| p.group().phase == Phase::Ended);
    assert!(peer.ride_state().is_some());
}

#[test]
fn assistance_transitions_keep_force_continuous_and_bounded() {
    use orqwa_domain::units::GradePercent;
    use orqwa_physics::GroupEffect;
    let old = GroupEffect::new(0.7, GradePercent(-2.0));
    let new = GroupEffect::new(1.0, GradePercent(2.0));
    assert_eq!(blend(old, new, Duration::ZERO), old);
    let middle = blend(old, new, Duration::from_secs(1));
    assert!((middle.drag_share() - 0.85).abs() < 1e-9);
    assert!(middle.grade_offset().0.abs() < 1e-9);
    assert_eq!(blend(old, new, Duration::from_secs(2)), new);
}

#[test]
#[allow(clippy::too_many_lines)] // one two-rider lifecycle checks shared commands, local safety, personal targets and host loss together
fn organizer_workout_controls_keep_personal_intensity_and_local_safety_pause_independent() {
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut peer = to.app();
    host.profile.profile.ftp = Watts(200.0);
    peer.profile.profile.ftp = Watts(300.0);
    start_shared_workout(&mut host, &mut peer);
    assert_eq!(
        peer.step_intensity(4),
        Some(orqwa_domain::units::Percent(120.0))
    );
    assert!(!peer.skip_step());
    assert!(peer.group_workout_pause(true).is_err());
    host.change_workout(Workout::ConstantPower(Watts(999.0)));
    assert!(
        host.ride_state()
            .unwrap()
            .workout
            .unwrap()
            .progress
            .is_some(),
        "a frozen shared plan cannot be replaced during riding"
    );
    host.group_workout_pause(true).unwrap();
    wait(&mut host, &mut peer, |host, peer| {
        host.ride_state().unwrap().workout.unwrap().paused
            && peer.ride_state().unwrap().workout.unwrap().paused
    });
    let held = peer.ride.as_ref().unwrap().ride.workout_elapsed().unwrap();
    let recorded = peer.ride_state().unwrap().elapsed;
    peer.update(Duration::from_secs(1));
    host.update(Duration::from_secs(1));
    assert_eq!(
        peer.ride.as_ref().unwrap().ride.workout_elapsed(),
        Some(held)
    );
    assert_eq!(
        peer.ride_state().unwrap().elapsed,
        recorded + Duration::from_secs(1)
    );
    assert!(
        !peer.is_paused(),
        "group workout pause does not stop personal riding"
    );
    assert_eq!(
        peer.ride_state().unwrap().workout.unwrap().target_power,
        None
    );
    assert!(host.skip_step());
    wait(&mut host, &mut peer, |host, peer| {
        host.ride_state()
            .unwrap()
            .workout
            .unwrap()
            .progress
            .unwrap()
            .step
            > 0
            && peer
                .ride_state()
                .unwrap()
                .workout
                .unwrap()
                .progress
                .unwrap()
                .step
                > 0
    });
    assert!(
        host.group().workout_clock.unwrap().paused && peer.group().workout_clock.unwrap().paused
    );
    host.group_workout_pause(false).unwrap();
    wait(&mut host, &mut peer, |host, peer| {
        !host.ride_state().unwrap().workout.unwrap().paused
            && !peer.ride_state().unwrap().workout.unwrap().paused
    });
    let state = peer.ride_state().unwrap().workout.unwrap();
    let expected = peer
        .group()
        .plan
        .unwrap()
        .at(state.progress.unwrap().elapsed, Watts(300.0))
        .unwrap()
        .power
        .unwrap()
        .0
        * 1.2;
    assert!((state.target_power.unwrap().0 - expected).abs() < 0.01);
    peer.set_paused(true);
    let recorded = peer.ride_state().unwrap().elapsed;
    let samples = peer.ride.as_ref().unwrap().ride.samples().len();
    assert!(host.skip_step());
    wait(&mut host, &mut peer, |host, _| {
        host.ride_state()
            .unwrap()
            .workout
            .unwrap()
            .progress
            .unwrap()
            .step
            >= 2
    });
    assert_eq!(peer.ride_state().unwrap().elapsed, recorded);
    assert_eq!(peer.ride.as_ref().unwrap().ride.samples().len(), samples);
    peer.set_paused(false);
    wait(&mut host, &mut peer, |_, peer| {
        peer.ride_state()
            .unwrap()
            .workout
            .unwrap()
            .progress
            .unwrap()
            .step
            >= 2
    });
    assert_eq!(
        peer.ride_state().unwrap().workout.unwrap().intensity,
        orqwa_domain::units::Percent(120.0)
    );
    host.group_leave();
    wait(&mut host, &mut peer, |_, peer| {
        peer.group().phase == Phase::Ended
    });
    let before = peer.ride.as_ref().unwrap().ride.workout_elapsed().unwrap();
    peer.update(Duration::from_secs(1));
    assert_eq!(
        peer.ride.as_ref().unwrap().ride.workout_elapsed(),
        Some(before + Duration::from_secs(1))
    );
}
#[test]
fn app_lobby_downloads_a_workout_starts_locally_and_survives_host_loss() {
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut peer = to.app();
    host.connect_trainer(TrainerChoice::Fake(FakeRider {
        power: Watts(180.0),
        cadence: Rpm(85.0),
        heart: None,
    }))
    .unwrap();
    peer.connect_trainer(TrainerChoice::Fake(FakeRider {
        power: Watts(300.0),
        cadence: Rpm(90.0),
        heart: None,
    }))
    .unwrap();
    peer.profile.profile.ftp = Watts(300.0);
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut peer, |h, _| !h.group().invite.is_empty());
    peer.group_open(Some(host.group().invite), options())
        .unwrap();
    wait(&mut host, &mut peer, |_, p| !p.group().reply.is_empty());
    host.group_admit(peer.group().reply).unwrap();
    wait(&mut host, &mut peer, |h, p| {
        h.group().roster.iter().flatten().count() == 2 && p.group().phase == Phase::Waiting
    });
    assert!(
        host.start_workout(
            host.structured_workout("builtin:recovery-30").unwrap(),
            false,
            DescentMode::Coast,
            "Solo"
        )
        .is_err()
    );
    host.group_select(None, Some("builtin:recovery-30"))
        .unwrap();
    wait(&mut host, &mut peer, |h, p| {
        h.group().prepared && p.group().prepared && h.trainer_connected() && p.trainer_connected()
    });
    assert!(
        peer.workouts()
            .iter()
            .any(|w| w.id.contains("shared-workouts"))
    );
    assert!(host.group_start().is_err());
    host.group_ready(true).unwrap();
    peer.group_ready(true).unwrap();
    wait(&mut host, &mut peer, |h, _| h.group().can_start());
    host.group_start().unwrap();
    // A setting click queued behind Start must not rewrite frozen rules or stop the actor.
    host.group_command(Command::Assistance(orqwa_net::activity::Assistance {
        drafting: true,
        keep_together: false,
    }))
    .unwrap();
    wait(&mut host, &mut peer, |h, p| {
        h.ride_state().is_some() && p.ride_state().is_some()
    });
    assert!(host.group().assistance.drafting && host.group().assistance.keep_together);
    assert_eq!(host.group().assistance, peer.group().assistance);
    let a = host.ride_state().unwrap().workout.unwrap();
    let b = peer.ride_state().unwrap().workout.unwrap();
    let a = a.target_power.unwrap().0;
    let b = b.target_power.unwrap().0;
    assert!(
        (b - a * 1.5).abs() < 1.0,
        "FTP-relative targets must follow each rider: {a}, {b}"
    );
    host.group_leave();
    wait(&mut host, &mut peer, |_, p| p.group().phase == Phase::Ended);
    let before = peer.ride_state().unwrap().elapsed;
    peer.update(Duration::from_secs(1));
    assert!(peer.ride_state().unwrap().elapsed > before);
    peer.group_leave();
    assert!(
        peer.workouts()
            .iter()
            .any(|w| w.id.contains("shared-workouts"))
    );
}

#[test]
fn host_and_joiner_learn_how_friends_could_reach_them_from_their_own_discovery() {
    use orqwa_net::code::Reachability;
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut peer = to.app();
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut peer, |h, _| !h.group().invite.is_empty());
    // Local-test addresses only: nobody at another home could connect.
    assert_eq!(host.group().reachability, Some(Reachability::Home));
    peer.group_open(Some(host.group().invite), options())
        .unwrap();
    wait(&mut host, &mut peer, |_, p| !p.group().reply.is_empty());
    assert_eq!(peer.group().reachability, Some(Reachability::Home));
    peer.group_leave();
    host.group_leave();
}

#[test]
fn a_late_lobby_admission_receives_the_already_selected_course_without_a_local_copy() {
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut peer = to.app();
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../orqwa-gd/courses/twann-chasseral.tqc");
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut peer, |h, _| !h.group().invite.is_empty());
    host.group_select(Some(source.clone()), None).unwrap();
    wait(&mut host, &mut peer, |h, _| h.group().selection.is_some());
    peer.group_open(Some(host.group().invite), options())
        .unwrap();
    wait(&mut host, &mut peer, |_, p| !p.group().reply.is_empty());
    host.group_admit(peer.group().reply).unwrap();
    wait(&mut host, &mut peer, |_, p| p.group().course.is_some());
    let downloaded = peer.group().course.unwrap();
    assert!(downloaded.starts_with(peer.courses_dir()));
    assert_eq!(
        std::fs::read(&source).unwrap(),
        std::fs::read(downloaded).unwrap()
    );
    peer.group_leave();
    host.group_leave();
}

#[test]
fn voice_off_closes_the_transmission_gate_synchronously_without_opening_audio_devices() {
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut idle = to.app();
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut idle, |h, _| {
        h.group().phase == Phase::Waiting
    });
    let port = host
        .group
        .as_ref()
        .unwrap()
        .voice_controls
        .port
        .lock()
        .unwrap()
        .clone()
        .unwrap();
    port.enable(true);
    port.microphone(true).unwrap();
    assert!(port.transmitting().is_some());
    host.group_voice(false, false, false, 0.6).unwrap();
    assert_eq!(
        port.transmitting(),
        None,
        "off must gate transmission before the actor processes its command"
    );
    host.group_leave();
    assert_eq!(port.transmitting(), None);
}

#[test]
fn changing_devices_mutes_immediately_and_supersedes_an_older_queued_unmute() {
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut idle = to.app();
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut idle, |h, _| {
        h.group().phase == Phase::Waiting
    });
    let controls = host.group.as_ref().unwrap().voice_controls.clone();
    let stale = controls.generation.load(Ordering::Acquire);
    let port = controls.port.lock().unwrap().clone().unwrap();
    host.group_voice_volume(0.4).unwrap();
    port.enable(true);
    port.microphone(true).unwrap();
    assert!(port.transmitting().is_some());
    host.group_voice_choose("headset-input".into(), "headset-output".into())
        .unwrap();
    assert_eq!(
        port.transmitting(),
        None,
        "device changes mute synchronously"
    );
    assert!(controls.generation.load(Ordering::Acquire) > stale);
    host.group_command(Command::Voice(stale, true, true, false, 0.6))
        .unwrap();
    // An independent processed command provides an actor barrier without opening hardware.
    host.group_command(Command::Failure("device-switch barrier".into()))
        .unwrap();
    wait(&mut host, &mut idle, |h, _| {
        h.group().error == "device-switch barrier"
    });
    assert_eq!(
        port.transmitting(),
        None,
        "stale unmute cannot reopen capture"
    );
    assert!(!host.group().voice_pending);
    assert_eq!(host.group().input_device, "headset-input");
    assert_eq!(host.group().output_device, "headset-output");
    assert!((host.group().voice_volume - 0.4).abs() < f32::EPSILON);
    host.group_leave();
}

#[test]
fn shared_rules_retain_verified_content_invalidate_ready_and_freeze_at_start() {
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut peer = to.app();
    host.profile.profile.ftp = Watts(200.0);
    peer.profile.profile.ftp = Watts(300.0);
    host.connect_trainer(TrainerChoice::Fake(FakeRider {
        power: Watts(180.0),
        cadence: Rpm(85.0),
        heart: None,
    }))
    .unwrap();
    peer.connect_trainer(TrainerChoice::Fake(FakeRider {
        power: Watts(300.0),
        cadence: Rpm(95.0),
        heart: None,
    }))
    .unwrap();
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut peer, |h, _| !h.group().invite.is_empty());
    peer.group_open(Some(host.group().invite), options())
        .unwrap();
    wait(&mut host, &mut peer, |_, p| !p.group().reply.is_empty());
    host.group_admit(peer.group().reply).unwrap();
    wait(&mut host, &mut peer, |h, p| {
        h.group().roster.iter().flatten().count() == 2 && p.group().phase == Phase::Waiting
    });
    host.group_select(None, Some("builtin:recovery-30"))
        .unwrap();
    wait(&mut host, &mut peer, |h, p| {
        h.group().prepared && p.group().prepared
    });
    let received = peer.group().workout;
    assert!(host.group().assistance.drafting && host.group().assistance.keep_together);
    host.group_ready(true).unwrap();
    peer.group_ready(true).unwrap();
    wait(&mut host, &mut peer, |h, _| h.group().can_start());
    host.group_assistance(false, true).unwrap();
    wait(&mut host, &mut peer, |h, p| {
        !h.group().ready[0] && !p.group().ready[1] && !p.group().assistance.drafting
    });
    assert!(host.group().prepared && peer.group().prepared);
    assert_eq!(
        peer.group().workout,
        received,
        "rule changes retain exact verified files"
    );
    assert!(!host.group().can_start());
    assert!(peer.group_assistance(true, false).is_err());
    host.group_ready(true).unwrap();
    peer.group_ready(true).unwrap();
    wait(&mut host, &mut peer, |h, _| h.group().can_start());
    host.group_start().unwrap();
    wait(&mut host, &mut peer, |h, p| {
        h.ride_state().is_some() && p.ride_state().is_some()
    });
    wait(&mut host, &mut peer, |h, p| {
        h.group_riders()
            .first()
            .is_some_and(|r| (r.power.0 - 120.0).abs() < 2.0)
            && p.group_riders()
                .first()
                .is_some_and(|r| (r.power.0 - 80.0).abs() < 2.0)
    });
    assert!(host.group_assistance(true, false).is_err());
    // The fake trainer follows ERG. This plan starts at 40% of each rider's own FTP.
    assert!((host.ride_state().unwrap().telemetry.power.unwrap().0 - 80.0).abs() < 2.0);
    assert!((peer.ride_state().unwrap().telemetry.power.unwrap().0 - 120.0).abs() < 2.0);
    assert!((host.group_riders()[0].power.0 - 120.0).abs() < 2.0);
    assert!((peer.group_riders()[0].power.0 - 80.0).abs() < 2.0);
    peer.set_paused(true);
    wait(&mut host, &mut peer, |h, _| {
        h.group_riders()
            .first()
            .is_some_and(|r| r.speed.0 == 0.0 && r.cadence.0 == 0.0)
    });
    peer.set_paused(false);
    host.group_leave();
    wait(&mut host, &mut peer, |_, p| p.group().phase == Phase::Ended);
    assert!(peer.group_riders().is_empty());
    assert!(
        peer.ride.as_ref().unwrap().assisted,
        "eligibility remains marked after leaving"
    );
    peer.group_leave();
}

#[test]
fn assisted_course_saves_measured_power_without_course_or_climb_speed_records() {
    let directory = Directory::new();
    let mut app = directory.app();
    let xml = r#"<gpx><trk><trkseg><trkpt lat="46" lon="7"><ele>500</ele></trkpt><trkpt lat="46.01" lon="7"><ele>650</ele></trkpt></trkseg></trk></gpx>"#;
    let route = app
        .runtime
        .block_on(orqwa_routes::Route::from_gpx(xml, None))
        .unwrap();
    assert!(
        !route.climbs().is_empty(),
        "the fixture includes a classified climb"
    );
    let ride = orqwa_session::Ride::new(route, orqwa_session::RideConfig::default());
    app.begin(
        ride,
        Some("Assisted climb".into()),
        crate::RouteRecords::default(),
        None,
        (None, None),
    );
    let active = app.ride.as_mut().unwrap();
    active.started = Some(std::time::SystemTime::now());
    active.assisted = true;
    active
        .ride
        .set_group_effect(orqwa_physics::GroupEffect::new(
            0.7,
            orqwa_domain::units::GradePercent(-2.0),
        ));
    active
        .ride
        .on_telemetry(&orqwa_domain::telemetry::Telemetry {
            power: Some(Watts(200.0)),
            cadence: Some(Rpm(85.0)),
            heart_rate: Some(orqwa_domain::units::BeatsPerMinute(151.0)),
            ..Default::default()
        });
    for _ in 0..2000 {
        active.ride.tick(Duration::from_secs(1));
        if active.ride.is_finished() {
            break;
        }
    }
    assert!(active.ride.is_finished());
    app.finish_ride();
    let history = app.history();
    assert_eq!(history.len(), 1);
    let record = &history[0].record;
    assert!(record.assisted && !record.simulated);
    assert!(record.route_key.is_some());
    assert!(record.route_time.is_none() && record.climbs.is_empty());
    assert!(!history[0].route_record);
    assert!(
        !history[0].power_records.is_empty(),
        "real measured power remains eligible"
    );
    assert_eq!(record.summary.avg_power, Some(Watts(200.0)));
    let (_, samples) = crate::read_fit(&history[0].fit).unwrap();
    assert_ne!(samples.len(), 0);
    assert!(
        samples
            .iter()
            .all(|sample| sample.power == Some(Watts(200.0))
                && sample.cadence == Some(Rpm(85.0))
                && sample.heart_rate == Some(orqwa_domain::units::BeatsPerMinute(151.0)))
    );
}

#[test]
fn late_course_entry_saves_only_ridden_distance_and_no_skipped_speed_records() {
    let directory = Directory::new();
    let mut app = directory.app();
    let xml = r#"<gpx><trk><trkseg><trkpt lat="46" lon="7"><ele>500</ele></trkpt><trkpt lat="46.01" lon="7"><ele>650</ele></trkpt><trkpt lat="46.02" lon="7"><ele>500</ele></trkpt><trkpt lat="46.03" lon="7"><ele>800</ele></trkpt></trkseg></trk></gpx>"#;
    let route = app
        .runtime
        .block_on(orqwa_routes::Route::from_gpx(xml, None))
        .unwrap();
    assert_eq!(route.climbs().len(), 2);
    let entry = Meters(route.climbs()[0].start.0.midpoint(route.climbs()[0].end.0));
    let full_distance = route.length().0;
    let mut ride = orqwa_session::Ride::new(route, orqwa_session::RideConfig::default());
    assert!(ride.enter_group(entry, None));
    app.begin(
        ride,
        Some("Joined halfway".into()),
        crate::RouteRecords::default(),
        None,
        (None, None),
    );
    assert!(
        app.current_climb().is_none(),
        "a skipped climb start has no personal timer"
    );
    let active = app.ride.as_mut().unwrap();
    active.started = Some(std::time::SystemTime::now());
    active
        .ride
        .on_telemetry(&orqwa_domain::telemetry::Telemetry {
            power: Some(Watts(200.0)),
            cadence: Some(Rpm(85.0)),
            ..Default::default()
        });
    for _ in 0..4000 {
        active.ride.tick(Duration::from_secs(1));
        if active.ride.is_finished() {
            break;
        }
    }
    assert!(active.ride.is_finished());
    app.finish_ride();
    let history = app.history();
    let record = &history[0].record;
    assert!(!record.simulated && !record.assisted);
    assert!(record.route_time.is_none());
    assert_eq!(
        record.climbs.len(),
        1,
        "only the complete later climb is eligible"
    );
    assert!(record.climbs[0].start.0 >= entry.0);
    assert!(!history[0].route_record);
    assert!(
        !history[0].power_records.is_empty(),
        "genuine post-entry power remains eligible"
    );
    assert!((record.summary.distance.0 - (full_distance - entry.0)).abs() < 1.0);
    let (_, samples) = crate::read_fit(&history[0].fit).unwrap();
    assert_eq!(samples[0].distance, Meters(0.0));
    assert_eq!(samples[0].elapsed, Duration::ZERO);
    assert!(
        samples
            .iter()
            .all(|sample| sample.power == Some(Watts(200.0)))
    );
    assert!((samples.last().unwrap().distance.0 - (full_distance - entry.0)).abs() < 1.0);
}
