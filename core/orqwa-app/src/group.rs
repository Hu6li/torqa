//! App group lobbies: owned background networking and local course preparation.

use crate::{App, AppError, AppEvent, DescentMode, Failure, GhostChoice, Kind, Workout};
use orqwa_domain::profile::Avatar;
use orqwa_domain::units::{Kilograms, Meters, MetersPerSecond, Rpm, Watts};
use orqwa_net::activity::{Assistance, Selection};
use orqwa_net::code::MAX_PASTE_BYTES;
pub use orqwa_net::link::ConnectOptions;
use orqwa_net::lobby::{RideProgress, Rider, WorkoutClock};
use orqwa_net::state::{MAX_RIDERS, RiderId, RiderState};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::Instant;
use tokio::sync::{mpsc, watch};
pub mod connection;
mod handover;
mod runtime;
#[cfg(test)]
mod tests;
use runtime::Command;

const _: () = assert!(orqwa_session::group::MAX_RIDERS == MAX_RIDERS);

/// A current authenticated group rider, interpolated locally without changing measurements.
#[derive(Clone, Debug)]
pub struct GroupRider {
    /// Stable roster slot for this membership.
    pub id: u8,
    /// Membership generation; a reused slot must get a new avatar and motion history.
    pub generation: u64,
    /// Display name from the admitted roster.
    pub name: String,
    /// Rider and bike mass, for avatar effort only.
    pub mass: Kilograms,
    /// The rider's own weight, for their power per kilogram (#422).
    pub weight: Kilograms,
    /// How the rider looks on the bike (#423).
    pub avatar: Avatar,
    /// Smoothed course distance.
    pub distance: Meters,
    /// Peer virtual speed.
    pub speed: MetersPerSecond,
    /// Peer measured power; never the local rider's recording or trainer input.
    pub power: Watts,
    /// Peer measured cadence for animation.
    pub cadence: Rpm,
}

/// Current connection/preparation phase, independent of presentation language.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    /// No network operation requested.
    #[default]
    Idle,
    /// Discovering or exchanging admission codes.
    Connecting,
    /// Waiting for activity selection or retry.
    Waiting,
    /// Hashing sources or receiving selected content.
    Downloading,
    /// Loading the local route and building its world.
    Preparing,
    /// Prepared locally; readiness can be announced.
    Ready,
    /// Waiting for the common start.
    Countdown,
    /// Shared start reached.
    Riding,
    /// Networking ended; local riding continues.
    Ended,
    /// Discovery or admission failed.
    Failed,
}
/// Planned organizer transition, independent of personal riding and recording.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Handover {
    /// No request is pending.
    #[default]
    Idle,
    /// The selected rider must explicitly accept or decline.
    Offered,
    /// The old organizer still owns the ride while replacement links connect.
    Preparing,
    /// The authorized commit is completing on the replacement links.
    Committing,
}
impl Handover {
    /// Stable presentation key for translated status text.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Offered => "offered",
            Self::Preparing => "preparing",
            Self::Committing => "committing",
        }
    }
}
impl Phase {
    /// Stable key for translated captions.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Connecting => "connecting",
            Self::Waiting => "waiting",
            Self::Downloading => "downloading",
            Self::Preparing => "preparing",
            Self::Ready => "ready",
            Self::Countdown => "countdown",
            Self::Riding => "riding",
            Self::Ended => "ended",
            Self::Failed => "failed",
        }
    }
}
/// Latest group view. Codes reveal riders' addresses, so this deliberately has no Debug.
#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)] // presentation of independent readiness and local audio gates
pub struct Snapshot {
    /// Current phase.
    pub phase: Phase,
    /// Whether this rider owns the lobby.
    pub host: bool,
    /// Private host invitation.
    pub invite: String,
    /// Private joiner reply while awaiting admission.
    pub reply: String,
    /// Whether friends at other homes can probably reach this rider as host: for the host from
    /// its invitation, for a joiner from its reply (offered organizer duties).
    pub reachability: Option<orqwa_net::code::Reachability>,
    /// Local assigned slot.
    pub me: u8,
    /// Original group identity, retained when transport invitations change.
    pub root: Option<orqwa_net::code::RideId>,
    /// Current committed authority epoch.
    pub epoch: u64,
    /// Organizer's stable roster slot.
    pub organizer: u8,
    /// Current planned transition.
    pub handover: Handover,
    /// Selected successor while a transition is pending.
    pub successor: Option<u8>,
    /// Current content identities and rules.
    pub selection: Option<Selection>,
    /// Fixed membership table without peer addresses or heart rate.
    pub roster: [Option<Rider>; MAX_RIDERS],
    /// Local membership epochs, advanced on each joined assignment before exposing state.
    pub generations: [u64; MAX_RIDERS],
    /// Organizer's current assistance choices, also before selecting an activity.
    pub assistance: Assistance,
    /// Readiness for the selected revision.
    pub ready: [bool; MAX_RIDERS],
    /// Whether local preparation completed.
    pub prepared: bool,
    /// Verified local course.
    pub course: Option<PathBuf>,
    /// Verified local normalized workout.
    pub workout: Option<PathBuf>,
    /// Library ID for the ride controls, independent of a temporary host transfer source.
    pub workout_id: String,
    /// Decoded workout retained in memory before countdown, avoiding file I/O in the ride loop.
    pub plan: Option<std::sync::Arc<orqwa_domain::workout::Plan>>,
    /// Local monotonic countdown target.
    pub start: Option<Instant>,
    /// Joined after countdown; preparation must not launch the ride implicitly.
    pub late: bool,
    /// An explicit entry request is awaiting the organizer's response.
    pub entering: bool,
    /// Fresh position and workout step accepted for this rider's first local tick.
    pub entry: Option<RideProgress>,
    /// Latest organizer-owned workout time, separate from personal recording and local pause.
    pub workout_clock: Option<WorkoutClock>,
    /// Latest peer samples and local arrival times.
    pub states: [Option<(RiderState, Instant)>; MAX_RIDERS],
    /// Diagnostic details under a translated error caption.
    pub error: String,
    /// Latest classified failure, emitted once through the application's normal failure events.
    pub failure: Option<Failure>,
    /// Monotonic occurrence number; dismissing an error must not resurface the same snapshot.
    pub failure_revision: u64,
    /// Preparation failed independently of later voice or admission errors.
    pub activity_failed: bool,
    /// Explicit voice listening consent.
    pub voice: bool,
    /// Native audio startup is pending; microphone controls stay disabled.
    pub voice_pending: bool,
    /// Live microphone transmission gate.
    pub microphone: bool,
    /// Local playback/transmission disabled.
    pub deafened: bool,
    /// Devices enumerated only after an explicit local request: (id, name, input, output).
    pub devices: Vec<(String, String, bool, bool)>,
    /// Selected local microphone ID; empty uses the system default.
    pub input_device: String,
    /// Selected local playback ID; empty uses the system default.
    pub output_device: String,
    /// Local playback gain, retained when devices reconnect.
    pub voice_volume: f32,
    /// Per-speaker playback mute on this computer.
    pub muted: [bool; MAX_RIDERS],
    /// Bytes stored for the current file, coalesced by the download worker.
    pub received: u64,
    /// Expected current-file size; zero before a transfer begins.
    pub total: u64,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            host: false,
            invite: String::new(),
            reply: String::new(),
            reachability: None,
            me: 0,
            root: None,
            epoch: 1,
            organizer: 0,
            handover: Handover::Idle,
            successor: None,
            selection: None,
            roster: std::array::from_fn(|_| None),
            generations: [0; MAX_RIDERS],
            assistance: Assistance::default(),
            ready: [false; MAX_RIDERS],
            prepared: false,
            course: None,
            workout: None,
            workout_id: String::new(),
            plan: None,
            start: None,
            late: false,
            entering: false,
            entry: None,
            workout_clock: None,
            states: [None; MAX_RIDERS],
            error: String::new(),
            failure: None,
            failure_revision: 0,
            activity_failed: false,
            voice: false,
            voice_pending: false,
            microphone: false,
            deafened: false,
            devices: Vec::new(),
            input_device: String::new(),
            output_device: String::new(),
            voice_volume: 0.6,
            muted: [false; MAX_RIDERS],
            received: 0,
            total: 0,
        }
    }
}
impl Snapshot {
    fn report(&mut self, kind: Kind, details: impl std::fmt::Display) {
        let failure = Failure::new(kind, details);
        self.error.clone_from(&failure.details);
        self.failure = Some(failure);
        self.failure_revision = self.failure_revision.saturating_add(1);
    }

    fn activity_error(&mut self, details: impl std::fmt::Display) {
        self.prepared = false;
        self.phase = Phase::Waiting;
        self.activity_failed = true;
        self.report(Kind::GroupActivity, details);
    }

    /// A failed activity can be retried even if a subsequent voice error was dismissed.
    #[must_use]
    pub fn can_retry(&self) -> bool {
        !self.host && self.activity_failed && self.phase == Phase::Waiting
    }

    /// A late joiner may explicitly enter after preparation, before starting a local ride.
    #[must_use]
    pub fn can_enter(&self) -> bool {
        self.late
            && self.prepared
            && self.phase == Phase::Ready
            && !self.entering
            && self.entry.is_none()
    }

    /// Organizer controls are available while the selected workout is running.
    #[must_use]
    pub fn can_control_workout(&self) -> bool {
        self.host
            && self.handover == Handover::Idle
            && self.phase == Phase::Riding
            && self
                .workout_clock
                .zip(self.plan.as_ref())
                .is_some_and(|(clock, plan)| clock.position(Instant::now()) < plan.duration())
    }

    /// An active organizer can offer authority to another prepared member.
    #[must_use]
    pub fn can_handover(&self) -> bool {
        self.host
            && self.prepared
            && self.phase == Phase::Riding
            && self.handover == Handover::Idle
            && self
                .roster
                .iter()
                .enumerate()
                .any(|(id, rider)| id != usize::from(self.me) && rider.is_some() && self.ready[id])
    }

    /// The shared plan has run its last step; course riding can still continue.
    #[must_use]
    pub fn workout_finished(&self) -> bool {
        self.workout_clock
            .zip(self.plan.as_ref())
            .is_some_and(|(clock, plan)| clock.position(Instant::now()) >= plan.duration())
    }

    /// Only the prepared host with every admitted participant ready can start.
    #[must_use]
    pub fn can_start(&self) -> bool {
        self.host
            && self.prepared
            && self.start.is_none()
            && self.selection.is_some()
            && self
                .roster
                .iter()
                .enumerate()
                .filter(|(_, r)| r.is_some())
                .all(|(id, _)| self.ready[id])
    }
}
pub(crate) struct Running {
    commands: mpsc::Sender<Command>,
    snapshot: watch::Receiver<Snapshot>,
    task: tokio::task::JoinHandle<()>,
    states: watch::Sender<Option<runtime::Local>>,
    preparing: Option<(u64, u64)>,
    prepared: u64,
    launched: bool,
    launching: bool,
    trainer_connected: bool,
    workout_id: String,
    voice_controls: Arc<VoiceControls>,
    peers: orqwa_session::group::Riders,
    reported_failure: u64,
    authority: u64,
    effect: orqwa_physics::GroupEffect,
    transition: Option<(Instant, orqwa_physics::GroupEffect)>,
}
#[derive(Default)]
struct VoiceControls {
    generation: AtomicU64,
    port: Mutex<Option<orqwa_net::voice::VoicePort>>,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.voice_controls
            .generation
            .fetch_add(1, Ordering::AcqRel);
        if let Some(port) = self
            .voice_controls
            .port
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            port.enable(false);
        }
        self.task.abort();
    }
}
fn error(details: impl std::fmt::Display) -> AppError {
    AppError::Reported(Failure::new(Kind::GroupRide, details))
}

impl App {
    /// Reads current group state without opening network/audio devices.
    #[must_use]
    pub fn group(&self) -> Snapshot {
        self.group.as_ref().map_or_else(Snapshot::default, |g| {
            let mut snapshot = g.snapshot.borrow().clone();
            if !g.workout_id.is_empty() {
                snapshot.workout_id.clone_from(&g.workout_id);
            }
            if snapshot.late
                && snapshot.entry.is_some()
                && !g.launched
                && snapshot.phase == Phase::Riding
            {
                snapshot.phase = Phase::Ready;
            }
            snapshot
        })
    }
    /// Creates a lobby or answers an invite, with explicit discovery/address choices.
    ///
    /// # Errors
    /// Returns [`AppError`] when already riding/grouped, on excessive input or entropy failure.
    pub fn group_open(
        &mut self,
        invite: Option<String>,
        options: ConnectOptions,
    ) -> Result<(), AppError> {
        if self.group.is_some() || self.ride.is_some() {
            return Err(error("Leave the current group or finish the ride first."));
        }
        if invite.as_ref().is_some_and(|s| s.len() > MAX_PASTE_BYTES) {
            return Err(error("The invitation is too large."));
        }
        let (commands, receive) = mpsc::channel(8);
        let (send, snapshot) = watch::channel(Snapshot {
            phase: Phase::Connecting,
            ..Snapshot::default()
        });
        let (states, local) = watch::channel(None);
        let rider = &self.profile.profile;
        let voice_controls = Arc::new(VoiceControls::default());
        let token = orqwa_net::code::RideId::generate().map_err(error)?;
        let request = runtime::Request {
            voice_controls: Arc::clone(&voice_controls),
            invite,
            options,
            rider: Rider {
                name: rider.name.clone(),
                mass: Kilograms(rider.rider_mass.0 + rider.bike_mass.0),
                weight: rider.rider_mass,
                avatar: rider.avatar,
            },
            data: self.data_dir.clone(),
            temporary: self.cache_dir.join(format!("group-{:x?}", token.0)),
        };
        let task = self
            .runtime
            .spawn(runtime::run(request, receive, send, local));
        self.group = Some(Running {
            commands,
            snapshot,
            task,
            states,
            preparing: None,
            prepared: 0,
            launched: false,
            launching: false,
            trainer_connected: self.trainer_connected(),
            workout_id: String::new(),
            voice_controls,
            peers: orqwa_session::group::Riders::default(),
            reported_failure: 0,
            authority: 1,
            effect: orqwa_physics::GroupEffect::default(),
            transition: None,
        });
        Ok(())
    }
    fn group_command(&self, command: Command) -> Result<(), AppError> {
        self.group
            .as_ref()
            .ok_or_else(|| error("Open a lobby first."))?
            .commands
            .try_send(command)
            .map_err(|_| error("The group is busy or disconnected. Retry or leave the lobby."))
    }
    /// Leaves networking/audio while preserving any local ride.
    pub fn group_leave(&mut self) {
        if let Some(active) = &mut self.ride {
            active.ride.sync_group_workout(None);
            active
                .ride
                .set_group_effect(orqwa_physics::GroupEffect::default());
        }
        if self.group.as_ref().is_some_and(|g| g.preparing.is_some()) && self.ride.is_none() {
            self.cancel_loading();
        }
        self.group = None;
    }
    /// Admits a privately exchanged reply.
    ///
    /// # Errors
    /// Returns [`AppError`] for excessive input or a full/stopped command queue.
    pub fn group_admit(&self, reply: String) -> Result<(), AppError> {
        if reply.len() > MAX_PASTE_BYTES {
            return Err(error("The reply is too large."));
        }
        self.group_command(Command::Admit(reply))
    }
    /// Selects a 3D course and/or a structured workout from the rider's libraries.
    ///
    /// # Errors
    /// Returns [`AppError`] for empty selection, unknown workout or a stopped/full queue.
    pub fn group_select(
        &mut self,
        course: Option<PathBuf>,
        workout: Option<&str>,
    ) -> Result<(), AppError> {
        let plan = workout
            .map(|id| {
                self.workouts()
                    .into_iter()
                    .find(|w| w.id == id)
                    .map(|w| w.plan)
                    .ok_or_else(|| error("The selected workout is unavailable."))
            })
            .transpose()?;
        if course.is_none() && plan.is_none() {
            return Err(error("Choose a course or a structured workout."));
        }
        self.group_command(Command::Select(course, plan))?;
        if let Some(group) = &mut self.group {
            workout.unwrap_or("").clone_into(&mut group.workout_id);
        }
        Ok(())
    }

    /// Changes the organizer's independent drafting and keep-together choices before start.
    /// A shared change invalidates readiness, while already verified content is retained.
    ///
    /// # Errors
    /// Returns [`AppError`] when not organizing, after countdown, or if the queue is unavailable.
    pub fn group_assistance(&self, drafting: bool, keep_together: bool) -> Result<(), AppError> {
        let status = self.group();
        if !status.host || status.start.is_some() {
            return Err(error(
                "Only the organizer can change ride settings before the start.",
            ));
        }
        self.group_command(Command::Assistance(Assistance {
            drafting,
            keep_together,
        }))
    }

    /// Up to seven current group riders for the road, map and profile. Stale state, departed
    /// memberships and mismatched reused slots are omitted rather than left on screen.
    #[must_use]
    pub fn group_riders(&self) -> Vec<GroupRider> {
        let Some(group) = &self.group else {
            return Vec::new();
        };
        let snapshot = group.snapshot.borrow();
        if snapshot.phase != Phase::Riding || !group.launched || self.ride.is_none() {
            return Vec::new();
        }
        group
            .peers
            .view(Instant::now())
            .iter()
            .enumerate()
            .filter_map(|(id, peer)| {
                let peer = peer.as_ref()?;
                let rider = snapshot.roster[id].as_ref()?;
                if id == usize::from(snapshot.me) || snapshot.generations[id] != peer.generation {
                    return None;
                }
                Some(GroupRider {
                    id: u8::try_from(id).ok()?,
                    generation: peer.generation,
                    name: rider.name.clone(),
                    mass: rider.mass,
                    weight: rider.weight,
                    avatar: rider.avatar,
                    distance: peer.distance,
                    speed: peer.speed,
                    power: peer.power,
                    cadence: peer.cadence,
                })
            })
            .collect()
    }
    /// Sets readiness after preparation; becoming ready also requires a connected trainer.
    ///
    /// # Errors
    /// Returns [`AppError`] for missing trainer or unavailable queue.
    pub fn group_ready(&self, ready: bool) -> Result<(), AppError> {
        if ready && !self.trainer_connected() {
            return Err(AppError::NoTrainer);
        }
        self.group_command(Command::Ready(ready))
    }
    /// Requests the host's shared five-second countdown.
    ///
    /// # Errors
    /// Returns [`AppError`] when participants are not all ready or the queue is unavailable.
    pub fn group_start(&self) -> Result<(), AppError> {
        if !self.group().can_start() {
            return Err(error("Wait until every participant is ready."));
        }
        self.group_command(Command::Start)
    }
    /// Requests entry near the organizer at the current workout step, after preparation.
    ///
    /// # Errors
    /// Returns [`AppError`] without a trainer, before preparation or if the group has ended.
    pub fn group_enter(&self) -> Result<(), AppError> {
        if !self.trainer_connected() {
            return Err(AppError::NoTrainer);
        }
        if !self.group().can_enter() {
            return Err(error("Prepare the activity before joining the ride."));
        }
        self.group_command(Command::Enter)
    }

    /// Pauses or resumes the group's workout clock; personal safety pause remains local.
    ///
    /// # Errors
    /// Returns [`AppError`] unless this rider is the organizer of an active shared workout.
    pub fn group_workout_pause(&self, paused: bool) -> Result<(), AppError> {
        if !self.group().can_control_workout() {
            return Err(error("Only the organizer can control the shared workout."));
        }
        self.group_command(Command::WorkoutPause(paused))
    }

    pub(crate) fn group_skip_step(&self) -> Option<bool> {
        let snapshot = self.group();
        if !matches!(snapshot.phase, Phase::Countdown | Phase::Riding) || snapshot.plan.is_none() {
            return None;
        }
        if !snapshot.can_control_workout() {
            return Some(false);
        }
        let Some(clock) = snapshot.workout_clock else {
            return Some(false);
        };
        let elapsed = clock.position(Instant::now());
        let Some(next) = snapshot
            .plan
            .as_ref()
            .and_then(|plan| plan.at(elapsed, self.profile.profile.ftp))
        else {
            return Some(false);
        };
        Some(
            self.group_command(Command::WorkoutSkip(elapsed + next.left))
                .is_ok(),
        )
    }

    /// Retries a failed content transfer/preparation.
    ///
    /// # Errors
    /// Returns [`AppError`] if the command queue is unavailable.
    pub fn group_retry(&mut self) -> Result<(), AppError> {
        if let Some(g) = &mut self.group {
            g.preparing = None;
            g.prepared = 0;
        }
        self.group_command(Command::Retry)
    }
    /// Removes an unavailable participant on the host.
    ///
    /// # Errors
    /// Returns [`AppError`] for invalid slots or unavailable queue.
    pub fn group_remove(&self, id: u8) -> Result<(), AppError> {
        self.group_command(Command::Remove(RiderId::new(id).map_err(error)?))
    }
    /// Offers organizer duties to a prepared connected rider, who must accept.
    ///
    /// # Errors
    /// Returns [`AppError`] unless locally riding as the current organizer.
    pub fn group_handover(&self, id: u8) -> Result<(), AppError> {
        if !self.group().can_handover() || self.ride.is_none() {
            return Err(error("Only the riding organizer can offer a handover."));
        }
        self.group_command(Command::Handover(RiderId::new(id).map_err(error)?))
    }
    /// Accepts or declines the current offer; accepting permits pinned peer connections.
    ///
    /// # Errors
    /// Returns [`AppError`] without an active local ride or command queue.
    pub fn group_handover_answer(&self, accepted: bool) -> Result<(), AppError> {
        if self.ride.is_none() {
            return Err(error(
                "Join the ride before answering the organizer request.",
            ));
        }
        self.group_command(Command::HandoverAnswer(accepted))
    }
    /// Cancels a pending handover before the old organizer releases authority.
    ///
    /// # Errors
    /// Returns [`AppError`] for an unavailable command queue.
    pub fn group_handover_cancel(&self) -> Result<(), AppError> {
        self.group_command(Command::HandoverCancel)
    }
    /// Configures opted-in voice. First activation starts with the microphone muted.
    ///
    /// # Errors
    /// Returns [`AppError`] for invalid volume or unavailable queue.
    pub fn group_voice(
        &self,
        on: bool,
        mic: bool,
        deaf: bool,
        volume: f32,
    ) -> Result<(), AppError> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(error("Voice volume is invalid."));
        }
        let controls = &self
            .group
            .as_ref()
            .ok_or_else(|| error("Open a lobby first."))?
            .voice_controls;
        let generation = controls
            .generation
            .try_update(Ordering::AcqRel, Ordering::Acquire, |g| g.checked_add(1))
            .map_err(|_| error("Voice activation exhausted."))?
            + 1;
        if let Some(port) = controls
            .port
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            if !on {
                port.enable(false);
            } else if !mic || deaf {
                let _ = port.microphone(false);
                if deaf {
                    port.deafen(true);
                }
            }
        }
        self.group_command(Command::Voice(generation, on, mic, deaf, volume))
    }
    /// Changes playback gain without changing microphone consent or activation.
    ///
    /// # Errors
    /// Returns [`AppError`] for invalid gain or an unavailable queue.
    pub fn group_voice_volume(&self, volume: f32) -> Result<(), AppError> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(error("Voice volume is invalid."));
        }
        self.group_command(Command::Volume(volume))
    }
    /// Enumerates local audio devices without opening microphone capture.
    ///
    /// # Errors
    /// Returns [`AppError`] if the queue is unavailable.
    pub fn group_voice_devices(&self) -> Result<(), AppError> {
        self.group_command(Command::Devices)
    }
    /// Selects native devices, restarting opted-in voice with the microphone muted.
    ///
    /// # Errors
    /// Returns [`AppError`] for excessive device IDs or an unavailable queue.
    pub fn group_voice_choose(&self, input: String, output: String) -> Result<(), AppError> {
        if input.len() > 512 || output.len() > 512 {
            return Err(error("The audio device ID is too long."));
        }
        let controls = &self
            .group
            .as_ref()
            .ok_or_else(|| error("Open a lobby first."))?
            .voice_controls;
        let generation = controls
            .generation
            .try_update(Ordering::AcqRel, Ordering::Acquire, |g| g.checked_add(1))
            .map_err(|_| error("Voice activation exhausted."))?
            + 1;
        if let Some(port) = controls
            .port
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            port.enable(false);
        }
        self.group_command(Command::ChooseDevices(generation, input, output))
    }
    /// Mutes one speaker locally without changing anyone else's audio.
    ///
    /// # Errors
    /// Returns [`AppError`] for invalid slots or an unavailable queue.
    pub fn group_voice_mute(&self, id: u8, muted: bool) -> Result<(), AppError> {
        self.group_command(Command::Mute(RiderId::new(id).map_err(error)?, muted))
    }
    pub(crate) fn check_group_start(&self) -> Result<(), AppError> {
        if self.group.as_ref().is_some_and(|g| !g.launching) {
            return Err(error("Leave the group before starting a solo ride."));
        }
        Ok(())
    }
    pub(crate) fn poll_group(&mut self, events: &mut Vec<AppEvent>) {
        let Some(group) = &self.group else {
            return;
        };
        let snapshot = group.snapshot.borrow().clone();
        let connected = self.trainer_connected();
        if group.trainer_connected
            && !connected
            && !group.launched
            && snapshot.ready[usize::from(snapshot.me)]
        {
            let _ = self.group_command(Command::Ready(false));
        }
        if let Some(group) = &mut self.group {
            group.trainer_connected = connected;
        }
        self.report_group_failure(&snapshot, events);
        let Some(group) = &self.group else {
            return;
        };
        let revision = snapshot.selection.as_ref().map_or(0, |s| s.revision);
        if group.preparing.is_some_and(|(old, _)| old != revision)
            || (matches!(snapshot.phase, Phase::Ended | Phase::Failed)
                && group.preparing.is_some()
                && self.ride.is_none())
        {
            self.cancel_loading();
            if let Some(group) = &mut self.group {
                group.preparing = None;
                group.prepared = 0;
            }
        }
        let Some(group) = &self.group else {
            return;
        };
        if snapshot.phase == Phase::Preparing && group.prepared != revision {
            if let Some(course) = &snapshot.course {
                if group.preparing.is_none_or(|(r, _)| r != revision) {
                    events.push(AppEvent::CoursesChanged);
                    self.open_shared_course(course.clone());
                    if let Some(g) = &mut self.group {
                        g.preparing = Some((revision, self.load));
                    }
                } else if group.preparing == Some((revision, self.load)) {
                    if events.iter().any(|e| matches!(e, AppEvent::RouteLoaded(_))) {
                        self.build_world();
                    }
                    if self.world.is_some() {
                        self.group_prepared(revision);
                    }
                    if events.iter().any(|e| matches!(e, AppEvent::Error(_))) {
                        let _=self.group_command(Command::Failure("The course could not be prepared. Retry the download or choose another course.".into()));
                    }
                }
            } else {
                self.group_prepared(revision);
            }
        }
        self.launch_group(&snapshot, revision, events);
        if snapshot.phase == Phase::Ready
            && snapshot.prepared
            && self.group.as_ref().is_some_and(|g| g.prepared != revision)
        {
            self.group_prepared(revision);
        }
        self.sync_group_workout(&snapshot);
        self.update_group_motion(&snapshot);
        self.publish_group_state(&snapshot);
    }

    fn report_group_failure(&mut self, snapshot: &Snapshot, events: &mut Vec<AppEvent>) {
        if let Some(group) = &mut self.group
            && group.reported_failure != snapshot.failure_revision
        {
            group.reported_failure = snapshot.failure_revision;
            if let Some(failure) = &snapshot.failure {
                events.push(AppEvent::Error(failure.clone()));
            }
        }
    }

    fn launch_group(&mut self, snapshot: &Snapshot, revision: u64, events: &mut Vec<AppEvent>) {
        if (!snapshot.late || snapshot.entry.is_some())
            && self.trainer_connected()
            && snapshot.start.is_some_and(|at| Instant::now() >= at)
            && self
                .group
                .as_ref()
                .is_some_and(|g| !g.launched && g.prepared == revision)
        {
            if let Some(g) = &mut self.group {
                g.launched = true;
                g.launching = true;
            }
            let on_course = snapshot.course.is_some();
            let result = if let Some(plan) = &snapshot.plan {
                self.start_workout(
                    Workout::Structured {
                        plan: std::sync::Arc::clone(plan),
                        ftp: self.profile.profile.ftp,
                    },
                    on_course,
                    DescentMode::Coast,
                    snapshot.selection.as_ref().map_or("", |s| s.name.as_str()),
                )
            } else {
                self.start_ride(
                    self.profile.profile.default_difficulty,
                    DescentMode::Coast,
                    &GhostChoice::None,
                )
            };
            if let Some(g) = &mut self.group {
                g.launching = false;
            }
            match result {
                Ok(()) => {
                    if let Some(active) = &mut self.ride {
                        active.assisted =
                            snapshot.assistance.drafting || snapshot.assistance.keep_together;
                        if let Some(entry) = snapshot.entry {
                            let near = Meters((entry.distance.0 - 5.0).max(0.0));
                            let entered = active.ride.enter_group(near, entry.workout);
                            debug_assert!(
                                entered,
                                "validated entry must match the prepared activity"
                            );
                            let distance = active.ride.state().distance;
                            active.milestones.skip_to(distance);
                            let climbs = active
                                .ride
                                .route()
                                .map_or(&[][..], orqwa_routes::Route::climbs);
                            active.next_climb = climbs
                                .iter()
                                .position(|climb| climb.start.0 >= distance.0)
                                .unwrap_or(climbs.len());
                        }
                    }
                    events.push(AppEvent::GroupRideStarted {
                        on_course,
                        workout: snapshot.workout.is_some(),
                    });
                }
                Err(e) => events.push(AppEvent::Error(Failure::from(e))),
            }
        }
    }

    fn sync_group_workout(&mut self, snapshot: &Snapshot) {
        if let Some(active) = &mut self.ride {
            if matches!(snapshot.phase, Phase::Ended | Phase::Failed) {
                active.ride.sync_group_workout(None);
            } else if let Some(clock) = snapshot.workout_clock {
                active
                    .ride
                    .sync_group_workout(Some((clock.position(Instant::now()), clock.paused)));
            }
        }
    }

    fn update_group_motion(&mut self, snapshot: &Snapshot) {
        let Some(group) = &mut self.group else {
            return;
        };
        if group.authority != snapshot.epoch {
            group.authority = snapshot.epoch;
            group.transition = Some((Instant::now(), group.effect));
            group.peers = orqwa_session::group::Riders::default();
        }
        for id in 0..MAX_RIDERS {
            if id == usize::from(snapshot.me)
                || snapshot.roster[id].is_none()
                || snapshot.phase != Phase::Riding
            {
                group.peers.clear(id);
            } else if let Some((state, at)) = snapshot.states[id] {
                group.peers.observe(
                    id,
                    snapshot.generations[id],
                    orqwa_session::group::Sample {
                        elapsed: state.at,
                        distance: state.distance,
                        speed: state.speed,
                        power: state.power,
                        cadence: state.cadence,
                    },
                    at,
                );
            }
        }
        if let Some(active) = &mut self.ride {
            let state = active.ride.state();
            let pedaling = !active.paused
                && !active.finished
                && state.telemetry.power.is_some_and(|p| p.0 > 0.0)
                && state.telemetry.cadence.is_none_or(|c| c.0 > 0.0);
            let rules = snapshot.assistance;
            let effect = orqwa_session::group::assistance(
                &group.peers.view(Instant::now()),
                state.distance,
                state.speed,
                pedaling,
                (snapshot.me != snapshot.organizer).then_some(usize::from(snapshot.organizer)),
                rules.drafting,
                rules.keep_together,
            );
            let effect = if pedaling {
                group.transition.map_or(effect, |(at, previous)| {
                    blend(previous, effect, at.elapsed())
                })
            } else {
                orqwa_physics::GroupEffect::default()
            };
            if group
                .transition
                .is_some_and(|(at, _)| at.elapsed() >= std::time::Duration::from_secs(2))
            {
                group.transition = None;
            }
            group.effect = effect;
            active.ride.set_group_effect(effect);
        }
    }
    fn publish_group_state(&self, snapshot: &Snapshot) {
        if let Some(g) = &self.group
            && let Some(state) = self.ride_state()
        {
            let at = snapshot.start.map_or(state.elapsed, |at| at.elapsed());
            let stopped = self
                .ride
                .as_ref()
                .is_some_and(|ride| ride.paused || ride.finished);
            g.states.send_replace(Some(runtime::Local {
                state: RiderState {
                    at,
                    distance: state.distance,
                    speed: if stopped {
                        MetersPerSecond(0.0)
                    } else {
                        state.speed
                    },
                    power: state.telemetry.power.unwrap_or_default(),
                    cadence: if stopped {
                        Rpm(0.0)
                    } else {
                        state.telemetry.cadence.unwrap_or_default()
                    },
                },
                progress: self
                    .ride
                    .as_ref()
                    .filter(|ride| !ride.finished && !ride.paused)
                    .map(|active| RideProgress {
                        distance: state.distance,
                        workout: active.ride.workout_elapsed(),
                    }),
            }));
        } else if let Some(group) = &self.group {
            group.states.send_replace(None);
        }
    }
    fn group_prepared(&mut self, revision: u64) {
        if let Some(g) = &mut self.group
            && g.commands.try_send(Command::Prepared(revision)).is_ok()
        {
            g.prepared = revision;
            g.preparing = None;
        }
    }
    pub(crate) fn shared_workouts(&self) -> Vec<orqwa_workouts::Entry> {
        std::fs::read_dir(self.data_dir.join("shared-workouts"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "oqw")
                    || entry.metadata().ok()?.len() > orqwa_net::activity::MAX_WORKOUT_BYTES
                {
                    return None;
                }
                let plan = orqwa_domain::workout::WorkoutParser::parse(
                    &orqwa_net::workout::SharedWorkoutParser,
                    &std::fs::read(&path).ok()?,
                    "",
                )
                .ok()?;
                Some(orqwa_workouts::Entry {
                    id: path.display().to_string(),
                    plan,
                })
            })
            .collect()
    }
}

fn blend(
    from: orqwa_physics::GroupEffect,
    to: orqwa_physics::GroupEffect,
    elapsed: std::time::Duration,
) -> orqwa_physics::GroupEffect {
    let t = (elapsed.as_secs_f64() / 2.0).clamp(0.0, 1.0);
    let weight = t * t * (3.0 - 2.0 * t);
    orqwa_physics::GroupEffect::new(
        from.drag_share() + (to.drag_share() - from.drag_share()) * weight,
        orqwa_domain::units::GradePercent(
            from.grade_offset().0 + (to.grade_offset().0 - from.grade_offset().0) * weight,
        ),
    )
}
