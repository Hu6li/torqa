use super::*;
use crate::{App, TrainerChoice};
use orqwa_devices::fake::FakeRider;
use orqwa_domain::profile::Avatar;
use orqwa_domain::units::{Kilograms, Rpm, Watts};
use orqwa_net::code::{ConnectionMode, Setup};
use std::path::Path;
use std::time::Duration;

#[test]
fn roster_ping_uses_each_friends_organizer_link_and_hides_unavailable_readings() {
    let mut snapshot = Snapshot {
        phase: Phase::Riding,
        me: 1,
        organizer: 2,
        ..Snapshot::default()
    };
    for id in 0..3 {
        snapshot.roster[id] = Some(Rider {
            name: format!("Rider {id}"),
            mass: Kilograms(83.0),
            weight: Kilograms(75.0),
            avatar: Avatar::Female,
        });
    }
    snapshot.latencies[0] = Some(Duration::from_millis(250));
    snapshot.latencies[1] = Some(Duration::from_millis(70));
    assert_eq!(snapshot.ping(0), Some(Duration::from_millis(250)));
    assert_eq!(snapshot.ping(1), None, "no self ping");
    assert_eq!(
        snapshot.ping(2),
        Some(Duration::from_millis(70)),
        "our link to the organizer"
    );
    assert_eq!(snapshot.ping(3), None);
    assert_eq!(snapshot.ping(MAX_RIDERS), None);
    snapshot.away[0] = true;
    assert_eq!(snapshot.ping(0), None);
    snapshot.reconnecting = Some(Instant::now());
    assert_eq!(snapshot.ping(2), None);
    snapshot.reconnecting = None;
    snapshot.me = 2;
    snapshot.host = true;
    assert_eq!(snapshot.ping(2), None);
    assert_eq!(snapshot.ping(1), Some(Duration::from_millis(70)));
    snapshot.phase = Phase::Ended;
    assert_eq!(snapshot.ping(1), None);
}

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
/// Voice audio that hears and plays nothing, so voice restarts run without audio hardware.
pub(super) struct Silent;
impl orqwa_domain::voice::VoiceAudio for Silent {
    fn microphone(&mut self, _: Option<u32>) -> Result<(), orqwa_domain::voice::VoiceDeviceError> {
        Ok(())
    }
    fn capture(&mut self) -> Option<orqwa_domain::voice::CapturedVoice> {
        None
    }
    fn play(&mut self, _: orqwa_domain::voice::PcmFrame) {}
    fn deafen(&mut self, _: bool) {}
    fn healthy(&self) -> bool {
        true
    }
}

/// Opens this rider's voice on [`Silent`] audio, listening with the microphone `mic`.
fn speak(app: &mut App, other: &mut App, mic: bool) {
    app.group
        .as_ref()
        .unwrap()
        .voice_controls
        .silent
        .store(true, Ordering::Release);
    app.group_voice(true, false, false, 0.6).unwrap();
    wait(app, other, |a, _| a.group().voice);
    app.group_voice(true, mic, false, 0.6).unwrap();
    wait(app, other, |a, _| a.group().microphone == mic);
}

/// A local test rides on one computer, as a group does on one home network.
fn options() -> ConnectOptions {
    let mut options = ConnectOptions::default();
    options.gather.policy.loopback = true;
    options.gather.stun_servers.clear();
    options.setup = Setup::new(ConnectionMode::Home, false);
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
fn buffered_group_positions_never_replace_live_telemetry_recording_or_assistance() {
    let directory = Directory::new();
    let mut app = directory.app();
    app.group_open(None, options()).unwrap();
    // Supply an authenticated-state fixture without depending on network scheduling.
    app.group.as_mut().unwrap().task.abort();
    let config = app.ride_config(orqwa_domain::units::Percent(100.0), DescentMode::Coast);
    app.begin(
        crate::Ride::workout(Workout::ConstantPower(Watts(200.0)), config),
        None,
        crate::RouteRecords::default(),
        None,
        (None, None),
    );
    let ride = &mut app.ride.as_mut().unwrap().ride;
    ride.on_telemetry(&orqwa_domain::telemetry::Telemetry {
        power: Some(Watts(200.0)),
        cadence: Some(Rpm(85.0)),
        ..Default::default()
    });
    for _ in 0..20 {
        ride.tick(Duration::from_millis(100));
    }
    let before = app.ride_state().unwrap();
    let recorded = app.ride_samples().to_vec();
    let now = Instant::now();
    let mut snapshot = Snapshot {
        phase: Phase::Riding,
        start: Some(now.checked_sub(Duration::from_secs(2)).unwrap()),
        ..Snapshot::default()
    };
    snapshot.roster[0] = Some(Rider {
        name: "Local".into(),
        mass: Kilograms(83.0),
        weight: Kilograms(75.0),
        avatar: Avatar::Female,
    });
    snapshot.generations[0] = 1;
    for ms in [1000, 1200, 1400, 1600, 1800, 2000] {
        snapshot.states[0] = Some((
            RiderState {
                at: Duration::from_millis(ms),
                distance: Meters(100.0),
                speed: MetersPerSecond(0.0),
                power: Watts(4000.0),
                cadence: Rpm(250.0),
            },
            now.checked_sub(Duration::from_millis(2000 - ms)).unwrap(),
        ));
        app.update_group_motion(&snapshot);
    }
    let (updates, receive) = watch::channel(snapshot.clone());
    {
        let group = app.group.as_mut().unwrap();
        group.snapshot = receive;
        group.launched = true;
    }
    assert_eq!(app.group_view().distance, Some(Meters(100.0)));
    assert!(
        app.group_view().riders.is_empty(),
        "one rider cannot see their own echo as a peer"
    );
    assert_eq!(
        app.group.as_ref().unwrap().effect,
        orqwa_physics::GroupEffect::default(),
        "one rider cannot draft or get keep-together assistance from their own echo"
    );
    let after = app.ride_state().unwrap();
    assert_eq!(after.distance, before.distance);
    assert_eq!(after.elapsed, before.elapsed);
    assert_eq!(after.telemetry.power, Some(Watts(200.0)));
    assert_eq!(after.telemetry.cadence, Some(Rpm(85.0)));
    assert_eq!(app.ride_samples().len(), recorded.len());
    for (actual, expected) in app.ride_samples().iter().zip(recorded) {
        assert_eq!(actual.distance, expected.distance);
        assert_eq!(actual.power, expected.power);
    }
    snapshot.reconnecting = Some(now + Duration::from_secs(30));
    app.update_group_motion(&snapshot);
    updates.send_replace(snapshot);
    assert!(
        app.group_view().distance.is_none(),
        "reconnecting discards old accepted motion"
    );
}

#[test]
fn pasted_replies_add_friends_or_say_why_not_and_a_late_reply_runs_out_quietly() {
    let (one, two, three) = (Directory::new(), Directory::new(), Directory::new());
    let (mut host, mut peer, mut late) = (one.app(), two.app(), three.app());
    let mut quick = options();
    quick.window = Duration::from_secs(1);
    host.group_open(None, quick.clone()).unwrap();
    wait(&mut host, &mut peer, |host, _| {
        !host.group().invite.is_empty()
    });
    // No reply at all: said beside the organizer's field (R195), not as a failure.
    host.group_admit("see you at seven").unwrap();
    wait(&mut host, &mut peer, |host, _| {
        host.group().refusal == Refusal::Invalid
    });
    assert_eq!(host.group().error, "");
    // A reply pasted with the message it came in adds the friend.
    peer.group_open(Some(host.group().invite), options())
        .unwrap();
    wait(&mut host, &mut peer, |_, peer| {
        !peer.group().reply.is_empty()
    });
    assert!(
        peer.group().reply_until.is_some(),
        "the joiner sees how long it has"
    );
    let reply = peer.group().reply;
    host.group_admit(&format!("Here is mine: {reply} thanks"))
        .unwrap();
    wait(&mut host, &mut peer, |host, _| {
        host.group().refusal == Refusal::None && host.group().roster.iter().flatten().count() == 2
    });
    assert_eq!(host.group().admitting, 0, "nobody is connecting any more");
    host.group_admit(&reply).unwrap();
    wait(&mut host, &mut peer, |host, _| {
        host.group().refusal == Refusal::Again
    });
    // A friend whose computer never connects runs out after the reply's minute, here one second.
    late.group_open(Some(host.group().invite), options())
        .unwrap();
    wait(&mut host, &mut late, |_, late| {
        !late.group().reply.is_empty()
    });
    let gone = late.group().reply;
    late.group_leave();
    host.group_admit(&gone).unwrap();
    wait(&mut host, &mut late, |host, _| {
        host.group().refusal == Refusal::Late
    });
    assert_eq!(host.group().admitting, 0);
    assert_eq!(host.group().error, "");
    // The joiner not added in time is told so, without a failure, and may make a new reply.
    late.group_open(Some(host.group().invite), quick).unwrap();
    wait(&mut host, &mut late, |_, late| {
        late.group().phase == Phase::Failed
    });
    assert!(late.group().reply_expired);
    assert!(late.group().error.is_empty(), "{}", late.group().error);
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
    host.group_admit(&peer.group().reply).unwrap();
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
    host.group_admit(&peer.group().reply).unwrap();
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
fn ending_and_completing_activities_reuses_the_group_and_keeps_voice_and_recordings() {
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut peer = to.app();
    start_shared_workout(&mut host, &mut peer);
    speak(&mut host, &mut peer, true);
    speak(&mut peer, &mut host, true);
    let invite = host.group().invite;
    let root = peer.group().root;
    let memberships = peer.group().generations;
    assert!(
        peer.group_end_activity().is_err(),
        "only the organizer ends everyone's ride"
    );
    for round in 0..2 {
        wait(&mut host, &mut peer, |h, p| {
            h.ride_samples().len() >= 2 && p.ride_samples().len() >= 2
        });
        if round == 0 {
            host.group_end_activity().unwrap();
        } else {
            // A completed workout uses the same lifecycle without a UI finishing it.
            let duration = host.group().plan.unwrap().duration();
            host.group_command(Command::WorkoutSkip(duration)).unwrap();
        }
        wait(&mut host, &mut peer, |h, p| {
            h.group().phase == Phase::Waiting
                && p.group().phase == Phase::Waiting
                && h.ride_state().is_none()
                && p.ride_state().is_none()
        });
        for app in [&host, &peer] {
            let status = app.group();
            assert!(status.selection.is_none() && status.start.is_none());
            assert!(!status.ready.contains(&true));
            assert!(status.states.iter().all(Option::is_none));
            assert!(
                status.voice && status.microphone,
                "voice keeps the rider's gates"
            );
            assert_eq!(status.roster.iter().flatten().count(), 2);
            assert_eq!(
                app.history().len(),
                round + 1,
                "each local recording is saved once"
            );
        }
        assert_eq!(host.group().invite, invite);
        assert_eq!(peer.group().root, root);
        assert_eq!(peer.group().generations, memberships);
        host.group_select(None, Some("builtin:recovery-30"))
            .unwrap();
        wait(&mut host, &mut peer, |h, p| {
            h.group().prepared && p.group().prepared
        });
        assert!(!host.group().can_start());
        host.group_ready(true).unwrap();
        peer.group_ready(true).unwrap();
        wait(&mut host, &mut peer, |h, _| h.group().can_start());
        host.group_start().unwrap();
        if round == 0 {
            wait(&mut host, &mut peer, |h, p| {
                h.ride_state().is_some() && p.ride_state().is_some()
            });
            assert!(host.ride_state().unwrap().elapsed < Duration::from_secs(1));
            assert!(peer.ride_state().unwrap().elapsed < Duration::from_secs(1));
        }
    }
    // Cancel the third countdown, then wait beyond it: nobody launches a phantom ride.
    wait(&mut host, &mut peer, |h, p| {
        h.group().phase == Phase::Countdown && p.group().phase == Phase::Countdown
    });
    host.group_end_activity().unwrap();
    wait(&mut host, &mut peer, |h, p| {
        h.group().phase == Phase::Waiting && p.group().phase == Phase::Waiting
    });
    let until = Instant::now() + Duration::from_secs(6);
    while Instant::now() < until {
        host.update(Duration::from_millis(16));
        peer.update(Duration::from_millis(16));
        assert!(host.ride_state().is_none() && peer.ride_state().is_none());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(host.history().len(), 2);
    assert_eq!(peer.history().len(), 2);
    assert!(host.group().voice && peer.group().voice);
}

#[test]
fn a_rider_whose_connection_is_lost_rejoins_by_themselves_and_takes_up_the_groups_workout_step() {
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut peer = to.app();
    start_shared_workout(&mut host, &mut peer);
    wait(&mut host, &mut peer, |_, p| {
        p.group().phase == Phase::Riding
    });
    speak(&mut peer, &mut host, true);
    let seat = peer.group().me;
    // #434: the peer's network drops mid-workout, and nobody exchanges codes again.
    peer.group_command(Command::LoseConnection).unwrap();
    // A later command the runtime has processed shows the connection is gone first.
    peer.group_voice_volume(0.5).unwrap();
    wait(&mut host, &mut peer, |_, p| {
        (p.group().voice_volume - 0.5).abs() < f32::EPSILON
    });
    // The organizer pauses meanwhile: only a new connection can tell the peer.
    host.group_workout_pause(true).unwrap();
    wait(&mut host, &mut peer, |h, p| {
        p.ride_state().unwrap().workout.unwrap().paused
            && p.group().reconnecting.is_none()
            && !h.group().away.contains(&true)
    });
    assert_eq!(peer.group().me, seat);
    assert!(
        peer.group().phase == Phase::Riding,
        "{} {}",
        peer.group().phase.key(),
        peer.group().error
    );
    assert!(peer.ride.is_some(), "the local ride never stopped");
    assert!(peer.group().microphone, "the microphone stays on as it was");
    peer.group_leave();
    host.group_leave();
}

#[test]
fn changing_devices_restarts_voice_with_the_microphone_as_it_was() {
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut idle = to.app();
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut idle, |h, _| {
        h.group().phase == Phase::Waiting
    });
    for (mic, device) in [(true, "headset"), (false, "speakers")] {
        speak(&mut host, &mut idle, mic);
        host.group_voice_choose(format!("{device}-input"), format!("{device}-output"))
            .unwrap();
        wait(&mut host, &mut idle, |h, _| {
            let group = h.group();
            group.voice && !group.voice_pending && group.input_device == format!("{device}-input")
        });
        assert_eq!(host.group().microphone, mic, "{device}");
    }
    host.group_leave();
}

#[test]
fn a_rider_reconnecting_keeps_the_start_and_an_organizer_change_waiting() {
    let rider = Rider {
        name: "Anna".into(),
        mass: Kilograms(80.0),
        weight: Kilograms(70.0),
        avatar: Avatar::Female,
    };
    let mut lobby = Snapshot {
        host: true,
        prepared: true,
        selection: Some(Selection {
            revision: 1,
            name: "Workout".into(),
            course: None,
            course_key: None,
            workout: Some(
                orqwa_net::activity::ContentId::of(
                    orqwa_net::activity::ContentKind::Workout,
                    b"workout",
                )
                .unwrap(),
            ),
            assistance: Assistance::default(),
        }),
        ready: [true; MAX_RIDERS],
        ..Snapshot::default()
    };
    lobby.roster[0] = Some(rider.clone());
    lobby.roster[1] = Some(rider);
    assert!(lobby.can_start());
    // #434: until the rider is back or removed.
    lobby.away[1] = true;
    assert!(!lobby.can_start());
    let mut riding = Snapshot {
        phase: Phase::Riding,
        start: Some(Instant::now()),
        workout_clock: Some(orqwa_net::lobby::WorkoutClock {
            revision: 1,
            sequence: 1,
            elapsed: Duration::ZERO,
            at: Instant::now(),
            paused: false,
        }),
        ..lobby.clone()
    };
    assert!(!riding.can_handover());
    riding.away[1] = false;
    assert!(riding.can_handover());
}

#[track_caller]
fn wait3(apps: [&mut App; 3], condition: impl Fn(&App, &App, &App) -> bool) {
    let [a, b, c] = apps;
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        for app in [&mut *a, &mut *b, &mut *c] {
            app.update(Duration::from_millis(16));
        }
        if condition(a, b, c) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let state = |app: &App| {
        let group = app.group();
        format!(
            "{} host {} organizer {} reconnecting {} error {:?}",
            group.phase.key(),
            group.host,
            group.organizer,
            group.reconnecting.is_some(),
            group.error
        )
    };
    panic!(
        "group timed out: {} / {} / {}",
        state(a),
        state(b),
        state(c)
    );
}

/// The organizer and two friends connected, without a selected activity.
fn lobby_in_three(host: &mut App, anna: &mut App, ben: &mut App) {
    for app in [&mut *host, &mut *anna, &mut *ben] {
        app.connect_trainer(TrainerChoice::Fake(FakeRider {
            power: Watts(180.0),
            cadence: Rpm(85.0),
            heart: None,
        }))
        .unwrap();
    }
    host.group_open(None, options()).unwrap();
    wait3([host, anna, ben], |h, _, _| !h.group().invite.is_empty());
    for (count, friend) in [(2, &mut *anna), (3, &mut *ben)] {
        friend
            .group_open(Some(host.group().invite), options())
            .unwrap();
        wait(host, friend, |_, f| !f.group().reply.is_empty());
        host.group_admit(&friend.group().reply).unwrap();
        wait(host, friend, |h, f| {
            h.group().roster.iter().flatten().count() == count && f.group().phase == Phase::Waiting
        });
    }
}

/// The organizer and two friends prepared and ready, before any countdown.
fn ready_in_three(host: &mut App, anna: &mut App, ben: &mut App) {
    lobby_in_three(host, anna, ben);
    prepare_in_three(host, anna, ben);
}

/// Selects a shared workout and readies an already connected group.
fn prepare_in_three(host: &mut App, anna: &mut App, ben: &mut App) {
    host.group_select(None, Some("builtin:recovery-30"))
        .unwrap();
    wait3([host, anna, ben], |h, a, b| {
        [h, a, b]
            .iter()
            .all(|app| app.group().prepared && app.trainer_connected())
    });
    for app in [&*host, &*anna, &*ben] {
        app.group_ready(true).unwrap();
    }
    wait3([host, anna, ben], |h, _, _| h.group().can_start());
}

/// The organizer and two friends riding a shared workout together.
fn ride_in_three(host: &mut App, anna: &mut App, ben: &mut App) {
    ready_in_three(host, anna, ben);
    host.group_start().unwrap();
    wait3([host, anna, ben], |h, a, b| {
        [h, a, b]
            .iter()
            .all(|app| app.ride_state().is_some() && app.group().phase == Phase::Riding)
    });
}

#[test]
fn a_lobby_fallback_asks_immediately_and_keeps_consent_across_rides() {
    let directories = [Directory::new(), Directory::new(), Directory::new()];
    let [mut host, mut anna, mut ben] = directories.each_ref().map(Directory::app);
    lobby_in_three(&mut host, &mut anna, &mut ben);
    wait3([&mut host, &mut anna, &mut ben], |h, a, b| {
        h.group().preferred_fallback == Some(a.group().me)
            && a.group().fallback_consent.is_some()
            && a.group().standby_offer
            && !b.group().standby_offer
    });
    assert!(
        host.group().selection.is_none(),
        "automatic consent needs no selected activity"
    );
    let selected = ben.group().me;
    assert!(host.group().can_choose_fallback());
    assert!(host.group_fallback(host.group().me).is_err());
    assert!(anna.group_fallback(selected).is_err());
    host.group_fallback(selected).unwrap();
    wait3([&mut host, &mut anna, &mut ben], |h, a, b| {
        h.group().preferred_fallback == Some(selected)
            && b.group().standby_offer
            && b.group().fallback_consent.is_some()
            && !a.group().standby_offer
    });
    for app in [&host, &anna, &ben] {
        assert!(app.group().start.is_none());
        assert!(
            app.group().fallback.is_none(),
            "lobby consent prepares no fallback links"
        );
    }
    assert!(ben.ride.is_none());
    ben.group_handover_answer(true).unwrap();
    wait3([&mut host, &mut anna, &mut ben], |h, _, b| {
        h.group().preferred_fallback_accepted && !b.group().standby_offer
    });
    assert!(host.group().fallback.is_none());
    prepare_in_three(&mut host, &mut anna, &mut ben);
    host.group_start().unwrap();
    wait3([&mut host, &mut anna, &mut ben], |h, a, b| {
        [h, a, b]
            .iter()
            .all(|app| app.group().fallback == Some(selected))
    });
    assert!(
        !ben.group().standby_offer,
        "accepted lobby consent needs no second answer"
    );
    host.group_end_activity().unwrap();
    wait3([&mut host, &mut anna, &mut ben], |h, a, b| {
        [h, a, b]
            .iter()
            .all(|app| app.group().phase == Phase::Waiting)
    });
    assert_eq!(host.group().preferred_fallback, Some(selected));
    assert!(host.group().preferred_fallback_accepted);
    assert!(host.group().can_choose_fallback());
    host.group_fallback(selected).unwrap();
    prepare_in_three(&mut host, &mut anna, &mut ben);
    host.group_start().unwrap();
    wait3([&mut host, &mut anna, &mut ben], |h, a, b| {
        [h, a, b]
            .iter()
            .all(|app| app.group().fallback == Some(selected))
    });
    assert!(
        !ben.group().standby_offer,
        "the next ride retains the group's consent"
    );
    host.group_end_activity().unwrap();
    wait3([&mut host, &mut anna, &mut ben], |h, _, _| {
        h.group().phase == Phase::Waiting
    });
    ben.group_leave();
    wait3([&mut host, &mut anna, &mut ben], |h, _, _| {
        h.group().roster[usize::from(selected)].is_none() && h.group().preferred_fallback.is_some()
    });
    assert_eq!(host.group().preferred_fallback, Some(anna.group().me));
    assert!(!host.group().preferred_fallback_accepted);
}

#[test]
fn a_declined_lobby_fallback_is_not_asked_again_when_the_ride_starts() {
    let directories = [Directory::new(), Directory::new(), Directory::new()];
    let [mut host, mut anna, mut ben] = directories.each_ref().map(Directory::app);
    ready_in_three(&mut host, &mut anna, &mut ben);
    let selected = ben.group().me;
    host.group_fallback(selected).unwrap();
    wait3([&mut host, &mut anna, &mut ben], |_, _, b| {
        b.group().standby_offer
    });
    ben.group_handover_answer(false).unwrap();
    wait3([&mut host, &mut anna, &mut ben], |h, a, b| {
        h.group().preferred_fallback == Some(a.group().me)
            && a.group().fallback_consent.is_some()
            && a.group().standby_offer
            && !b.group().standby_offer
    });
    assert!(!host.group().preferred_fallback_accepted);
    assert!(
        anna.ride.is_none(),
        "the alternative is asked before starting too"
    );
    assert!(
        !ben.group().standby_offer,
        "a previous decline is respected"
    );
    anna.group_handover_answer(true).unwrap();
    let alternative = anna.group().me;
    wait3([&mut host, &mut anna, &mut ben], |h, _, _| {
        h.group().preferred_fallback_accepted
    });
    host.group_start().unwrap();
    wait3([&mut host, &mut anna, &mut ben], |h, _, _| {
        h.group().fallback == Some(alternative)
    });
    assert!(!anna.group().standby_offer && !ben.group().standby_offer);
}

#[test]
fn changing_the_lobby_fallback_withdraws_the_previous_friends_request() {
    let directories = [Directory::new(), Directory::new(), Directory::new()];
    let [mut host, mut anna, mut ben] = directories.each_ref().map(Directory::app);
    ready_in_three(&mut host, &mut anna, &mut ben);
    host.group_fallback(ben.group().me).unwrap();
    wait3([&mut host, &mut anna, &mut ben], |_, _, b| {
        b.group().standby_offer
    });
    host.group_fallback(anna.group().me).unwrap();
    wait3([&mut host, &mut anna, &mut ben], |_, a, b| {
        a.group().standby_offer && !b.group().standby_offer && b.group().fallback_consent.is_none()
    });
    assert!(
        ben.group_handover_answer(true).is_err(),
        "withdrawn lobby offers cannot be accepted"
    );
    anna.group_handover_answer(true).unwrap();
    wait3([&mut host, &mut anna, &mut ben], |h, a, _| {
        h.group().preferred_fallback == Some(a.group().me) && h.group().preferred_fallback_accepted
    });
}

#[test]
fn a_lobby_without_a_friend_has_no_fallback_candidate() {
    let directory = Directory::new();
    let other = Directory::new();
    let mut host = directory.app();
    let mut peer = other.app();
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut peer, |h, _| {
        h.group().phase == Phase::Waiting
    });
    assert!(!host.group().can_choose_fallback());
    assert!(host.group_fallback(host.group().me).is_err());
    assert!(host.group_fallback(7).is_err());
}

#[test]
fn a_departed_fallback_choice_cannot_select_a_replacement_in_the_same_slot() {
    let directories = [
        Directory::new(),
        Directory::new(),
        Directory::new(),
        Directory::new(),
    ];
    let [mut host, mut anna, mut ben, mut replacement] = directories.each_ref().map(Directory::app);
    ready_in_three(&mut host, &mut anna, &mut ben);
    let selected = ben.group().me;
    let generation = host.group().generations[usize::from(selected)];
    host.group_fallback(selected).unwrap();
    wait(&mut host, &mut ben, |h, _| {
        h.group().preferred_fallback == Some(selected)
    });
    ben.group_leave();
    wait(&mut host, &mut anna, |h, a| {
        h.group().roster[usize::from(selected)].is_none()
            && h.group().preferred_fallback == Some(a.group().me)
    });
    replacement
        .group_open(Some(host.group().invite), options())
        .unwrap();
    wait(&mut host, &mut replacement, |_, r| {
        !r.group().reply.is_empty()
    });
    host.group_admit(&replacement.group().reply).unwrap();
    wait(&mut host, &mut replacement, |h, r| {
        r.group().phase == Phase::Ready && h.group().roster.iter().flatten().count() == 3
    });
    assert_eq!(replacement.group().me, selected);
    host.group_command(Command::Fallback(
        RiderId::new(selected).unwrap(),
        generation,
    ))
    .unwrap();
    wait(&mut host, &mut replacement, |h, _| {
        h.group().error.contains("has left the group")
    });
    assert_eq!(host.group().preferred_fallback, Some(anna.group().me));
    assert!(!replacement.group().standby_offer);
}

#[test]
fn a_fallback_takes_over_when_the_organizer_is_lost_and_the_next_one_is_asked_at_once() {
    let directories = [Directory::new(), Directory::new(), Directory::new()];
    let [mut host, mut anna, mut ben] = directories.each_ref().map(Directory::app);
    ready_in_three(&mut host, &mut anna, &mut ben);
    wait3([&mut host, &mut anna, &mut ben], |h, a, _| {
        h.group().preferred_fallback == Some(a.group().me)
            && a.group().standby_offer
            && a.group().fallback_consent.is_some()
    });
    assert!(
        anna.ride.is_none(),
        "the first friend is asked automatically in the lobby"
    );
    assert!(!ben.group().standby_offer);
    anna.group_handover_answer(true).unwrap();
    let fallback = anna.group().me;
    wait3([&mut host, &mut anna, &mut ben], |h, _, _| {
        h.group().preferred_fallback_accepted
    });
    host.group_start().unwrap();
    wait3([&mut host, &mut anna, &mut ben], |h, a, b| {
        [h, a, b]
            .iter()
            .all(|app| app.group().fallback == Some(fallback))
    });
    assert!(
        !anna.group().standby_offer,
        "no new prompt interrupts the first ride"
    );
    for friend in [&mut anna, &mut ben] {
        speak(friend, &mut host, true);
    }
    // The organizer's network fails and its ORQWA does not come back: nobody hears it again.
    for friend in [&anna, &ben] {
        friend.group_command(Command::LoseConnection).unwrap();
    }
    host.group_leave();
    wait3([&mut host, &mut anna, &mut ben], |_, a, b| {
        a.group().host
            && b.group().organizer == fallback
            && [a, b]
                .iter()
                .all(|app| app.group().phase == Phase::Riding && app.group().reconnecting.is_none())
    });
    assert!(
        anna.ride.is_some() && ben.ride.is_some(),
        "riding never stopped"
    );
    // Voice restarts on the new organizer's links with the microphones as they were.
    wait3([&mut host, &mut anna, &mut ben], |_, a, b| {
        [a, b]
            .iter()
            .all(|app| app.group().voice && app.group().microphone)
    });
    // The new organizer keeps a fallback too, asking the next rider at once.
    wait3([&mut host, &mut anna, &mut ben], |_, _, b| {
        b.group().standby_offer
    });
    assert!(
        ben.group().fallback_consent.is_none(),
        "a takeover still uses the in-ride request"
    );
    ben.group_handover_answer(true).unwrap();
    wait3([&mut host, &mut anna, &mut ben], |_, a, b| {
        a.group().fallback == Some(b.group().me) && b.group().fallback == Some(b.group().me)
    });
    anna.group_leave();
    ben.group_leave();
}

#[test]
fn when_the_organizer_leaves_the_fallback_takes_over_with_voice_as_it_was() {
    let directories = [Directory::new(), Directory::new(), Directory::new()];
    let [mut host, mut anna, mut ben] = directories.each_ref().map(Directory::app);
    ride_in_three(&mut host, &mut anna, &mut ben);
    wait3([&mut host, &mut anna, &mut ben], |_, a, _| {
        a.group().standby_offer
    });
    anna.group_handover_answer(true).unwrap();
    let fallback = anna.group().me;
    wait3([&mut host, &mut anna, &mut ben], |h, a, b| {
        [h, a, b]
            .iter()
            .all(|app| app.group().fallback == Some(fallback))
    });
    speak(&mut anna, &mut host, true);
    speak(&mut ben, &mut host, false);
    // R187: leaving on purpose ends the organizer's links at once, voice's with them.
    host.group_leave();
    wait3([&mut host, &mut anna, &mut ben], |_, a, b| {
        a.group().host
            && b.group().organizer == fallback
            && [a, b].iter().all(|app| app.group().phase == Phase::Riding)
    });
    // R188: voice comes back on the new organizer's links, each microphone as it was.
    wait3([&mut host, &mut anna, &mut ben], |_, a, b| {
        a.group().voice && b.group().voice && !b.group().voice_pending
    });
    assert!(anna.group().microphone, "Anna's microphone was on");
    assert!(!ben.group().microphone, "Ben's microphone was off");
    anna.group_leave();
    ben.group_leave();
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
    // Every proposal used an epoch, the declined and cancelled ones and the standby offers
    // the organizer keeps making in the background (#426) too.
    let first = peer.group().epoch;
    assert!(first >= 4, "epoch {first}");
    for app in [&host, &peer] {
        let group = app.group();
        assert_eq!(group.root, root);
        assert_eq!(group.epoch, first);
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
    peer.group_admit(&late.group().reply).unwrap();
    wait(&mut peer, &mut late, |_, p| {
        p.group().can_enter() && p.trainer_connected()
    });
    assert_eq!(late.group().root, root);
    assert_eq!(late.group().epoch, first);
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
    let second = host.group().epoch;
    assert!(second > first, "epoch {second} after {first}");
    assert_eq!(peer.group().organizer, 0);
    assert_eq!(host.group().root, root);
    late.update(Duration::from_millis(16));
    assert!(late.ride_state().is_some());
    assert_eq!(late.group().epoch, second);
    // Losing the new host preserves both the old host's local recording and independent timing.
    host.group_leave();
    wait(&mut host, &mut peer, |_, p| p.group().phase == Phase::Ended);
    assert!(peer.ride_state().is_some());
}

fn assistance_fixture() -> (Directory, App, Snapshot, Instant) {
    let directory = Directory::new();
    let mut app = directory.app();
    assert_eq!(app.group_feedback(), AssistanceFeedback::default());
    app.group_open(None, options()).unwrap();
    app.group.as_mut().unwrap().task.abort();
    let config = app.ride_config(orqwa_domain::units::Percent(100.0), DescentMode::Coast);
    app.begin(
        crate::Ride::workout(Workout::ConstantPower(Watts(200.0)), config),
        None,
        crate::RouteRecords::default(),
        None,
        (None, None),
    );
    app.ride
        .as_mut()
        .unwrap()
        .ride
        .on_telemetry(&orqwa_domain::telemetry::Telemetry {
            power: Some(Watts(200.0)),
            cadence: Some(Rpm(85.0)),
            ..Default::default()
        });
    for _ in 0..20 {
        app.ride
            .as_mut()
            .unwrap()
            .ride
            .tick(Duration::from_millis(100));
    }
    let state = app.ride_state().unwrap();
    let now = Instant::now();
    let mut snapshot = Snapshot {
        phase: Phase::Riding,
        me: 1,
        organizer: 0,
        start: Some(now),
        ..Snapshot::default()
    };
    snapshot.roster[0] = Some(Rider {
        name: "Organizer".into(),
        mass: Kilograms(83.0),
        weight: Kilograms(75.0),
        avatar: Avatar::Female,
    });
    snapshot.generations[0] = 1;
    snapshot.states[0] = Some((
        RiderState {
            at: Duration::ZERO,
            distance: Meters(state.distance.0 + 30.0),
            speed: state.speed,
            power: Watts(200.0),
            cadence: Rpm(85.0),
        },
        now,
    ));
    app.group.as_mut().unwrap().effect_at = now;
    (directory, app, snapshot, now)
}

fn assistance_step(app: &mut App, snapshot: &mut Snapshot, start: Instant, ms: u64) {
    let elapsed = Duration::from_millis(ms);
    for (state, at) in snapshot.states.iter_mut().flatten() {
        state.at = elapsed;
        *at = start + elapsed;
    }
    app.update_group_motion_at(snapshot, start + elapsed);
}

#[test]
fn assistance_feedback_reports_applied_effects_without_changing_measurements() {
    let (_directory, mut app, mut snapshot, now) = assistance_fixture();
    let before = app.ride_state().unwrap();
    snapshot.states[0].as_mut().unwrap().0.distance = Meters(before.distance.0 + 12.0);
    assistance_step(&mut app, &mut snapshot, now, 100);
    let feedback = app.group_feedback();
    assert!(feedback.drafting > 0.0 && feedback.drafting < 1.0);
    assert!(feedback.together > 0.0 && feedback.together < 1.0);
    assert_eq!(
        app.ride_state().unwrap().telemetry.power,
        before.telemetry.power
    );
    assert_eq!(
        app.ride_state().unwrap().telemetry.cadence,
        before.telemetry.cadence
    );
    // A fast catch-up can cancel support, but must never brake the rider still behind.
    snapshot.states[0].as_mut().unwrap().0.speed = MetersPerSecond(0.6);
    assistance_step(&mut app, &mut snapshot, now, 1100);
    assert_eq!(app.group_feedback().together, 0.0);
    assert!(app.group_feedback().drafting > 0.0);
    let fresh = app.group_feedback();
    let effect = app.group.as_ref().unwrap().effect;
    assert!((app.group_feedback().together + effect.grade_offset().0 / 2.0).abs() < 1e-9);
    app.group.as_mut().unwrap().transition = Some((now + Duration::from_millis(1100), effect));
    assistance_step(&mut app, &mut snapshot, now, 1200);
    let blended = app.group.as_ref().unwrap().effect;
    assert!((app.group_feedback().drafting - (1.0 - blended.drag_share()) / 0.3).abs() < 1e-9);
    app.group.as_mut().unwrap().transition = None;
    app.group.as_mut().unwrap().peers = orqwa_session::group::Riders::default();
    snapshot.states[0].as_mut().unwrap().1 = now + Duration::from_millis(200);
    app.update_group_motion_at(&snapshot, now + Duration::from_millis(2000));
    let fading = app.group_feedback();
    assert!(fading.drafting > 0.0 && fading.drafting < fresh.drafting);
    app.group.as_mut().unwrap().peers = orqwa_session::group::Riders::default();
    app.update_group_motion_at(&snapshot, now + Duration::from_millis(4200));
    assert_eq!(app.group_feedback(), AssistanceFeedback::default());
    app.group.as_mut().unwrap().peers = orqwa_session::group::Riders::default();
    assistance_step(&mut app, &mut snapshot, now, 4300);
    assert!(app.group_feedback().drafting > 0.0);
    snapshot.assistance = Assistance {
        drafting: false,
        keep_together: false,
    };
    assistance_step(&mut app, &mut snapshot, now, 4400);
    assert_eq!(app.group_feedback(), AssistanceFeedback::default());
    snapshot.assistance = Assistance::default();
    assistance_step(&mut app, &mut snapshot, now, 4500);
    app.ride.as_mut().unwrap().paused = true;
    assert_eq!(app.group_feedback(), AssistanceFeedback::default());
    app.ride.as_mut().unwrap().paused = false;
    app.ride.as_mut().unwrap().finished = true;
    assert_eq!(app.group_feedback(), AssistanceFeedback::default());
    app.ride.as_mut().unwrap().finished = false;
    app.ride
        .as_mut()
        .unwrap()
        .ride
        .on_telemetry(&orqwa_domain::telemetry::Telemetry {
            power: Some(Watts(0.0)),
            ..Default::default()
        });
    assert_eq!(app.group_feedback(), AssistanceFeedback::default());
    assert_eq!(before.telemetry.power, Some(Watts(200.0)));
    assert_eq!(
        app.ride_state().unwrap().workout.unwrap().target_power,
        Some(Watts(200.0))
    );
    app.group_leave();
    assert_eq!(app.group_feedback(), AssistanceFeedback::default());
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
fn assistance_cancels_filtered_forces_on_crossing_the_group_middle() {
    let (_directory, mut app, mut snapshot, now) = assistance_fixture();
    app.ride.as_mut().unwrap().ride.jump_to(Meters(1000.0));
    let local = app.ride_state().unwrap();
    let peer = &mut snapshot.states[0].as_mut().unwrap().0;
    peer.distance = Meters(local.distance.0 + 100.0);
    peer.speed = local.speed;
    assistance_step(&mut app, &mut snapshot, now, 1000);
    assert!(app.group_feedback().together > 0.0);

    // A fresh position correction puts the local rider ahead. Old support must not survive
    // the filter for another frame, even though the new braking still ramps in gently.
    snapshot.states[0].as_mut().unwrap().0.distance = Meters(0.0);
    assistance_step(&mut app, &mut snapshot, now, 1100);
    assert!(app.group_feedback().together < 0.0);

    snapshot.states[0].as_mut().unwrap().0.distance = Meters(local.distance.0 + 100.0);
    assistance_step(&mut app, &mut snapshot, now, 1200);
    assert!(app.group_feedback().together > 0.0);

    snapshot.states[0].as_mut().unwrap().0.distance = local.distance;
    assistance_step(&mut app, &mut snapshot, now, 1300);
    assert_eq!(
        app.group_feedback().together,
        0.0,
        "the middle has no retained band"
    );
}

#[test]
fn a_new_unassisted_group_ride_does_not_inherit_the_previous_rides_effects() {
    let (from, to) = (Directory::new(), Directory::new());
    let (mut host, mut peer) = (from.app(), to.app());
    for (app, power) in [(&mut host, 300.0), (&mut peer, 120.0)] {
        app.connect_trainer(TrainerChoice::Fake(FakeRider {
            power: Watts(power),
            cadence: Rpm(85.0),
            heart: None,
        }))
        .unwrap();
    }
    let workout = host
        .save_workout(
            &orqwa_domain::workout::Plan {
                name: "Unassisted group regression".into(),
                steps: vec![orqwa_domain::workout::Step {
                    duration: Duration::from_secs(600),
                    target: orqwa_domain::workout::Target::Free,
                    cadence: None,
                }],
                ..Default::default()
            },
            None,
        )
        .unwrap();
    for enabled in [true, false] {
        host.group_open(None, options()).unwrap();
        wait(&mut host, &mut peer, |h, _| !h.group().invite.is_empty());
        peer.group_open(Some(host.group().invite), options())
            .unwrap();
        wait(&mut host, &mut peer, |_, p| !p.group().reply.is_empty());
        host.group_admit(&peer.group().reply).unwrap();
        wait(&mut host, &mut peer, |h, p| {
            h.group().roster.iter().flatten().count() == 2 && p.group().phase == Phase::Waiting
        });
        host.group_select(None, Some(&workout)).unwrap();
        wait(&mut host, &mut peer, |h, p| {
            h.group().prepared
                && p.group().prepared
                && h.trainer_connected()
                && p.trainer_connected()
        });
        host.group_assistance(enabled, enabled).unwrap();
        wait(&mut host, &mut peer, |h, p| {
            [h, p].iter().all(|a| {
                a.group().assistance
                    == Assistance {
                        drafting: enabled,
                        keep_together: enabled,
                    }
            })
        });
        host.group_ready(true).unwrap();
        peer.group_ready(true).unwrap();
        wait(&mut host, &mut peer, |h, _| h.group().can_start());
        host.group_start().unwrap();
        wait(&mut host, &mut peer, |h, p| {
            h.ride_state().is_some() && p.ride_state().is_some()
        });
        if enabled {
            // Establish material assistance before ending the first ride.
            host.ride.as_mut().unwrap().ride.jump_to(Meters(1000.0));
            wait(&mut host, &mut peer, |h, p| {
                h.group_feedback().together < -0.1 && p.group_feedback().together > 0.1
            });
        } else {
            wait(&mut host, &mut peer, |h, p| {
                h.ride_state().unwrap().telemetry.power == Some(Watts(300.0))
                    && p.ride_state().unwrap().telemetry.power == Some(Watts(120.0))
            });
            let mut solo = [&host, &peer]
                .map(|a| orqwa_physics::Motion::moving_at(a.ride_state().unwrap().speed));
            let mut distances = [&host, &peer].map(|a| a.ride_state().unwrap().distance.0);
            let step = Duration::from_millis(100);
            for _ in 0..20 {
                host.update(step);
                peer.update(step);
                for (id, app) in [&host, &peer].into_iter().enumerate() {
                    let state = app.ride_state().unwrap();
                    distances[id] += solo[id]
                        .step(
                            &app.rider_setup(),
                            state.telemetry.power.unwrap(),
                            orqwa_domain::units::GradePercent(0.0),
                            MetersPerSecond(0.0),
                            step,
                        )
                        .0;
                    assert_eq!(app.group_feedback(), AssistanceFeedback::default());
                    assert!((state.distance.0 - distances[id]).abs() < 1e-9);
                    assert!((state.speed.0 - solo[id].speed().0).abs() < 1e-9);
                    assert!(!app.ride.as_ref().unwrap().assisted);
                }
            }
        }
        host.group_leave();
        peer.group_leave();
        host.abort_ride();
        peer.abort_ride();
    }
}

#[test]
fn assistance_applied_to_the_ride_stays_steady_through_jitter_and_loss() {
    for gap in [75.0, 0.0, 3.0] {
        let (_directory, mut app, mut snapshot, start) = assistance_fixture();
        for _ in 0..1000 {
            app.ride
                .as_mut()
                .unwrap()
                .ride
                .tick(Duration::from_millis(100));
        }
        snapshot.states[0] = None;
        snapshot.assistance.keep_together = gap > 15.0;
        let mut packets = Vec::new();
        let mut low = [f64::INFINITY; 2];
        let mut high = [f64::NEG_INFINITY; 2];
        for ms in (0_u64..=30_000).step_by(20) {
            let elapsed = Duration::from_millis(ms);
            if ms > 0 {
                app.ride
                    .as_mut()
                    .unwrap()
                    .ride
                    .tick(Duration::from_millis(20));
            }
            let local = app.ride_state().unwrap();
            let sequence = ms / 200;
            if ms.is_multiple_of(200) && sequence % 9 != 3 {
                let delay = [90, 330, 160, 510, 110][usize::try_from(sequence % 5).unwrap()];
                let peer = orqwa_net::state::RiderId::new(0).unwrap();
                let state = RiderState {
                    at: elapsed,
                    distance: Meters(local.distance.0 + gap),
                    speed: local.speed,
                    power: Watts(200.0),
                    cadence: Rpm(85.0),
                };
                let bytes = state.datagram(peer).unwrap();
                let state = RiderState::from_datagram(&bytes, peer, elapsed).unwrap();
                packets.push((ms + delay, state));
                packets.sort_by_key(|p| p.0);
            }
            while packets.first().is_some_and(|p| p.0 <= ms) {
                let (arrival, state) = packets.remove(0);
                snapshot.states[0] = Some((state, start + Duration::from_millis(arrival)));
            }
            app.update_group_motion_at(&snapshot, start + elapsed);
            let feedback = app.group_feedback();
            assert_eq!(local.telemetry.power, Some(Watts(200.0)));
            if gap < 1.0 {
                assert_eq!(feedback.drafting, 0.0, "abreast at {ms} ms");
            }
            if ms >= 20_000 {
                for (id, value) in [feedback.drafting, feedback.together]
                    .into_iter()
                    .enumerate()
                {
                    low[id] = low[id].min(value);
                    high[id] = high[id].max(value);
                }
            }
        }
        for id in 0..2 {
            assert!(
                high[id] - low[id] < 0.02,
                "gap {gap}, effect {id}: applied effect fluctuates {low:?} .. {high:?}"
            );
        }
        if gap > 15.0 {
            assert!(
                low[1] > 0.5,
                "support remains effective at a steady 75 m gap"
            );
        } else if gap > 1.0 {
            assert!(low[0] > 0.9, "drafting remains effective behind a rider");
        }
    }
}

#[test]
fn assistance_compares_peers_at_the_live_locals_sample_time_before_the_next_physics_step() {
    let (_directory, mut app, mut snapshot, start) = assistance_fixture();
    let state = app.ride_state().unwrap();
    let sample = RiderState {
        at: Duration::from_secs(1),
        distance: state.distance,
        speed: state.speed,
        power: Watts(200.0),
        cadence: Rpm(85.0),
    };
    app.group
        .as_mut()
        .unwrap()
        .states
        .send_replace(Some(runtime::Local {
            state: sample,
            progress: None,
        }));
    snapshot.states[0] = Some((sample, start + Duration::from_secs(1)));
    app.update_group_motion_at(&snapshot, start + Duration::from_millis(1800));
    assert_eq!(
        app.group_feedback(),
        AssistanceFeedback::default(),
        "a slow frame must not invent a gap ahead of the unadvanced local rider"
    );
    assert_eq!(app.ride_state().unwrap().distance, state.distance);
}

#[test]
fn assistance_is_identical_for_organizers_friends_and_reassigned_slots() {
    for me in [0, 1, 7] {
        let (_directory, mut app, mut snapshot, now) = assistance_fixture();
        let peer = (usize::from(me) + 1) % MAX_RIDERS;
        let member = snapshot.roster[0].take();
        let state = snapshot.states[0].take();
        snapshot.roster[peer] = member.clone();
        snapshot.states[peer] = state;
        snapshot.generations[peer] = 1;
        snapshot.me = me;
        snapshot.roster[usize::from(me)] = member;
        snapshot.generations[usize::from(me)] = 1;
        let mut echo = state.unwrap();
        echo.0.distance = Meters(100_000.0);
        snapshot.states[usize::from(me)] = Some(echo);
        snapshot.organizer = me;
        assistance_step(&mut app, &mut snapshot, now, 1000);
        let support = app.group_feedback().together;
        assert!(
            support > 0.0 && support <= 0.2,
            "organizer receives smoothly applied support"
        );
        snapshot.organizer = u8::try_from(peer).unwrap();
        snapshot.epoch += 1;
        assistance_step(&mut app, &mut snapshot, now, 1000);
        assert!((app.group_feedback().together - support).abs() < 1e-9);
        // The peer is now behind: the organizer must also receive braking.
        app.ride.as_mut().unwrap().ride.jump_to(Meters(1000.0));
        snapshot.organizer = me;
        snapshot.states[peer].as_mut().unwrap().0.distance = Meters(0.0);
        snapshot.states[peer].as_mut().unwrap().0.speed = MetersPerSecond(0.6);
        assistance_step(&mut app, &mut snapshot, now, 4000);
        assert!(app.group_feedback().together < 0.0);
    }
}

#[test]
fn assistance_roster_changes_fade_but_local_stop_and_leave_clear_immediately() {
    let (_directory, mut app, mut snapshot, now) = assistance_fixture();
    assistance_step(&mut app, &mut snapshot, now, 1000);
    let before = app.group.as_ref().unwrap().effect.grade_offset().0;
    snapshot.states[0].as_mut().unwrap().0.distance = Meters(100_000.0);
    snapshot.states[0].as_mut().unwrap().0.speed = MetersPerSecond(150.0);
    assistance_step(&mut app, &mut snapshot, now, 1100);
    let sprint = app.group.as_ref().unwrap().effect.grade_offset().0;
    assert!((sprint - before).abs() <= 0.100_000_001);
    snapshot.away[0] = true;
    assistance_step(&mut app, &mut snapshot, now, 1200);
    let missing = app.group.as_ref().unwrap().effect.grade_offset().0;
    assert!(missing < 0.0 && missing > sprint);
    assistance_step(&mut app, &mut snapshot, now, 3200);
    assert_eq!(app.group_feedback(), AssistanceFeedback::default());
    snapshot.away[0] = false;
    snapshot.generations[0] += 1;
    assistance_step(&mut app, &mut snapshot, now, 3300);
    assert!((app.group_feedback().together - 0.05).abs() < 1e-9);
    for ms in [3400, 3600] {
        app.ride.as_mut().unwrap().paused = true;
        assistance_step(&mut app, &mut snapshot, now, ms);
        assert_eq!(
            app.group.as_ref().unwrap().effect,
            orqwa_physics::GroupEffect::default()
        );
        app.ride.as_mut().unwrap().paused = false;
        assistance_step(&mut app, &mut snapshot, now, ms + 100);
        assert!((app.group_feedback().together - 0.05).abs() < 1e-9);
    }
    app.ride.as_mut().unwrap().finished = true;
    assistance_step(&mut app, &mut snapshot, now, 3800);
    assert_eq!(
        app.group.as_ref().unwrap().effect,
        orqwa_physics::GroupEffect::default()
    );
    app.ride.as_mut().unwrap().finished = false;
    assistance_step(&mut app, &mut snapshot, now, 3900);
    app.group_leave();
    assert_eq!(app.group_feedback(), AssistanceFeedback::default());
}

#[test]
fn assistance_brings_different_power_riders_into_a_pack() {
    use orqwa_domain::units::GradePercent;
    use orqwa_physics::{Motion, RiderSetup};
    let dt = Duration::from_millis(100);
    let powers = [Watts(180.0), Watts(220.0), Watts(260.0)];
    let mut motion = [Motion::moving_at(MetersPerSecond(10.0)); 3];
    let mut solo = motion;
    let mut positions = [100.0, 160.0, 220.0];
    let mut solo_positions = positions;
    let mut applied = [GradePercent(0.0); 3];
    for _ in 0..3000 {
        let peers: [Option<orqwa_session::group::Rider>; MAX_RIDERS] = std::array::from_fn(|id| {
            positions
                .get(id)
                .map(|&distance| orqwa_session::group::Rider {
                    generation: 1,
                    distance: Meters(distance),
                    speed: motion[id].speed(),
                    power: powers[id],
                    cadence: Rpm(85.0),
                    age: Duration::ZERO,
                })
        });
        for id in 0..3 {
            let mut others = peers;
            others[id] = None;
            let target = orqwa_session::group::assistance(
                &others,
                Meters(positions[id]),
                motion[id].speed(),
                true,
                false,
                true,
            );
            let previous = applied[id];
            applied[id] = smooth_band(previous, target.grade_offset(), dt);
            assert!((applied[id].0 - previous.0).abs() <= 0.100_000_001);
            positions[id] += motion[id]
                .step(
                    &RiderSetup::default(),
                    powers[id],
                    applied[id],
                    MetersPerSecond(0.0),
                    dt,
                )
                .0;
            solo_positions[id] += solo[id]
                .step(
                    &RiderSetup::default(),
                    powers[id],
                    GradePercent(0.0),
                    MetersPerSecond(0.0),
                    dt,
                )
                .0;
        }
    }
    let spread = |p: [f64; 3]| {
        p.into_iter().fold(f64::NEG_INFINITY, f64::max)
            - p.into_iter().fold(f64::INFINITY, f64::min)
    };
    assert!(
        spread(positions) < 60.0,
        "different abilities stay together: {positions:?}"
    );
    assert!(
        spread(solo_positions) > 100.0,
        "the same riders separate without assistance"
    );
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
    host.group_admit(&peer.group().reply).unwrap();
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
    // With friends elsewhere, local-test addresses only reach nobody at another home.
    let mut mixed = options();
    mixed.setup = Setup::new(ConnectionMode::Mixed, false);
    host.group_open(None, mixed).unwrap();
    wait(&mut host, &mut peer, |h, _| !h.group().invite.is_empty());
    assert_eq!(host.group().reachability, Some(Reachability::Home));
    // The joiner is judged by the organizer's choice in the invitation, not its own.
    peer.group_open(Some(host.group().invite), options())
        .unwrap();
    wait(&mut host, &mut peer, |_, p| !p.group().reply.is_empty());
    assert_eq!(peer.group().reachability, Some(Reachability::Home));
    peer.group_leave();
    host.group_leave();
    // On one home network nobody joins from another home: there is nothing to warn about.
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut peer, |h, _| !h.group().invite.is_empty());
    assert_eq!(host.group().reachability, Some(Reachability::Together));
    peer.group_open(Some(host.group().invite), options())
        .unwrap();
    wait(&mut host, &mut peer, |_, p| !p.group().reply.is_empty());
    assert_eq!(peer.group().reachability, Some(Reachability::Together));
    peer.group_leave();
    host.group_leave();
}

fn sample_course() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../orqwa-gd/courses/twann-chasseral.oqc")
}

/// The course a friend with the `library` laid out by `prepare` rides when they join after the
/// organizer has chosen the sample course.
fn course_on_joining(to: &Directory, prepare: impl FnOnce(&Path)) -> PathBuf {
    let from = Directory::new();
    let mut host = from.app();
    let mut peer = to.app();
    std::fs::create_dir_all(peer.courses_dir()).unwrap();
    prepare(&peer.courses_dir());
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut peer, |h, _| !h.group().invite.is_empty());
    host.group_select(Some(sample_course()), None).unwrap();
    wait(&mut host, &mut peer, |h, _| h.group().selection.is_some());
    peer.group_open(Some(host.group().invite), options())
        .unwrap();
    wait(&mut host, &mut peer, |_, p| !p.group().reply.is_empty());
    host.group_admit(&peer.group().reply).unwrap();
    wait(&mut host, &mut peer, |_, p| p.group().course.is_some());
    let course = peer.group().course.unwrap();
    peer.group_leave();
    host.group_leave();
    course
}

fn library(to: &Directory) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(to.0.join("data/courses"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    files
}

#[test]
fn a_late_lobby_admission_receives_the_already_selected_course_without_a_local_copy() {
    let to = Directory::new();
    let downloaded = course_on_joining(&to, |_| {});
    assert!(downloaded.starts_with(to.0.join("data/courses")));
    assert_eq!(
        std::fs::read(sample_course()).unwrap(),
        std::fs::read(downloaded).unwrap()
    );
}

#[test]
fn a_friend_rides_their_own_copy_of_the_course_under_any_name_without_downloading_it() {
    use orqwa_storage::course;
    // #435: the organizer sent the course beforehand; the friend imported it under their own
    // name, and it has found its countries and drawn its map picture since.
    let to = Directory::new();
    let own = to.0.join("data/courses/Twann mit Freunden.oqc");
    let ridden = course_on_joining(&to, |library| {
        std::fs::copy(sample_course(), &own).unwrap();
        let mut manifest = course::read_manifest(&own).unwrap();
        manifest.name = "Twann mit Freunden".to_owned();
        manifest.countries = Some(Vec::new());
        course::rewrite_manifest(&own, &manifest).unwrap();
        course::set_preview(&own, b"PNG").unwrap();
        assert!(own.starts_with(library));
    });
    assert_eq!(ridden, own);
    assert_eq!(library(&to), [own]);
}

#[test]
fn a_changed_earlier_download_keeps_its_name_and_the_course_arrives_beside_it() {
    use orqwa_net::activity::{ContentId, ContentKind};
    use orqwa_storage::course;
    let id = ContentId::of(
        ContentKind::Course,
        &std::fs::read(sample_course()).unwrap(),
    )
    .unwrap();
    let to = Directory::new();
    let changed = to.0.join("data/courses").join(id.basename());
    let downloaded = course_on_joining(&to, |_| {
        // Received in an earlier ride, then ridden alone with map detail: another course now.
        std::fs::copy(sample_course(), &changed).unwrap();
        let mut manifest = course::read_manifest(&changed).unwrap();
        manifest.map_detail = vec!["switzerland".to_owned()];
        course::rewrite_manifest(&changed, &manifest).unwrap();
    });
    assert_ne!(downloaded, changed);
    assert_eq!(
        std::fs::read(sample_course()).unwrap(),
        std::fs::read(&downloaded).unwrap()
    );
    assert_eq!(
        course::read_manifest(&changed).unwrap().map_detail,
        ["switzerland"]
    );
    assert_eq!(library(&to).len(), 2);
}

#[test]
fn friends_see_what_the_organizer_chooses_until_it_is_shared() {
    use orqwa_storage::course;
    const WORKOUT: &str = "builtin:recovery-30";
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut peer = to.app();
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut peer, |h, _| !h.group().invite.is_empty());
    peer.group_open(Some(host.group().invite), options())
        .unwrap();
    wait(&mut host, &mut peer, |_, p| !p.group().reply.is_empty());
    host.group_admit(&peer.group().reply).unwrap();
    wait(&mut host, &mut peer, |_, p| {
        p.group().phase == Phase::Waiting
    });
    // #418: the organizer looks at a course and a workout; their friend sees both, without
    // anything downloaded, to talk it over.
    host.group_preview(Some(sample_course()), Some(WORKOUT))
        .unwrap();
    wait(&mut host, &mut peer, |_, p| {
        p.group()
            .preview
            .is_some_and(|shown| shown.preview.workout.is_some())
    });
    let shown = peer.group().preview.unwrap();
    let manifest = course::read_manifest(&sample_course()).unwrap();
    let previewed = shown.preview.course.as_ref().unwrap();
    assert_eq!(previewed.name, manifest.name);
    assert_eq!(previewed.profile, manifest.profile);
    let full = course::preview(&sample_course()).unwrap().len();
    assert!(
        (1..full).contains(&previewed.picture.len()),
        "the map picture at half its size"
    );
    assert_eq!(
        shown.preview.workout.as_ref().unwrap().steps,
        orqwa_workouts::load(WORKOUT).unwrap().steps
    );
    assert!(peer.group().selection.is_none());
    assert!(!to.0.join("data/courses").exists() || library(&to).is_empty());
    // Once shared, it needs no preview; looking at something else shows that instead ...
    host.group_select(Some(sample_course()), Some(WORKOUT))
        .unwrap();
    wait(&mut host, &mut peer, |_, p| {
        p.group().selection.is_some() && p.group().preview.is_none()
    });
    host.group_preview(Some(sample_course()), None).unwrap();
    wait(&mut host, &mut peer, |_, p| {
        p.group()
            .preview
            .is_some_and(|shown| shown.preview.workout.is_none())
    });
    // ... until the organizer is back at what everyone shares, or at nothing.
    host.group_preview(Some(sample_course()), Some(WORKOUT))
        .unwrap();
    wait(&mut host, &mut peer, |_, p| p.group().preview.is_none());
    host.group_preview(None, Some(WORKOUT)).unwrap();
    wait(&mut host, &mut peer, |_, p| p.group().preview.is_some());
    host.group_preview(None, None).unwrap();
    wait(&mut host, &mut peer, |_, p| p.group().preview.is_none());
    peer.group_leave();
    host.group_leave();
}

#[test]
fn joining_voice_during_a_solo_group_ride_preserves_riding_and_the_microphone_choice() {
    let directory = Directory::new();
    let idle_directory = Directory::new();
    let mut host = directory.app();
    let mut idle = idle_directory.app();
    host.connect_trainer(TrainerChoice::Fake(FakeRider {
        power: Watts(180.0),
        cadence: Rpm(85.0),
        heart: None,
    }))
    .unwrap();
    host.group_open(None, options()).unwrap();
    wait(&mut host, &mut idle, |h, _| {
        h.group().phase == Phase::Waiting
    });
    host.group_select(None, Some("builtin:recovery-30"))
        .unwrap();
    wait(&mut host, &mut idle, |h, _| {
        h.group().prepared && h.trainer_connected()
    });
    host.group_ready(true).unwrap();
    wait(&mut host, &mut idle, |h, _| h.group().can_start());
    host.group_start().unwrap();
    wait(&mut host, &mut idle, |h, _| h.ride_state().is_some());
    let controls = host.group.as_ref().unwrap().voice_controls.clone();
    controls.silent.store(true, Ordering::Release);
    for mic in [false, true] {
        // A requested local check must not prevent the public Join voice action either.
        controls.audio_test.store(1, Ordering::Release);
        host.group_voice(true, mic, false, 0.4).unwrap();
        wait(&mut host, &mut idle, |h, _| {
            h.group().voice && h.group().microphone == mic
        });
        assert_eq!(controls.audio_test.load(Ordering::Acquire), 0);
        assert_eq!(host.group().error, "");
        assert_eq!(host.group().roster.iter().flatten().count(), 1);
        let before = host.ride_state().unwrap().elapsed;
        host.update(Duration::from_secs(1));
        assert!(host.ride_state().unwrap().elapsed > before);
        host.group_voice(false, false, false, 0.4).unwrap();
        wait(&mut host, &mut idle, |h, _| !h.group().voice);
    }
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
#[allow(clippy::too_many_lines)] // one two-rider lobby checks shared rules, preparation and readiness together
fn shared_rules_retain_verified_content_invalidate_ready_and_freeze_at_start() {
    let from = Directory::new();
    let to = Directory::new();
    let mut host = from.app();
    let mut peer = to.app();
    host.profile.profile.ftp = Watts(200.0);
    peer.profile.profile.ftp = Watts(300.0);
    // Each rider's own weight and look reach the others (#422, #423).
    host.profile.profile.rider_mass = Kilograms(80.0);
    peer.profile.profile.rider_mass = Kilograms(68.0);
    peer.profile.profile.avatar = Avatar::Male;
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
    host.group_admit(&peer.group().reply).unwrap();
    wait(&mut host, &mut peer, |h, p| {
        h.group().roster.iter().flatten().count() == 2 && p.group().phase == Phase::Waiting
    });
    host.group_select(None, Some("builtin:recovery-30"))
        .unwrap();
    wait(&mut host, &mut peer, |h, p| {
        h.group().prepared && p.group().prepared
    });
    // Each hears that the other has prepared (#419), until they say they are ready.
    let done = Some((Stage::Preparing, 100));
    wait(&mut host, &mut peer, |h, p| {
        h.group().preparation[1] == done && p.group().preparation[0] == done
    });
    let received = peer.group().workout;
    assert!(host.group().assistance.drafting && host.group().assistance.keep_together);
    host.group_ready(true).unwrap();
    peer.group_ready(true).unwrap();
    wait(&mut host, &mut peer, |h, _| h.group().can_start());
    assert_eq!(
        host.group().preparation[1],
        None,
        "ready, nothing left to report"
    );
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
    let (joiner, organizer) = (&host.group_riders()[0], &peer.group_riders()[0]);
    assert!(
        host.group_view().distance.is_some() && peer.group_view().distance.is_some(),
        "each road view uses the local accepted echo alongside the remote rider"
    );
    assert_eq!(
        (joiner.weight, joiner.avatar),
        (Kilograms(68.0), Avatar::Male)
    );
    assert_eq!(
        (organizer.weight, organizer.avatar),
        (Kilograms(80.0), Avatar::Female)
    );
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
