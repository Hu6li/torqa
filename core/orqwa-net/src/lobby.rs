//! Eight-rider star lobby with bounded controls, coalesced state and owned background tasks.
//! In app lobbies a rider whose connection is lost keeps their seat for
//! [`RECONNECT_WINDOW`] and reconnects by themselves, without new codes (#434).

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use orqwa_domain::profile::Avatar;
use orqwa_domain::units::Kilograms;
use quinn::{RecvStream, SendStream};
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::JoinSet;
use tokio::time::{Instant, interval, timeout, timeout_at};

use crate::code::{Invite, Reply, RideId, RouteHash, valid_name};
use crate::link::{Connection, Host, HostEvent, Joiner, LinkError, Resume};
use crate::state::{
    MAX_RIDERS, Membership, RiderId, RiderState, STATE_EVERY, StateError, StateInbox,
};
use crate::voice::{self, VoiceHandle, VoiceOwner, VoicePort};
use crate::wire::{self, Message};

mod activity;
pub(crate) mod entry;
mod latency;
mod replacement;
pub(crate) mod timing;
use activity::Activities;
pub use activity::SharedActivity;
pub use entry::RideProgress;
use latency::Latency;
use replacement::Prepared;
pub use replacement::Replacement;
use timing::Timeline;
pub use timing::WorkoutClock;

const CONTROL_CAPACITY: usize = 32;
/// Simultaneous transfers to one rider: the selected activity and the organizer's preview.
const MAX_TRANSFERS: usize = 2;
const WRITE_WAIT: Duration = Duration::from_secs(2);
/// How long the organizer holds the seat of a rider whose connection was lost (#434).
pub const RECONNECT_WINDOW: Duration = Duration::from_mins(5);
/// How long that rider keeps trying to reconnect: longer by the time the organizer may take
/// to notice the loss, so they give up only once the seat is released (#536).
pub const RECONNECT_TRYING: Duration =
    Duration::from_secs(RECONNECT_WINDOW.as_secs() + crate::tls::IDLE_TIMEOUT.as_secs());
/// The pause between a joiner's attempts to reconnect.
const RECONNECT_EVERY: Duration = Duration::from_secs(5);
type States = [Option<RiderState>; MAX_RIDERS];
/// A fixed-size roster, indexed by the host-assigned rider number.
pub type Roster = [Option<Rider>; MAX_RIDERS];

/// Public rider metadata. Heart rate and peer addresses are never part of the roster.
#[derive(Clone, Debug, PartialEq)]
pub struct Rider {
    /// Bounded printable UTF-8 name.
    pub name: String,
    /// Rider plus bicycle mass in kilograms.
    pub mass: Kilograms,
    /// The rider's own weight, without the bicycle, for others to see their power per
    /// kilogram (#422); never more than `mass`.
    pub weight: Kilograms,
    /// How the rider looks on the bike in the others' worlds (#423).
    pub avatar: Avatar,
}
impl Rider {
    pub(crate) fn validate(&self) -> Result<(), LobbyError> {
        let finite = self.mass.0.is_finite() && self.weight.0.is_finite();
        if !valid_name(&self.name)
            || !finite
            || !(20.0..=300.0).contains(&self.mass.0)
            || !(20.0..=self.mass.0).contains(&self.weight.0)
        {
            return Err(LobbyError::Protocol);
        }
        Ok(())
    }
}

/// How far a rider has got preparing the selected activity, for the others to see (#419).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Receiving the course or workout from the organizer.
    Downloading,
    /// Building the course's world, or loading the workout.
    Preparing,
}

/// Reliable membership/countdown events and disposable latest-state updates.
#[derive(Clone, Debug)]
pub enum LobbyEvent {
    /// The host selected new content; previous local preparation is obsolete.
    Selection(crate::activity::Selection),
    /// The organizer ended this revision; memberships and voice remain connected.
    Lobby {
        /// Last ended selection; later activities must use a greater revision.
        revision: u64,
    },
    /// What the organizer is choosing, to fetch with [`crate::content::fetch_preview`]
    /// (#418); none once it is shared or put away, and none after the countdown.
    Preview(Option<crate::activity::ContentId>),
    /// The organizer's drafting and keep-together choices changed before the countdown,
    /// also while nothing is selected (#557).
    Assistance(crate::activity::Assistance),
    /// One current participant verified/prepared this revision, or withdrew readiness.
    Readiness {
        /// Authenticated roster slot, including the local rider.
        id: RiderId,
        /// Selection this acknowledgment applies to.
        revision: u64,
        /// Whether local preparation completed.
        ready: bool,
    },
    /// A participant not yet ready reported how far it has prepared this revision (#419).
    Preparation {
        /// Authenticated roster slot, including the local rider.
        id: RiderId,
        /// Selection being prepared.
        revision: u64,
        /// What it is doing.
        stage: Stage,
        /// How much of it is done, 0–100.
        percent: u8,
    },
    /// An admitted rider joined the roster.
    Joined {
        /// Assigned number.
        id: RiderId,
        /// Printable metadata.
        rider: Rider,
    },
    /// A rider disconnected; the local ride continues.
    Left {
        /// Departing number.
        id: RiderId,
    },
    /// A rider's connection was lost; their seat is held while they reconnect (#434). Their
    /// position and voice stop meanwhile, and in the lobby they keep the start waiting.
    Away {
        /// The absent rider's number.
        id: RiderId,
    },
    /// A rider whose seat was held is connected again.
    Back {
        /// The returning rider's number.
        id: RiderId,
    },
    /// This rider's connection to the organizer was lost; it reconnects by itself until
    /// `until`, and [`LobbyEvent::HostLeft`] follows if that fails (#434). The local ride
    /// continues; the microphone is off, and others' positions stop until reconnected.
    Reconnecting {
        /// When the organizer gives this rider's seat up.
        until: Instant,
    },
    /// This rider is connected to the organizer again, in the same seat.
    Reconnected,
    /// Common countdown target on this computer's monotonic clock.
    Start {
        /// Estimated start, compensating for half the measured RTT at joiners.
        at: Instant,
    },
    /// A rider joining after countdown learns the group clock without starting locally.
    Running {
        /// Original shared start on the local monotonic clock.
        at: Instant,
    },
    /// Response to an explicit request to enter the running activity.
    Entry {
        /// Frozen selection this entry belongs to.
        revision: u64,
        /// Fresh organizer progress, or unavailable while the organizer is not riding.
        progress: Option<RideProgress>,
    },
    /// Organizer-owned structured-workout time; never a trainer target or personal intensity.
    Workout(WorkoutClock),
    /// A bounded handover control from the authenticated current membership or organizer.
    Handover {
        /// Authenticated sender; never copied from an asserted peer identity.
        sender: RiderId,
        /// Typed, bounded proposal or preparation message.
        message: crate::handover::Message,
    },
    /// One validated latest sample, at most five times per second per rider. App lobbies
    /// include the local rider's host-accepted echo, for consistent group presentation.
    State {
        /// Authenticated sender assigned by the host.
        id: RiderId,
        /// Physical state without heart rate.
        state: RiderState,
    },
    /// A pending reply expired.
    Expired,
    /// The host or networking task ended. The local ride continues.
    HostLeft,
    /// Local networking diagnostics, delivered before departure/end. Never peer-supplied text.
    Failed {
        /// Affected admitted rider, or `None` for the local organizer task.
        peer: Option<RiderId>,
        /// Original bounded protocol, transport or queue failure.
        error: Arc<LobbyError>,
        /// Local operation that failed; never copied from a remote message.
        operation: &'static str,
    },
}

/// Bounded lobby failures, without copying peer-supplied diagnostics.
#[derive(Debug, thiserror::Error)]
pub enum LobbyError {
    /// Raw datagram traffic persistently exceeded a full room's safety allowance.
    #[error("The peer persistently exceeded the group datagram safety budget.")]
    DatagramFlood,
    /// Invalid or excessive voice traffic, retaining its specific cause.
    #[error(transparent)]
    Voice(#[from] voice::VoiceError),
    /// Consent/readiness controls exceeded the bounded input allowance.
    #[error("The peer sent group controls too quickly.")]
    ControlFlood,
    /// A local networking reader panicked or stopped without a result.
    #[error("The group networking reader stopped unexpectedly.")]
    Worker,
    /// A started reliable control frame did not finish within its deadline.
    #[error("The peer started a group control message but did not finish it within two seconds.")]
    ControlTimeout,
    /// Activity state, membership or revision mismatch.
    #[error(transparent)]
    Activity(#[from] crate::activity::ActivityError),
    /// Discovery, transport or admission failed.
    #[error(transparent)]
    Link(#[from] LinkError),
    /// Invalid physical state or a state flood.
    #[error(transparent)]
    State(#[from] StateError),
    /// Invalid rider metadata, frame or protocol transition.
    #[error("The peer sent an invalid lobby message or rider description.")]
    Protocol,
    /// A queue is full, or the networking owner disappeared.
    #[error("The lobby is busy or disconnected.")]
    Lost,
    /// A repeated start, or admission after the legacy CLI countdown.
    #[error("The ride has already started.")]
    Started,
    /// Countdown outside the supported range.
    #[error("Choose a countdown from one to sixty seconds.")]
    Countdown,
}

struct Events {
    controls: mpsc::Receiver<LobbyEvent>,
    states: watch::Receiver<States>,
    clock: watch::Receiver<Option<WorkoutClock>>,
    pending: States,
    me: RiderId,
    include_local: bool,
    ended: bool,
    active: [bool; MAX_RIDERS],
}
impl Events {
    fn control(&mut self, event: LobbyEvent) -> LobbyEvent {
        match &event {
            LobbyEvent::Joined { id, .. } | LobbyEvent::Back { id } => {
                self.active[usize::from(id.value())] = true;
            }
            LobbyEvent::Left { id } | LobbyEvent::Away { id } => {
                self.active[usize::from(id.value())] = false;
                self.pending[usize::from(id.value())] = None;
            }
            LobbyEvent::Reconnecting { .. } | LobbyEvent::Lobby { .. } => {
                self.pending = [None; MAX_RIDERS];
            }
            _ => {}
        }
        event
    }
    async fn next(&mut self) -> Option<LobbyEvent> {
        loop {
            match self.controls.try_recv() {
                Ok(event) => return Some(self.control(event)),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    if self.ended {
                        return None;
                    }
                    self.ended = true;
                    return Some(LobbyEvent::HostLeft);
                }
                Err(mpsc::error::TryRecvError::Empty) => {}
            }
            if self.clock.has_changed().unwrap_or(false)
                && let Some(clock) = *self.clock.borrow_and_update()
            {
                return Some(LobbyEvent::Workout(clock));
            }
            for (id, state) in self.pending.iter_mut().enumerate() {
                let id = rider_id(id);
                if let Some(state) = state.take().filter(|_| {
                    (id != self.me || self.include_local) && self.active[usize::from(id.value())]
                }) {
                    return Some(LobbyEvent::State { id, state });
                }
            }
            tokio::select! {
                control = self.controls.recv() => if let Some(event) = control { return Some(self.control(event)); },
                changed = self.states.changed() => if changed.is_ok() {
                    self.pending = *self.states.borrow_and_update();
                },
                changed = self.clock.changed() => if changed.is_ok() && let Some(clock) = *self.clock.borrow_and_update() {return Some(LobbyEvent::Workout(clock));},
            }
        }
    }
}

fn rider_id(index: usize) -> RiderId {
    RiderId::new(u8::try_from(index).expect("fixed roster index")).expect("fixed roster index")
}

fn events(
    me: RiderId,
    controls: mpsc::Receiver<LobbyEvent>,
    states: watch::Receiver<States>,
    clock: watch::Receiver<Option<WorkoutClock>>,
    include_local: bool,
) -> Events {
    let mut active = [false; MAX_RIDERS];
    active[usize::from(me.value())] = include_local;
    Events {
        controls,
        states,
        clock,
        pending: [None; MAX_RIDERS],
        me,
        include_local,
        ended: false,
        active,
    }
}

enum Command {
    Admit(Reply, oneshot::Sender<Result<(), LobbyError>>),
    Start(Duration, oneshot::Sender<Result<Instant, LobbyError>>),
    End(u64, oneshot::Sender<Result<(), LobbyError>>),
    Select(SharedActivity, oneshot::Sender<Result<(), LobbyError>>),
    Assist(
        crate::activity::Assistance,
        oneshot::Sender<Result<(), LobbyError>>,
    ),
    Ready(u64, bool, oneshot::Sender<Result<(), LobbyError>>),
    PeerReady(Membership, u64, bool),
    Preparation(u64, Stage, u8),
    PeerPreparation(Membership, u64, Stage, u8),
    Enter(Membership, u64),
    WorkoutPause(bool, oneshot::Sender<Result<(), LobbyError>>),
    WorkoutSkip(Duration, oneshot::Sender<Result<(), LobbyError>>),
    Remove(RiderId),
    AdmitAs(Reply, RiderId, oneshot::Sender<Result<(), LobbyError>>),
    Handover(Option<RiderId>, crate::handover::Message),
    PeerHandover(Membership, crate::handover::Message),
    TakeOver(u64, crate::handover::Proof, Option<WorkoutClock>),
}

/// The host's lobby owner. Dropping it aborts its actor, peer tasks and transport listeners.
pub struct HostLobby {
    latency: Latency,
    organizer: RiderId,
    root: RideId,
    epoch: u64,
    invite: Invite,
    commands: mpsc::Sender<Command>,
    state: watch::Sender<Option<RiderState>>,
    progress: watch::Sender<Option<(RideProgress, Instant)>>,
    preview: watch::Sender<Option<activity::Shown>>,
    events: Events,
    tasks: JoinSet<()>,
    voice: Option<VoicePort>,
}
impl HostLobby {
    /// Starts a lobby from an already-open transport. Metadata is validated before spawning.
    ///
    /// # Errors
    /// Returns [`LobbyError::Protocol`] for invalid metadata or a host-name mismatch.
    pub fn open(host: Host, me: Rider) -> Result<Self, LobbyError> {
        Self::open_as(host, me, RiderId::HOST)
    }
    fn open_as(host: Host, me: Rider, organizer: RiderId) -> Result<Self, LobbyError> {
        Self::open_with(host, me, organizer, None)
    }
    /// Prepares a fresh successor listener without changing the live group's authority.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for a changed roster/activity, mismatched pin or invalid sources.
    pub fn prepare(host: Host, replacement: Replacement) -> Result<Self, LobbyError> {
        let organizer = replacement.plan.context.successor;
        let me = replacement.roster[usize::from(organizer.value())]
            .clone()
            .ok_or(LobbyError::Protocol)?;
        let prepared = Prepared::new(replacement, &host)?;
        Self::open_with(host, me, organizer, Some(prepared))
    }
    fn open_with(
        host: Host,
        me: Rider,
        organizer: RiderId,
        prepared: Option<Prepared>,
    ) -> Result<Self, LobbyError> {
        me.validate()?;
        if me.name != host.invite().host {
            return Err(LobbyError::Protocol);
        }
        let invite = host.invite().clone();
        let (root, epoch) = prepared.as_ref().map_or((invite.ride, 1), |prepared| {
            (
                prepared.replacement.plan.context.root,
                prepared.replacement.plan.context.epoch,
            )
        });
        let (commands, receive) = mpsc::channel(8);
        let (control, controls) = mpsc::channel(CONTROL_CAPACITY);
        let (states, updates) = watch::channel([None; MAX_RIDERS]);
        let (clock, clock_updates) = watch::channel(None);
        let (state, local) = watch::channel(None);
        let (progress, progress_updates) = watch::channel(None);
        let (preview, previews) = watch::channel(None);
        let (voice, owner, _changes) = voice::channel_for(organizer, organizer);
        if prepared.is_some() {
            owner.0.suspend();
        }
        let mut tasks = JoinSet::new();
        let latency = Latency::new();
        tasks.spawn(host_actor(
            host,
            me,
            receive,
            control,
            states,
            local,
            owner,
            commands.clone(),
            progress_updates,
            previews,
            clock,
            organizer,
            prepared,
            (root, epoch),
            latency.clone(),
        ));
        let include_local = invite.route == RouteHash::LOBBY;
        Ok(Self {
            latency,
            organizer,
            root,
            epoch,
            invite,
            commands,
            state,
            progress,
            preview,
            events: events(organizer, controls, updates, clock_updates, include_local),
            tasks,
            voice: Some(voice),
        })
    }
    /// The organizer's stable rider identity, independent of the hosting role.
    #[must_use]
    pub const fn organizer(&self) -> RiderId {
        self.organizer
    }

    /// Recent round-trip times between connected riders and the organizer (R235).
    /// The organizer's own slot and links without a fresh reading are `None`.
    #[must_use]
    pub fn latencies(&self) -> [Option<Duration>; MAX_RIDERS] {
        self.latency.values()
    }

    /// Logical ride identity, retained through planned handover.
    #[must_use]
    pub const fn root(&self) -> RideId {
        self.root
    }
    /// Current authority epoch, independent of fresh transport invitation identities.
    #[must_use]
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The private-channel invite for this host lifetime.
    #[must_use]
    pub fn invite(&self) -> &Invite {
        &self.invite
    }
    /// Takes the local voice port once. It is disabled and microphone-muted initially.
    pub fn take_voice(&mut self) -> Option<VoicePort> {
        self.voice.take()
    }
    /// Explicitly admits a reply; app lobbies also accept riders after countdown.
    ///
    /// # Errors
    /// Returns [`LobbyError`] on mismatch, expiry/replay, exhaustion, start or disconnection.
    pub async fn admit(&self, reply: &Reply) -> Result<(), LobbyError> {
        let (send, receive) = oneshot::channel();
        self.commands
            .try_send(Command::Admit(reply.clone(), send))
            .map_err(|_| LobbyError::Lost)?;
        receive.await.map_err(|_| LobbyError::Lost)?
    }
    /// Replaces an app lobby's activity before countdown and invalidates all readiness.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for invalid descriptors, frozen selection or disconnected actor.
    pub async fn select(&self, activity: SharedActivity) -> Result<(), LobbyError> {
        let (send, receive) = oneshot::channel();
        self.commands
            .try_send(Command::Select(activity, send))
            .map_err(|_| LobbyError::Lost)?;
        receive.await.map_err(|_| LobbyError::Lost)?
    }
    /// Tells every rider the organizer's drafting and keep-together choices before the
    /// countdown, whether or not an activity is selected (#557). Newcomers hear of them too.
    ///
    /// # Errors
    /// Returns [`LobbyError`] outside an app lobby, from the countdown on or for a
    /// disconnected actor.
    pub async fn assist(&self, rules: crate::activity::Assistance) -> Result<(), LobbyError> {
        let (send, receive) = oneshot::channel();
        self.commands
            .try_send(Command::Assist(rules, send))
            .map_err(|_| LobbyError::Lost)?;
        receive.await.map_err(|_| LobbyError::Lost)?
    }
    /// Sets the host's readiness after local preparation.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for obsolete selection or a disconnected actor.
    pub async fn ready(&self, revision: u64, ready: bool) -> Result<(), LobbyError> {
        let (send, receive) = oneshot::channel();
        self.commands
            .try_send(Command::Ready(revision, ready, send))
            .map_err(|_| LobbyError::Lost)?;
        receive.await.map_err(|_| LobbyError::Lost)?
    }
    /// Tells the group how far the organizer has prepared `revision` (#419); `percent` is
    /// 0–100. Fire-and-forget: a full queue drops it, as a later report supersedes it.
    ///
    /// # Errors
    /// Returns [`LobbyError::Lost`] for a disconnected actor.
    pub fn preparation(&self, revision: u64, stage: Stage, percent: u8) -> Result<(), LobbyError> {
        match self
            .commands
            .try_send(Command::Preparation(revision, stage, percent))
        {
            Err(mpsc::error::TrySendError::Closed(_)) => Err(LobbyError::Lost),
            _ => Ok(()),
        }
    }
    /// Shows every rider what the organizer is choosing before sharing it, or nothing (#418).
    /// Only the latest counts: riders hear of a change within about half a second and fetch
    /// it themselves. Ignored from the countdown on.
    ///
    /// # Errors
    /// Returns [`LobbyError::Activity`] for a preview out of bounds.
    pub fn preview(&self, preview: Option<&crate::preview::Preview>) -> Result<(), LobbyError> {
        let shown = preview
            .map(|preview| {
                let bytes = preview.encode()?;
                Ok::<_, LobbyError>(activity::Shown {
                    id: crate::activity::ContentId::of(
                        crate::activity::ContentKind::Preview,
                        &bytes,
                    )?,
                    bytes: bytes.into(),
                })
            })
            .transpose()?;
        self.preview.send_replace(shown);
        Ok(())
    }
    /// Privately binds a replacement admission credential to its original stable identity.
    ///
    /// # Errors
    /// Returns [`LobbyError`] outside preparation, for occupied/unexpected identities or replay.
    pub async fn admit_as(&self, reply: &Reply, id: RiderId) -> Result<(), LobbyError> {
        let (send, receive) = oneshot::channel();
        self.commands
            .try_send(Command::AdmitAs(reply.clone(), id, send))
            .map_err(|_| LobbyError::Lost)?;
        receive.await.map_err(|_| LobbyError::Lost)?
    }
    /// Sends an old-authority handover control privately, or broadcasts public preparation.
    /// Admission replies must always specify the consenting successor as recipient.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for a stopped/full queue or a broadcast admission credential.
    pub fn handover(
        &self,
        recipient: Option<RiderId>,
        message: crate::handover::Message,
    ) -> Result<(), LobbyError> {
        if recipient.is_none() && matches!(message, crate::handover::Message::Reply { .. }) {
            return Err(LobbyError::Protocol);
        }
        self.commands
            .try_send(Command::Handover(recipient, message))
            .map_err(|_| LobbyError::Lost)
    }
    /// Takes over as the group's organizer from a standby whose organizer was lost (#426),
    /// committing with the `proof` it was given in advance and continuing the shared workout
    /// from `clock`, the last one heard. The commit goes to every rider connected now and to
    /// each held seat's rider when they reconnect. Fire-and-forget like [`HostLobby::handover`].
    ///
    /// # Errors
    /// Returns [`LobbyError::Lost`] for a stopped or full command queue.
    pub fn take_over(
        &self,
        epoch: u64,
        proof: crate::handover::Proof,
        clock: Option<WorkoutClock>,
    ) -> Result<(), LobbyError> {
        self.commands
            .try_send(Command::TakeOver(epoch, proof, clock))
            .map_err(|_| LobbyError::Lost)
    }
    /// Disconnects one joiner so an unavailable participant need not block countdown.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for the host slot or a full/stopped command queue.
    pub fn remove(&self, id: RiderId) -> Result<(), LobbyError> {
        if id == self.organizer {
            return Err(LobbyError::Protocol);
        }
        self.commands
            .try_send(Command::Remove(id))
            .map_err(|_| LobbyError::Lost)
    }
    /// Starts once, returning the local countdown target. Legacy CLI admissions are sealed.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for invalid duration, repeated starts or disconnection.
    pub async fn start(&self, countdown: Duration) -> Result<Instant, LobbyError> {
        if !(Duration::from_secs(1)..=Duration::from_secs(60)).contains(&countdown) {
            return Err(LobbyError::Countdown);
        }
        let (send, receive) = oneshot::channel();
        self.commands
            .try_send(Command::Start(countdown, send))
            .map_err(|_| LobbyError::Lost)?;
        receive.await.map_err(|_| LobbyError::Lost)?
    }
    /// Ends the current activity for everyone, preserving connections and voice.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for an obsolete revision, inactive activity or unavailable queue.
    pub async fn end_activity(&self, revision: u64) -> Result<(), LobbyError> {
        let (send, receive) = oneshot::channel();
        self.commands
            .try_send(Command::End(revision, send))
            .map_err(|_| LobbyError::Lost)?;
        receive.await.map_err(|_| LobbyError::Lost)?
    }

    /// Pauses or resumes the shared workout clock while personal safety pauses stay local.
    ///
    /// # Errors
    /// Returns [`LobbyError`] without an active selected workout or an available command queue.
    pub async fn workout_pause(&self, paused: bool) -> Result<(), LobbyError> {
        let (send, receive) = oneshot::channel();
        self.commands
            .try_send(Command::WorkoutPause(paused, send))
            .map_err(|_| LobbyError::Lost)?;
        receive.await.map_err(|_| LobbyError::Lost)?
    }
    /// Advances to a later workout boundary computed from the organizer's selected plan.
    ///
    /// # Errors
    /// Returns [`LobbyError`] before start, on backward/out-of-range time or a stopped owner.
    pub async fn workout_skip(&self, next: Duration) -> Result<(), LobbyError> {
        let (send, receive) = oneshot::channel();
        self.commands
            .try_send(Command::WorkoutSkip(next, send))
            .map_err(|_| LobbyError::Lost)?;
        receive.await.map_err(|_| LobbyError::Lost)?
    }
    /// Coalesces the local rider's state; no per-call task or event is allocated.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for invalid physical values.
    pub fn send_state(&self, state: RiderState) -> Result<(), LobbyError> {
        state.datagram(self.organizer)?;
        self.state.send_replace(Some(state));
        Ok(())
    }
    /// Replaces organizer progress for late entry. `None` disables entry when not riding.
    ///
    /// # Errors
    /// Returns [`LobbyError::Protocol`] for non-finite or out-of-range progress.
    pub fn progress(&self, progress: Option<RideProgress>) -> Result<(), LobbyError> {
        if let Some(progress) = progress {
            progress.validate()?;
        }
        self.progress
            .send_replace(progress.map(|progress| (progress, Instant::now())));
        Ok(())
    }
    /// Next control or latest-state event; a stopped owner yields `HostLeft` then `None`.
    pub async fn next(&mut self) -> Option<LobbyEvent> {
        self.events.next().await
    }
    /// Stops the actor and waits for cancellation. Socket shutdown has a 100 ms close grace.
    pub async fn close(mut self) {
        self.tasks.abort_all();
        while self.tasks.join_next().await.is_some() {}
    }
}

/// The joiner's lobby owner. Losing it closes its connection and owned endpoint.
pub struct JoinerLobby {
    latency: Latency,
    me: RiderId,
    organizer: RiderId,
    root: RideId,
    epoch: u64,
    state: watch::Sender<Option<RiderState>>,
    events: Events,
    tasks: JoinSet<()>,
    voice: Option<VoicePort>,
    /// The current connection, replaced on each reconnection.
    connection: Arc<Mutex<Connection>>,
    retry: Arc<Notify>,
    readiness: mpsc::Sender<Message>,
    app_lobby: bool,
}
impl JoinerLobby {
    /// Connects, describes the rider and validates the welcome within the original deadline.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for invalid metadata, transport, timeout or protocol failures.
    pub async fn join(joiner: Joiner, rider: Rider) -> Result<Self, LobbyError> {
        Self::join_inner(joiner, rider, None, false).await
    }
    /// Prepares a pinned replacement link while retaining the original rider identity.
    /// The caller checks the plan against its live group and local policy before opening the joiner.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for changed bindings, a wrong assigned identity or invalid welcome.
    pub async fn prepare(
        joiner: Joiner,
        rider: Rider,
        id: RiderId,
        plan: crate::handover::Plan,
        roster: Roster,
        selection: crate::activity::Selection,
    ) -> Result<Self, LobbyError> {
        Self::prepared(joiner, rider, id, (plan, roster, selection), false).await
    }
    /// Prepares a standby link to the organizer's fallback like [`JoinerLobby::prepare`], kept
    /// for the ride and reconnected like any other link until the fallback takes over (#426).
    ///
    /// # Errors
    /// Returns [`LobbyError`] for changed bindings, a wrong assigned identity or invalid welcome.
    pub async fn standby(
        joiner: Joiner,
        rider: Rider,
        id: RiderId,
        plan: crate::handover::Plan,
        roster: Roster,
        selection: crate::activity::Selection,
    ) -> Result<Self, LobbyError> {
        Self::prepared(joiner, rider, id, (plan, roster, selection), true).await
    }
    async fn prepared(
        joiner: Joiner,
        rider: Rider,
        id: RiderId,
        (plan, roster, selection): (crate::handover::Plan, Roster, crate::activity::Selection),
        standby: bool,
    ) -> Result<Self, LobbyError> {
        let context = crate::handover::Context::new(
            plan.context.root,
            plan.context.epoch,
            plan.context.predecessor,
            plan.context.successor,
            &roster,
            &selection,
        )?;
        if plan.context != context || roster[usize::from(id.value())].as_ref() != Some(&rider) {
            return Err(LobbyError::Protocol);
        }
        joiner
            .reply()
            .matches(&plan.invite)
            .map_err(LinkError::from)?;
        Self::join_inner(joiner, rider, Some((id, plan, roster)), standby).await
    }
    #[allow(clippy::too_many_lines)] // one admission owns identity checks, stream handoff and failure reporting
    async fn join_inner(
        joiner: Joiner,
        rider: Rider,
        prepared: Option<(RiderId, crate::handover::Plan, Roster)>,
        standby: bool,
    ) -> Result<Self, LobbyError> {
        rider.validate()?;
        let route = joiner.reply().route;
        let (connection, resume) = joiner.connect_resumable().await?;
        let (me, organizer, root, epoch, roster, send, recv) =
            timeout_at(connection.admission_deadline(), async {
                let (mut send, mut recv) = connection.open_bi().await?;
                wire::write(&mut send, &Message::Hello(route, rider.clone())).await?;
                let Message::Welcome(me, organizer, root, epoch, roster) =
                    wire::read(&mut recv).await?
                else {
                    return Err(LobbyError::Protocol);
                };
                if roster[usize::from(me.value())].as_ref() != Some(&rider) {
                    return Err(LobbyError::Protocol);
                }
                if let Some((id, plan, expected)) = &prepared
                    && (*id != me
                        || organizer != plan.context.successor
                        || root != plan.context.root
                        || epoch != plan.context.epoch
                        || roster
                            .iter()
                            .enumerate()
                            .any(|(id, rider)| rider.is_some() && rider != &expected[id]))
                {
                    return Err(LobbyError::Protocol);
                }
                Ok::<_, LobbyError>((me, organizer, root, epoch, roster, send, recv))
            })
            .await
            .map_err(|_| LinkError::Timeout)??;
        let (control, controls) = mpsc::channel(CONTROL_CAPACITY);
        for (id, rider) in roster
            .iter()
            .enumerate()
            .filter_map(|(id, r)| r.clone().map(|r| (rider_id(id), r)))
        {
            if id != me {
                control
                    .try_send(LobbyEvent::Joined { id, rider })
                    .map_err(|_| LobbyError::Lost)?;
            }
        }
        let (states, updates) = watch::channel([None; MAX_RIDERS]);
        let (clock, clock_updates) = watch::channel(None);
        let (state, local) = watch::channel(None);
        let (voice, owner, changes) = voice::channel_for(me, organizer);
        if prepared.is_some() {
            owner.0.suspend();
        }
        let prepared = prepared.map(|(_, plan, _)| plan);
        let mut tasks = JoinSet::new();
        let (readiness, ready) = mpsc::channel(8);
        let current = Arc::new(Mutex::new(connection.clone()));
        let retry = Arc::new(Notify::new());
        let latency = Latency::new();
        let link = Link {
            latency: latency.clone(),
            controls: control,
            states,
            local,
            voice: owner,
            voice_changes: changes,
            readiness: ready,
            clock,
            app_lobby: route == RouteHash::LOBBY,
            me,
            organizer,
            prepared,
            standby,
            roster,
            away: [false; MAX_RIDERS],
        };
        let rejoin = Rejoin {
            resume,
            route,
            rider,
            root,
            epoch,
            current: current.clone(),
            retry: retry.clone(),
        };
        tasks.spawn(supervise(link, connection, send, recv, rejoin));
        Ok(Self {
            latency,
            me,
            organizer,
            root,
            epoch,
            state,
            events: events(
                me,
                controls,
                updates,
                clock_updates,
                route == RouteHash::LOBBY,
            ),
            tasks,
            voice: Some(voice),
            connection: current,
            retry,
            readiness,
            app_lobby: route == RouteHash::LOBBY,
        })
    }
    /// This connection's host-assigned number.
    #[must_use]
    pub const fn me(&self) -> RiderId {
        self.me
    }
    /// Recent organizer-measured round-trip times for the group's connected riders (R235).
    /// The organizer's slot is `None`; this rider's slot measures their link to the organizer.
    #[must_use]
    pub fn latencies(&self) -> [Option<Duration>; MAX_RIDERS] {
        if current(&self.connection).is_closed() {
            [None; MAX_RIDERS]
        } else {
            self.latency.values()
        }
    }
    /// The current organizer, authenticated by this connection's certificate pin.
    #[must_use]
    pub const fn organizer(&self) -> RiderId {
        self.organizer
    }

    /// Logical ride identity learned from the pinned authority, including late admission after handover.
    #[must_use]
    pub const fn root(&self) -> RideId {
        self.root
    }
    /// Current authority epoch from the pinned welcome.
    #[must_use]
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }

    /// An authenticated content link for one locally owned download task in app-lobby mode;
    /// after a reconnection, the new one.
    #[must_use]
    pub fn content_link(&self) -> Option<Connection> {
        self.app_lobby.then(|| current(&self.connection).clone())
    }
    /// Tries to reconnect now instead of after the pause between attempts (#434).
    pub fn reconnect(&self) {
        self.retry.notify_one();
    }
    /// Loses the connection as a failing network would, without a goodbye (#434).
    #[cfg(feature = "testing")]
    #[doc(hidden)]
    pub fn lose_connection(&self) {
        current(&self.connection).close_for_reconnect();
    }
    /// Announces local readiness for a selected app activity through a bounded control queue.
    ///
    /// # Errors
    /// Returns [`LobbyError`] in legacy mode or if the control queue is full/stopped.
    pub fn ready(&self, revision: u64, ready: bool) -> Result<(), LobbyError> {
        if !self.app_lobby {
            return Err(LobbyError::Protocol);
        }
        self.readiness
            .try_send(Message::Ready(revision, ready))
            .map_err(|_| LobbyError::Lost)
    }
    /// Tells the group how far this rider has prepared `revision` (#419); `percent` is 0–100.
    /// Callers send it on a change of stage and at most every couple of seconds otherwise, well
    /// within the host's control budget; a full queue drops it, as a later report supersedes it.
    ///
    /// # Errors
    /// Returns [`LobbyError`] in legacy mode or for a stopped control queue.
    pub fn preparation(&self, revision: u64, stage: Stage, percent: u8) -> Result<(), LobbyError> {
        if !self.app_lobby {
            return Err(LobbyError::Protocol);
        }
        match self
            .readiness
            .try_send(Message::Preparation(revision, stage, percent))
        {
            Err(mpsc::error::TrySendError::Closed(_)) => Err(LobbyError::Lost),
            _ => Ok(()),
        }
    }
    /// Explicitly enters a running app ride after declaring this selection prepared.
    /// A response is delivered as [`LobbyEvent::Entry`]; it never restarts an existing ride.
    ///
    /// # Errors
    /// Returns [`LobbyError`] in legacy mode or when the bounded control queue is unavailable.
    pub fn enter(&self, revision: u64) -> Result<(), LobbyError> {
        if !self.app_lobby {
            return Err(LobbyError::Protocol);
        }
        self.readiness
            .try_send(Message::Enter(revision))
            .map_err(|_| LobbyError::Lost)
    }
    /// Sends a bounded preparation response over the existing pinned organizer connection.
    ///
    /// # Errors
    /// Returns [`LobbyError`] in legacy mode or when its fixed control queue is unavailable.
    pub fn handover(&self, message: crate::handover::Message) -> Result<(), LobbyError> {
        if !self.app_lobby {
            return Err(LobbyError::Protocol);
        }
        self.readiness
            .try_send(Message::Handover(message))
            .map_err(|_| LobbyError::Lost)
    }
    /// Takes the local, disabled voice port once, without opening audio hardware.
    pub fn take_voice(&mut self) -> Option<VoicePort> {
        self.voice.take()
    }
    /// Replaces one latest local state, published at most five times per second.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for invalid physical values.
    pub fn send_state(&self, state: RiderState) -> Result<(), LobbyError> {
        state.datagram(self.me)?;
        self.state.send_replace(Some(state));
        Ok(())
    }
    /// Next control or coalesced state; losing the host never stops local ride logic.
    pub async fn next(&mut self) -> Option<LobbyEvent> {
        self.events.next().await
    }
    /// Closes this connection and stops its actor and readers.
    pub async fn close(mut self) {
        self.tasks.abort_all();
        while self.tasks.join_next().await.is_some() {}
    }
}

impl Drop for JoinerLobby {
    fn drop(&mut self) {
        // A goodbye, so the organizer frees the seat at once rather than holding it.
        current(&self.connection).close();
    }
}

fn current(connection: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    connection
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A control that could not be written in time: the connection stalled (#434).
const STALLED: LobbyError = LobbyError::Link(LinkError::Timeout);

/// Closes a connection whose actor stopped and tells whether the network lost it (#434): a
/// stalled one is closed for the peer to reconnect, anything else as a goodbye, also a
/// control stream that ended, which a leaving peer's stream does before its close arrives.
/// The transport's own reason, a timeout or the peer's close, takes precedence.
fn lost(connection: &Connection, stalled: bool) -> bool {
    if stalled {
        connection.close_for_reconnect();
    } else {
        connection.close();
    }
    connection.lost()
}

/// What the joiner's actor keeps across reconnections: its channels to the lobby owner and
/// what the owner was last told about the group.
struct Link {
    latency: Latency,
    controls: mpsc::Sender<LobbyEvent>,
    states: watch::Sender<States>,
    local: watch::Receiver<Option<RiderState>>,
    voice: VoiceOwner,
    voice_changes: watch::Receiver<bool>,
    readiness: mpsc::Receiver<Message>,
    clock: watch::Sender<Option<WorkoutClock>>,
    app_lobby: bool,
    me: RiderId,
    organizer: RiderId,
    prepared: Option<crate::handover::Plan>,
    /// A standby link, reconnected like any other while it waits for a takeover (#426).
    standby: bool,
    roster: Roster,
    away: [bool; MAX_RIDERS],
}
impl Link {
    /// Tells the owner how the group changed while this rider was away, then that it is back.
    /// Riders still away are marked again by the organizer's next controls.
    fn rejoined(&mut self, roster: Roster) -> Result<(), LobbyError> {
        for (index, (old, new)) in self.roster.iter().zip(&roster).enumerate() {
            let id = rider_id(index);
            if id == self.me {
                continue;
            }
            if old.is_some() && old != new {
                event(&self.controls, LobbyEvent::Left { id })?;
            } else if old.is_some() && self.away[index] {
                event(&self.controls, LobbyEvent::Back { id })?;
            }
            if let Some(rider) = new.clone().filter(|_| old != new) {
                event(&self.controls, LobbyEvent::Joined { id, rider })?;
            }
        }
        self.away = [false; MAX_RIDERS];
        self.roster = roster;
        event(&self.controls, LobbyEvent::Reconnected)
    }
}

/// How a joiner takes its held seat back: the same certificate and rider, the same group.
struct Rejoin {
    resume: Resume,
    route: RouteHash,
    rider: Rider,
    root: RideId,
    epoch: u64,
    current: Arc<Mutex<Connection>>,
    retry: Arc<Notify>,
}
type Rejoined = (Connection, SendStream, RecvStream, Roster);
impl Rejoin {
    /// Tries again every few seconds, or at once when asked, until `until`; `None` once the
    /// organizer turned this rider away or the time is up.
    async fn until(&self, until: Instant, link: &Link) -> Option<Rejoined> {
        loop {
            let round = Instant::now();
            if round >= until {
                return None;
            }
            match timeout_at(until, self.attempt(link)).await {
                Ok(Ok(rejoined)) => return Some(rejoined),
                Ok(Err(LobbyError::Link(LinkError::Replay) | LobbyError::Protocol)) | Err(_) => {
                    return None;
                }
                Ok(Err(_)) => {}
            }
            tokio::select! {
                () = tokio::time::sleep_until((round + RECONNECT_EVERY).min(until)) => {}
                () = self.retry.notified() => {}
            }
        }
    }
    async fn attempt(&self, link: &Link) -> Result<Rejoined, LobbyError> {
        let connection = self.resume.connect().await?;
        let (send, recv, roster) = timeout_at(connection.admission_deadline(), async {
            let (mut send, mut recv) = connection.open_bi().await?;
            wire::write(&mut send, &Message::Hello(self.route, self.rider.clone())).await?;
            let Message::Welcome(me, organizer, root, epoch, roster) =
                wire::read(&mut recv).await?
            else {
                return Err(LobbyError::Protocol);
            };
            if me != link.me
                || organizer != link.organizer
                || root != self.root
                || epoch != self.epoch
                || roster[usize::from(me.value())].as_ref() != Some(&self.rider)
            {
                return Err(LobbyError::Protocol);
            }
            Ok::<_, LobbyError>((send, recv, roster))
        })
        .await
        .map_err(|_| LinkError::Timeout)??;
        Ok((connection, send, recv, roster))
    }
}

/// Runs the joiner's actor on its connection and, in app lobbies, again on each reconnection
/// within [`RECONNECT_TRYING`] of losing it (#434). Returning ends the lobby for its owner.
async fn supervise(
    mut link: Link,
    mut connection: Connection,
    mut send: SendStream,
    mut recv: RecvStream,
    rejoin: Rejoin,
) {
    let mut resumed = false;
    loop {
        let result = client_actor(&connection, send, recv, &mut link, resumed).await;
        let stalled = matches!(result, Err(LobbyError::Link(LinkError::Timeout)));
        // Mid-handover, the prepared successor link takes over instead; a standby link waits
        // for a takeover all ride long, so it reconnects too.
        let reconnects = link.app_lobby && (link.prepared.is_none() || link.standby);
        let network = stalled || connection.lost();
        ended(
            &link.controls,
            link.organizer,
            "receiving organizer controls",
            &connection,
            result,
            reconnects && network,
        );
        if !lost(&connection, stalled) || !reconnects {
            return;
        }
        let until = Instant::now() + RECONNECT_TRYING;
        link.voice.0.interrupt();
        link.states.send_replace([None; MAX_RIDERS]);
        link.latency.update([None; MAX_RIDERS]);
        if event(&link.controls, LobbyEvent::Reconnecting { until }).is_err() {
            return;
        }
        let Some((next, next_send, next_recv, roster)) = rejoin.until(until, &link).await else {
            return;
        };
        if link.rejoined(roster).is_err() {
            return;
        }
        current(&rejoin.current).clone_from(&next);
        (connection, send, recv, resumed) = (next, next_send, next_recv, true);
    }
}

struct Mailbox {
    inbox: StateInbox,
    revision: u64,
    start: Option<Instant>,
    entered: [bool; MAX_RIDERS],
    active: bool,
}
type Shared = Arc<Mutex<Mailbox>>;
fn lock(shared: &Shared) -> MutexGuard<'_, Mailbox> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn elapsed(start: Option<Instant>) -> Option<Duration> {
    start
        .filter(|at| *at <= Instant::now())
        .map(|at| Instant::now() - at)
}
// Joiners place the shared start half a measured round trip early, so a peer's first state
// can arrive shortly before our own start. Count it as the start; its timestamp is still
// checked against MAX_FUTURE and charged to the peer's budget. State in the lobby is refused.
fn peer_elapsed(start: Option<Instant>) -> Option<Duration> {
    let start = start?;
    let now = Instant::now();
    match start.checked_duration_since(now) {
        None => Some(now - start),
        Some(ahead) if ahead <= crate::state::MAX_FUTURE => Some(Duration::ZERO),
        Some(_) => None,
    }
}

// App connections outlive their activities. Bind disposable motion to its selection so
// packets still in flight cannot move a rider on the next course. CLI rides use revision 0.
fn state_datagram(state: RiderState, id: RiderId, revision: u64) -> Result<Vec<u8>, LobbyError> {
    let mut bytes = state.datagram(id)?.to_vec();
    if revision > 0 {
        bytes.extend(revision.to_be_bytes());
    }
    Ok(bytes)
}

fn activity_state(bytes: &[u8], revision: u64) -> Option<&[u8]> {
    if revision == 0 {
        return Some(bytes);
    }
    let boundary = bytes.len().checked_sub(8)?;
    (bytes[boundary..] == revision.to_be_bytes()).then_some(&bytes[..boundary])
}

fn running(at: Instant) -> Result<Message, LobbyError> {
    let now = Instant::now();
    let age = if now >= at {
        i64::try_from((now - at).as_millis())
    } else {
        i64::try_from((at - now).as_millis()).map(|age| -age)
    }
    .map_err(|_| LobbyError::Protocol)?;
    entry::clock(age)?;
    Ok(Message::Running(age))
}

#[allow(clippy::large_enum_variant)] // fixed sixteen-entry peer queue; inline controls avoid allocations
enum Control {
    Message(Message),
    Start(Instant),
}
struct Peer {
    connection: Connection,
    member: Membership,
    control: mpsc::Sender<Control>,
}
enum Done {
    Hello(
        Connection,
        Result<(SendStream, RecvStream, Rider), LobbyError>,
    ),
    /// A peer's worker ended; `true` if the network lost it, so its seat is held (#434).
    Left(Membership, bool),
}
/// The seat of a rider whose connection was lost, held until `until` (#434).
#[derive(Clone, Copy)]
struct Away {
    until: Instant,
    client: crate::code::Fingerprint,
    /// Whether the rider had entered the running ride, and so may send state at once.
    entered: bool,
    /// Held while a standby took over (#426): the rider learns of it on returning.
    missed: bool,
}

fn event(control: &mpsc::Sender<LobbyEvent>, event: LobbyEvent) -> Result<(), LobbyError> {
    control.try_send(event).map_err(|_| LobbyError::Lost)
}
// A peer too slow to take its controls has a stalled connection: it may reconnect.
fn broadcast(peers: &[Option<Peer>; MAX_RIDERS], message: &Message) {
    for peer in peers.iter().flatten() {
        if peer
            .control
            .try_send(Control::Message(message.clone()))
            .is_err()
        {
            peer.connection.close_for_reconnect();
        }
    }
}

/// Frees a departed rider's seat for good and tells everyone.
fn leave(
    roster: &mut Roster,
    peers: &[Option<Peer>; MAX_RIDERS],
    activities: &mut Activities,
    controls: &mpsc::Sender<LobbyEvent>,
    id: RiderId,
) -> Result<(), LobbyError> {
    activities.left(id);
    roster[usize::from(id.value())] = None;
    broadcast(peers, &Message::Left(id));
    event(controls, LobbyEvent::Left { id })
}

/// Makes a prepared successor's lobby the group's authority: everyone in the roster is ready
/// for the frozen revision, ride state and voice open. Its commit then goes to the riders.
fn activated(
    revision: u64,
    roster: &Roster,
    activities: &mut Activities,
    shared: &Shared,
    local: &mut watch::Receiver<Option<RiderState>>,
    voice: &VoiceHandle,
) -> Result<(), LobbyError> {
    for (slot, rider) in roster.iter().enumerate() {
        if rider.is_some() {
            activities.ready(rider_id(slot), revision, true)?;
        }
    }
    {
        let mut mailbox = lock(shared);
        mailbox.active = true;
        mailbox.entered = std::array::from_fn(|id| roster[id].is_some());
    }
    drop(local.borrow_and_update());
    voice.activate();
    Ok(())
}

/// Tells a rider just connected which seats are held for riders reconnecting (#434).
fn held_seats(away: &[Option<Away>; MAX_RIDERS]) -> Vec<Message> {
    away.iter()
        .enumerate()
        .filter(|(_, seat)| seat.is_some())
        .map(|(slot, _)| Message::Away(rider_id(slot)))
        .collect()
}

/// A rider who just said hello, about to be served.
struct Admitted {
    connection: Connection,
    send: SendStream,
    recv: RecvStream,
    member: Membership,
    welcome: Message,
    prelude: Vec<Message>,
}

/// Starts the worker serving one admitted or reconnected rider, with what it must catch up on.
#[allow(clippy::too_many_arguments)] // the host actor's shared state, lent for one more worker
fn serve(
    tasks: &mut JoinSet<Done>,
    admitted: Admitted,
    shared: &Shared,
    states: &watch::Sender<States>,
    voice: &VoiceHandle,
    activities: &Activities,
    timeline: &mut Option<Timeline>,
    commands: &mpsc::Sender<Command>,
    controls: &mpsc::Sender<LobbyEvent>,
) -> Result<Peer, LobbyError> {
    let Admitted {
        connection,
        send,
        recv,
        member,
        welcome,
        prelude,
    } = admitted;
    let mut initial = activities.initial();
    if let Some(at) = lock(shared).start {
        initial.push(running(at)?);
        if at <= Instant::now()
            && let Some(timeline) = timeline
        {
            initial.push(Message::Workout(timeline.sample(at.into_std())?));
        }
    }
    let (control, receive) = mpsc::channel(16);
    let peer = Peer {
        connection: connection.clone(),
        member,
        control,
    };
    let (shared, updates, epochs) = (shared.clone(), states.subscribe(), voice.epochs());
    let (voice, selection, shown, commands, diagnostics) = (
        voice.clone(),
        activities.selected.subscribe(),
        activities.shown.subscribe(),
        commands.clone(),
        controls.clone(),
    );
    let app_lobby = activities.enabled;
    tasks.spawn(async move {
        let result = serve_peer(
            &connection,
            send,
            recv,
            welcome,
            prelude,
            member,
            shared,
            receive,
            updates,
            voice,
            epochs,
            initial,
            selection,
            shown,
            commands,
            app_lobby,
        )
        .await;
        let stalled = matches!(result, Err(LobbyError::Link(LinkError::Timeout)));
        let network = stalled || connection.lost();
        ended(
            &diagnostics,
            member.rider(),
            "receiving peer controls",
            &connection,
            result,
            network,
        );
        Done::Left(member, lost(&connection, stalled))
    });
    Ok(peer)
}

async fn hello(connection: Connection, route: RouteHash) -> Done {
    let result = timeout_at(connection.admission_deadline(), async {
        let (send, mut recv) = connection.accept_bi().await?;
        let Message::Hello(received, rider) = wire::read(&mut recv).await? else {
            return Err(LobbyError::Protocol);
        };
        if received != route {
            return Err(LobbyError::Protocol);
        }
        Ok::<_, LobbyError>((send, recv, rider))
    })
    .await
    .map_err(|_| LinkError::Timeout.into())
    .flatten();
    Done::Hello(connection, result)
}

#[allow(clippy::too_many_arguments)] // actor channels and the lifetime guard that invalidates audio ports
async fn host_actor(
    mut host: Host,
    me: Rider,
    mut commands: mpsc::Receiver<Command>,
    controls: mpsc::Sender<LobbyEvent>,
    states: watch::Sender<States>,
    mut local: watch::Receiver<Option<RiderState>>,
    voice: VoiceOwner,
    command_sender: mpsc::Sender<Command>,
    progress: watch::Receiver<Option<(RideProgress, Instant)>>,
    mut previews: watch::Receiver<Option<activity::Shown>>,
    clock: watch::Sender<Option<WorkoutClock>>,
    organizer: RiderId,
    prepared: Option<Prepared>,
    authority: (RideId, u64),
    latency: Latency,
) {
    let result = host_loop(
        &mut host,
        me,
        &mut commands,
        &controls,
        &states,
        &mut local,
        &voice.0,
        &command_sender,
        &progress,
        &mut previews,
        &clock,
        organizer,
        prepared,
        authority,
        &latency,
    )
    .await;
    if let Err(error) = result {
        report(&controls, None, "running organizer lobby", error);
    }
}

fn report(
    controls: &mpsc::Sender<LobbyEvent>,
    peer: Option<RiderId>,
    operation: &'static str,
    error: LobbyError,
) {
    tracing::warn!(?peer, operation, %error, "group networking failed");
    let _ = event(
        controls,
        LobbyEvent::Failed {
            peer,
            error: Arc::new(error),
            operation,
        },
    );
}

fn reader_result(
    result: Option<Result<Result<(), LobbyError>, tokio::task::JoinError>>,
) -> LobbyError {
    match result {
        Some(Ok(Err(error))) => error,
        _ => LobbyError::Worker,
    }
}

/// Says why a connection's worker ended (#461), read before the connection is closed. A
/// normal close is only logged; so is a network loss the group reconnects from (#434), whose
/// rider shows as reconnecting instead. Any other failure reaches the lobby's owner.
fn ended(
    controls: &mpsc::Sender<LobbyEvent>,
    peer: RiderId,
    operation: &'static str,
    connection: &Connection,
    result: Result<(), LobbyError>,
    reconnects: bool,
) {
    let Err(error) = result else {
        return;
    };
    if matches!(error, LobbyError::Lost | LobbyError::Link(LinkError::Lost))
        && connection.ended_normally()
    {
        tracing::info!(peer = peer.value(), operation, "group connection closed");
        return;
    }
    let error = transport_error(error, connection);
    if reconnects {
        tracing::warn!(peer = peer.value(), operation, %error, "group connection lost");
        return;
    }
    report(controls, Some(peer), operation, error);
}

fn transport_error(error: LobbyError, connection: &Connection) -> LobbyError {
    if matches!(error, LobbyError::Lost | LobbyError::Link(LinkError::Lost)) {
        connection.failure().into()
    } else {
        error
    }
}

#[allow(clippy::too_many_arguments)] // bounded actor channels and independent voice mailbox
#[allow(clippy::too_many_lines)] // one bounded host transition table owns admission, selection and teardown
async fn host_loop(
    host: &mut Host,
    me: Rider,
    commands: &mut mpsc::Receiver<Command>,
    controls: &mpsc::Sender<LobbyEvent>,
    states: &watch::Sender<States>,
    local: &mut watch::Receiver<Option<RiderState>>,
    voice: &VoiceHandle,
    command_sender: &mpsc::Sender<Command>,
    progress: &watch::Receiver<Option<(RideProgress, Instant)>>,
    previews: &mut watch::Receiver<Option<activity::Shown>>,
    clock: &watch::Sender<Option<WorkoutClock>>,
    organizer: RiderId,
    mut prepared: Option<Prepared>,
    authority: (RideId, u64),
    latency: &Latency,
) -> Result<(), LobbyError> {
    let shared = Arc::new(Mutex::new(Mailbox {
        inbox: StateInbox::new(Instant::now().into_std()),
        revision: prepared
            .as_ref()
            .map_or(0, |p| p.replacement.activity.selection.revision),
        start: prepared
            .as_ref()
            .map(|prepared| Instant::from_std(prepared.replacement.start)),
        entered: [false; MAX_RIDERS],
        active: prepared.is_none(),
    }));
    let member = lock(&shared)
        .inbox
        .join(organizer, Instant::now().into_std())?;
    voice.join(member, None);
    let mut roster: Roster = std::array::from_fn(|_| None);
    roster[usize::from(organizer.value())] = Some(me);
    let mut peers: [Option<Peer>; MAX_RIDERS] = std::array::from_fn(|_| None);
    let mut activities = Activities::new(host.invite().route == RouteHash::LOBBY, organizer)?;
    let mut timeline: Option<Timeline> = None;
    if let Some(prepared) = &prepared {
        let replacement = &prepared.replacement;
        activities.select(replacement.activity.clone())?;
        activities.ready(organizer, replacement.plan.context.revision, true)?;
        activities.start()?;
        timeline = replacement
            .plan
            .clock(replacement.start)?
            .map(Timeline::from_clock);
    }
    let mut tasks = JoinSet::new();
    let mut away: [Option<Away>; MAX_RIDERS] = [None; MAX_RIDERS];
    // The certificate that last had each seat: a rider added again takes theirs back (#536).
    let mut seats: [Option<crate::code::Fingerprint>; MAX_RIDERS] = [None; MAX_RIDERS];
    // A standby's takeover, for riders who were away when it happened (#426).
    let mut committed: Option<crate::handover::Message> = None;
    let mut ticker = interval(STATE_EVERY);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ping_tick = interval(latency::EVERY);
    ping_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(Command::Admit(reply, result)) => {
                    let answer = if prepared.as_ref().is_some_and(|prepared| !prepared.active) { Err(LobbyError::Protocol) } else if !activities.enabled && lock(&shared).start.is_some() { Err(LobbyError::Started) } else { host.admit(&reply).map_err(Into::into) };
                    let _ = result.send(answer);
                }
                Some(Command::Start(countdown, result)) => {
                    if lock(&shared).start.is_some() { let _ = result.send(Err(LobbyError::Started)); continue; }
                    // A rider reconnecting keeps the start waiting until back or removed (#434).
                    if away.iter().any(Option::is_some) { let _ = result.send(Err(crate::activity::ActivityError::NotReady.into())); continue; }
                    if let Err(error) = activities.start() { let _ = result.send(Err(error)); continue; }
                    let at = Instant::now() + countdown;
                    drop(local.borrow_and_update());
                    lock(&shared).start = Some(at);
                    if let Some(selected) = activities.selected.borrow().as_ref().filter(|selected| selected.selection.workout.is_some()) {
                        timeline = Some(Timeline::new(selected.selection.revision, at.into_std()));
                    }
                    if !activities.enabled { host.seal(); }
                    lock(&shared).entered = std::array::from_fn(|id| roster[id].is_some());
                    for peer in peers.iter().flatten() {
                        if peer.control.try_send(Control::Start(at)).is_err() { peer.connection.close_for_reconnect(); }
                    }
                    event(controls, LobbyEvent::Start { at })?;
                    let _ = result.send(Ok(at));
                }
                Some(Command::Select(selected,result)) => {
                    let answer = activities.select(selected.clone());
                    if answer.is_ok() {
                        lock(&shared).revision = selected.selection.revision;
                        broadcast(&peers,&Message::Selection(selected.selection.clone()));
                        event(controls,LobbyEvent::Selection(selected.selection))?;
                    }
                    let _ = result.send(answer);
                }
                Some(Command::Assist(rules,result)) => {
                    let answer = activities.assist(rules);
                    if let Ok(Some(message)) = &answer {broadcast(&peers,message);}
                    let _ = result.send(answer.map(|_| ()));
                }
                Some(Command::End(revision, result)) => {
                    let answer = activities.end(revision);
                    if answer.is_ok() {
                        {
                            let mut mailbox = lock(&shared);
                            mailbox.start = None;
                            mailbox.entered.fill(false);
                            mailbox.inbox.clear_states();
                            states.send_replace([None; MAX_RIDERS]);
                        }
                        for held in away.iter_mut().flatten() {held.entered = false;}
                        timeline = None;
                        clock.send_replace(None);
                        drop(local.borrow_and_update());
                        broadcast(&peers, &Message::Lobby(revision));
                        event(controls, LobbyEvent::Lobby {revision})?;
                    }
                    let _ = result.send(answer);
                }
                Some(Command::Ready(revision,ready,result)) => {
                    let answer = activities.ready(organizer,revision,ready);
                    if answer.is_ok() {
                        broadcast(&peers,&Message::Readiness(organizer,revision,ready));
                        event(controls,LobbyEvent::Readiness {id:organizer,revision,ready})?;
                    }
                    let _ = result.send(answer);
                }
                Some(Command::Preparation(revision,stage,percent)) => {
                    if percent <= 100 && activities.preparing(organizer,revision) {
                        broadcast(&peers,&Message::Preparations(organizer,revision,stage,percent));
                        event(controls,LobbyEvent::Preparation {id:organizer,revision,stage,percent})?;
                    }
                }
                Some(Command::PeerPreparation(member,revision,stage,percent)) => {
                    let id = member.rider();
                    if peers[usize::from(id.value())].as_ref().is_some_and(|p| p.member==member)
                        && percent <= 100 && activities.preparing(id,revision) {
                        broadcast(&peers,&Message::Preparations(id,revision,stage,percent));
                        event(controls,LobbyEvent::Preparation {id,revision,stage,percent})?;
                    }
                }
                Some(Command::PeerReady(member,revision,ready)) => {
                    let id = member.rider();
                    if peers[usize::from(id.value())].as_ref().is_some_and(|p| p.member==member)
                        && activities.ready(id,revision,ready).is_ok() {
                        broadcast(&peers,&Message::Readiness(id,revision,ready));
                        event(controls,LobbyEvent::Readiness {id,revision,ready})?;
                    }
                }
                Some(Command::Enter(member, revision)) => {
                    let slot = usize::from(member.rider().value());
                    if let Some(peer) = peers[slot].as_ref().filter(|peer| peer.member == member) {
                        let available = {
                            let mailbox = lock(&shared);
                            elapsed(mailbox.start).is_some() && !mailbox.entered[slot]
                                && activities.can_enter(member.rider(), revision)
                        };
                        let current = (*progress.borrow()).filter(|(_, at)| at.elapsed() <= Duration::from_secs(2))
                            .map(|(current, _)| current).filter(|_| available);
                        let current = current.map(|mut current| {if current.workout.is_some() {current.workout = timeline.as_ref().map(Timeline::position);} current});
                        if current.is_some() { lock(&shared).entered[slot] = true; }
                        if peer.control.try_send(Control::Message(Message::Entry(revision, current))).is_err() {peer.connection.close();}
                    }
                }
                Some(Command::WorkoutPause(paused, result)) => {
                    let answer = if elapsed(lock(&shared).start).is_some() {
                        if let Some(timeline) = &mut timeline {timeline.pause(paused); Ok(())} else {Err(LobbyError::Protocol)}
                    } else {Err(LobbyError::Protocol)};
                    let _ = result.send(answer);
                }
                Some(Command::WorkoutSkip(next, result)) => {
                    let answer = if elapsed(lock(&shared).start).is_some() {
                        timeline.as_mut().ok_or(LobbyError::Protocol).and_then(|timeline| timeline.skip(next))
                    } else {Err(LobbyError::Protocol)};
                    let _ = result.send(answer);
                }
                Some(Command::AdmitAs(reply, id, result)) => {
                    let answer = prepared.as_mut().ok_or(LobbyError::Protocol).and_then(|prepared| prepared.admit(host, &roster, id, &reply));
                    let _ = result.send(answer);
                }
                Some(Command::Handover(recipient, message)) => {
                    if let Some(id) = recipient {
                        if let Some(peer) = &peers[usize::from(id.value())]
                            && peer.control.try_send(Control::Message(Message::Handover(message))).is_err() {peer.connection.close();}
                    } else {broadcast(&peers, &Message::Handover(message));}
                }
                Some(Command::PeerHandover(member, message)) => {
                    let slot = usize::from(member.rider().value());
                    if !peers[slot].as_ref().is_some_and(|peer| peer.member == member) {continue;}
                    if let crate::handover::Message::Commit {epoch, ref proof} = message
                        && let Some(prepared) = &mut prepared && !prepared.active {
                        match prepared.activate(epoch, proof, &roster) {
                            Ok(true) => {
                                activated(prepared.replacement.plan.context.revision, &roster, &mut activities, &shared, local, voice)?;
                                broadcast(&peers, &Message::Handover(message.clone()));
                                event(controls, LobbyEvent::Handover {sender: organizer, message})?;
                            }
                            _ => if let Some(peer) = &peers[slot] {peer.connection.close();},
                        }
                    } else {event(controls, LobbyEvent::Handover {sender: member.rider(), message})?;}
                }
                // The organizer was lost: this standby takes over by itself (#426).
                Some(Command::TakeOver(epoch, proof, clock)) => {
                    if let Some(prepared) = prepared.as_mut().filter(|prepared| prepared.replacement.standby && !prepared.active)
                        && prepared.activate(epoch, &proof, &roster).is_ok_and(|changed| changed)
                    {
                        activated(prepared.replacement.plan.context.revision, &roster, &mut activities, &shared, local, voice)?;
                        if let Some(clock) = clock.filter(|clock| clock.revision == prepared.replacement.plan.context.revision) {timeline = Some(Timeline::from_clock(clock));}
                        for seat in away.iter_mut().flatten() {seat.missed = true; seat.entered = true;}
                        let message = crate::handover::Message::Commit {epoch, proof};
                        broadcast(&peers, &Message::Handover(message.clone()));
                        committed = Some(message.clone());
                        event(controls, LobbyEvent::Handover {sender: organizer, message})?;
                    }
                }
                Some(Command::Remove(id)) => {
                    let slot = usize::from(id.value());
                    if let Some(peer) = &peers[slot] {peer.connection.close();}
                    else if let Some(held) = away[slot].take() {
                        host.release(held.client);
                        leave(&mut roster, &peers, &mut activities, controls, id)?;
                    }
                }
                None => return Ok(()),
            },
            incoming = host.next() => match incoming {
                Some(HostEvent::Joined(connection)) if (activities.enabled || lock(&shared).start.is_none()) && tasks.len() < 16 => {
                    let route = host.invite().route;
                    tasks.spawn(hello(connection, route));
                }
                Some(HostEvent::Joined(connection)) => connection.close(),
                Some(HostEvent::Expired) => event(controls, LobbyEvent::Expired)?,
                None => return Err(LobbyError::Lost),
            },
            done = tasks.join_next(), if !tasks.is_empty() => match done {
                Some(Ok(Done::Hello(connection, Ok((send, recv, rider))))) if activities.enabled || lock(&shared).start.is_none() => {
                    // A rider whose seat is held takes it back with the same certificate (#434).
                    let held = connection.client().and_then(|client| away.iter().position(|seat| seat.is_some_and(|seat| seat.client == client)));
                    if let Some(slot) = held {
                        let id = rider_id(slot);
                        let seat = away[slot].take().expect("a held seat");
                        if roster[slot].as_ref() != Some(&rider) {
                            connection.close(); host.release(seat.client);
                            leave(&mut roster, &peers, &mut activities, controls, id)?;
                            continue;
                        }
                        let member = lock(&shared).inbox.join(id, Instant::now().into_std())?;
                        voice.join(member, Some(connection.clone()));
                        broadcast(&peers, &Message::Back(id));
                        broadcast(&peers, &Message::VoiceRoster(voice.epochs()));
                        event(controls, LobbyEvent::Back { id })?;
                        lock(&shared).entered[slot] = seat.entered;
                        let mut prelude = vec![Message::Rejoined(seat.entered)];
                        // A standby rider away at the takeover commits on returning (#426).
                        if let Some(commit) = committed.clone().filter(|_| seat.missed) {prelude.push(Message::Handover(commit));}
                        prelude.extend(held_seats(&away));
                        let welcome = Message::Welcome(id, organizer, authority.0, authority.1, roster.clone());
                        let peer = serve(&mut tasks, Admitted {connection, send, recv, member, welcome, prelude}, &shared, states, voice, &activities, &mut timeline, command_sender, controls)?;
                        peers[slot] = Some(peer);
                        continue;
                    }
                    let id = if let Some(prepared) = prepared.as_mut().filter(|prepared| !prepared.active) {
                        if let Ok(id) = prepared.assign(&connection, &rider, &roster) {id} else {connection.close(); continue;}
                    } else {
                        let old = connection.client().and_then(|client| seats.iter().position(|seat| *seat == Some(client))).filter(|slot| roster[*slot].is_none());
                        let Some(slot) = old.or_else(|| roster.iter().position(Option::is_none)) else {connection.close(); continue;}; rider_id(slot)
                    };
                    let slot = usize::from(id.value());
                    seats[slot] = connection.client();
                    activities.joined(id)?;
                    latency.clear(id);
                    let member = lock(&shared).inbox.join(id, Instant::now().into_std())?;
                    voice.join(member,Some(connection.clone()));
                    broadcast(&peers, &Message::Joined(id, rider.clone()));
                    broadcast(&peers, &Message::VoiceRoster(voice.epochs()));
                    event(controls, LobbyEvent::Joined { id, rider: rider.clone() })?;
                    roster[slot] = Some(rider);
                    lock(&shared).entered[slot] = false;
                    let welcome = Message::Welcome(id, organizer, authority.0, authority.1, roster.clone());
                    let peer = serve(&mut tasks, Admitted {connection, send, recv, member, welcome, prelude: held_seats(&away)}, &shared, states, voice, &activities, &mut timeline, command_sender, controls)?;
                    peers[slot] = Some(peer);
                }
                // A rider who reconnected but did not finish saying hello may try again.
                Some(Ok(Done::Hello(connection, Err(_)))) => {
                    if let Some(client) = connection.client() && let Some(seat) = away.iter().flatten().find(|seat| seat.client == client) {
                        host.hold(client, seat.until, connection.peer_address());
                    }
                }
                Some(Ok(Done::Left(member, lost))) => {
                    let slot = usize::from(member.rider().value());
                    if let Some(peer) = peers[slot].take_if(|p| p.member == member) {
                        let id = member.rider();
                        latency.clear(id);
                        voice.leave(member);
                        lock(&shared).inbox.remove(member);
                        let entered = std::mem::take(&mut lock(&shared).entered[slot]);
                        let client = peer.connection.client();
                        // Hold the seat of a rider the network lost; one leaving frees it (#434).
                        // A standby holds seats before any takeover too, for riders to come back.
                        if lost && activities.enabled && prepared.as_ref().is_none_or(|prepared| prepared.active || prepared.replacement.standby) && let Some(client) = client {
                            let until = Instant::now() + RECONNECT_WINDOW;
                            host.hold(client, until, peer.connection.peer_address());
                            away[slot] = Some(Away {until, client, entered, missed: false});
                            broadcast(&peers, &Message::Away(id));
                            broadcast(&peers, &Message::VoiceRoster(voice.epochs()));
                            event(controls, LobbyEvent::Away { id })?;
                        } else {
                            if let Some(client) = client {host.release(client);}
                            leave(&mut roster, &peers, &mut activities, controls, id)?;
                            broadcast(&peers, &Message::VoiceRoster(voice.epochs()));
                        }
                    }
                }
                _ => {}
            },
            _ = ping_tick.tick(), if activities.enabled && prepared.as_ref().is_none_or(|prepared| prepared.active) => {
                let values = std::array::from_fn(|slot| peers[slot].as_ref()
                    .filter(|peer| !peer.connection.is_closed())
                    .map(|peer| peer.connection.rtt().min(latency::MAX)));
                latency.update(values);
                broadcast(&peers, &Message::Latencies(values));
            },
            _ = ticker.tick() => {
                if previews.has_changed().unwrap_or(false) {activities.show(previews.borrow_and_update().clone());}
                if let Some(message) = activities.announcement(Instant::now()) {broadcast(&peers, &message);}
                for (slot, seat) in away.iter_mut().enumerate() {
                    if let Some(held) = seat.take_if(|held| held.until <= Instant::now()) {
                        host.release(held.client);
                        leave(&mut roster, &peers, &mut activities, controls, rider_id(slot))?;
                    }
                }
                if let Some(at) = {let mailbox = lock(&shared); mailbox.start.filter(|at| mailbox.active && *at <= Instant::now())} && let Some(timeline) = &mut timeline {
                    let sample = timeline.sample(at.into_std())?;
                    broadcast(&peers, &Message::Workout(sample));
                    clock.send_replace(Some(sample.clock(at.into_std())?));
                }
                let mut mailbox = lock(&shared);
                if let Some(elapsed) = elapsed(mailbox.start).filter(|_| mailbox.active) {
                    if local.has_changed().unwrap_or(false)
                        && let Some(state) = *local.borrow_and_update() {
                            mailbox.inbox.ingest(member, &state.datagram(organizer)?, elapsed, Instant::now().into_std())?;
                    }
                    states.send_replace(mailbox.inbox.drain(Instant::now().into_std()));
                }
            },
        }
    }
}

// One bounded peer worker owns both stream halves and mailboxes and reads every control a
// rider may send.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn serve_peer(
    connection: &Connection,
    mut send: SendStream,
    mut recv: RecvStream,
    welcome: Message,
    prelude: Vec<Message>,
    member: Membership,
    shared: Shared,
    mut controls: mpsc::Receiver<Control>,
    mut states: watch::Receiver<States>,
    voice: VoiceHandle,
    epochs: [u64; MAX_RIDERS],
    initial: Vec<Message>,
    selected: watch::Receiver<Option<SharedActivity>>,
    shown: watch::Receiver<Option<activity::Shown>>,
    commands: mpsc::Sender<Command>,
    app_lobby: bool,
) -> Result<(), LobbyError> {
    timeout_at(
        connection.admission_deadline(),
        wire::write(&mut send, &welcome),
    )
    .await
    .map_err(|_| LinkError::Timeout)??;
    // Whether a reconnected rider had entered, and which seats are held, before the voice
    // roster leaves those riders out (#434).
    for message in prelude {
        timeout_at(
            connection.admission_deadline(),
            wire::write(&mut send, &message),
        )
        .await
        .map_err(|_| LinkError::Timeout)??;
    }
    timeout_at(
        connection.admission_deadline(),
        wire::write(&mut send, &Message::VoiceRoster(epochs)),
    )
    .await
    .map_err(|_| LinkError::Timeout)??;
    for message in initial {
        let message = if matches!(message, Message::Running(_)) {
            running(lock(&shared).start.ok_or(LobbyError::Protocol)?)?
        } else {
            message
        };
        timeout(WRITE_WAIT, wire::write(&mut send, &message))
            .await
            .map_err(|_| STALLED)??;
    }
    let mut readers = JoinSet::new();
    let mut transfers = JoinSet::new();
    let mut transfer_budget = voice::IncomingBudget::controls();
    let mut datagrams = crate::traffic::DatagramIngress::new(std::time::Instant::now());
    let receiver_voice = voice.clone();
    let receiver_shared = shared.clone();
    readers.spawn(async move {
        let mut budget = voice::IncomingBudget::controls();
        loop {
            let message = wire::read(&mut recv).await?;
            budget.charge().map_err(|_| LobbyError::ControlFlood)?;
            match message {
                Message::VoiceEnabled(enabled) => {
                    if enabled && !lock(&receiver_shared).active {
                        return Err(LobbyError::Protocol);
                    }
                    receiver_voice.consent(member, enabled)?;
                    tracing::info!(
                        peer = member.rider().value(),
                        enabled,
                        "peer group voice listening changed"
                    );
                }
                Message::Handover(message) => commands
                    .try_send(Command::PeerHandover(member, message))
                    .map_err(|_| LobbyError::Lost)?,
                Message::Enter(revision) => commands
                    .try_send(Command::Enter(member, revision))
                    .map_err(|_| LobbyError::Lost)?,
                Message::Ready(revision, ready) => commands
                    .try_send(Command::PeerReady(member, revision, ready))
                    .map_err(|_| LobbyError::Lost)?,
                Message::Preparation(revision, stage, percent) => commands
                    .try_send(Command::PeerPreparation(member, revision, stage, percent))
                    .map_err(|_| LobbyError::Lost)?,
                _ => return Err(LobbyError::Protocol),
            }
        }
        #[allow(unreachable_code)]
        Ok::<(), LobbyError>(())
    });
    loop {
        tokio::select! {
            control = controls.recv() => {
                let message = match control {
                    Some(Control::Message(message)) => message,
                    Some(Control::Start(at)) => Message::Start(u32::try_from(at.saturating_duration_since(Instant::now()).as_millis()).map_err(|_| LobbyError::Protocol)?),
                    None => return Ok(()),
                };
                timeout(WRITE_WAIT, wire::write(&mut send, &message)).await.map_err(|_| STALLED)??;
            },
            changed = states.changed() => {
                changed.map_err(|_| LobbyError::Lost)?;
                let mailbox = lock(&shared);
                let snapshot = *states.borrow_and_update();
                for (id, state) in snapshot.iter().enumerate().filter_map(|(id,state)| state.map(|state| (rider_id(id), state))) {
                    if id != member.rider() || app_lobby {
                        let revision = if app_lobby {mailbox.revision} else {0};
                        connection.send_datagram(&state_datagram(state,id,revision)?)?;
                    }
                }
            },
            bytes = connection.read_datagram() => {
                let bytes = bytes?;
                if !datagrams.accept(std::time::Instant::now()).map_err(|_| LobbyError::DatagramFlood)? {continue;}
                if !lock(&shared).active {return Err(LobbyError::Protocol);}
                if bytes.first() == Some(&voice::TAG) {
                    voice.ingest(member,&bytes)?;
                    continue;
                }
                let mut mailbox = lock(&shared);
                let Some(bytes) = activity_state(&bytes, if app_lobby {mailbox.revision} else {0}) else {continue;};
                let Some(elapsed) = peer_elapsed(mailbox.start) else {if app_lobby {continue;} return Err(LobbyError::Protocol);};
                if !mailbox.entered[usize::from(member.rider().value())] { return Err(LobbyError::Protocol); }
                mailbox.inbox.ingest(member, bytes, elapsed, Instant::now().into_std())?;
            },
            result = readers.join_next() => return Err(reader_result(result)),
            stream = connection.accept_bi() => {
                transfer_budget.charge().map_err(|_|LobbyError::Protocol)?;
                let (send,recv) = stream?;
                while transfers.try_join_next().is_some() {}
                // One download of the selected activity, and one of the preview (#418).
                if transfers.len() >= MAX_TRANSFERS {return Err(LobbyError::Protocol);}
                transfers.spawn(activity::transfer(send,recv,selected.clone(),shown.clone()));
            },
            _ = transfers.join_next(), if !transfers.is_empty() => {},
        }
    }
}

#[allow(clippy::too_many_lines)] // ordered reliable controls and datagrams share one connection owner
async fn client_actor(
    connection: &Connection,
    mut send: SendStream,
    mut recv: RecvStream,
    link: &mut Link,
    resumed: bool,
) -> Result<(), LobbyError> {
    let (me, organizer, app_lobby, standby) =
        (link.me, link.organizer, link.app_lobby, link.standby);
    let Link {
        latency,
        controls,
        states,
        local,
        voice,
        voice_changes,
        readiness,
        clock,
        prepared,
        roster,
        away,
        ..
    } = link;
    voice.0.uplink(connection.clone());
    let mut readers = JoinSet::new();
    let (messages, mut incoming) = mpsc::channel(16);
    readers.spawn(async move {
        loop {
            let message = wire::read(&mut recv).await?;
            messages.try_send(message).map_err(|_| LobbyError::Lost)?;
        }
        #[allow(unreachable_code)]
        Ok::<(), LobbyError>(())
    });
    let mut inbox = StateInbox::new(Instant::now().into_std());
    let mut members = [None; MAX_RIDERS];
    for (id, rider) in roster.iter().enumerate() {
        if rider.is_some() && (rider_id(id) != me || app_lobby) {
            members[id] = Some(inbox.join(rider_id(id), Instant::now().into_std())?);
            if rider_id(id) != me {
                voice.0.join(members[id].ok_or(LobbyError::Protocol)?, None);
            }
        }
    }
    let mut start = None;
    let mut selection_revision = 0;
    let mut workout_sequence = 0;
    let mut late = false;
    let mut entered = false;
    // A reconnected rider first learns whether it had entered the running ride (#434).
    let mut rejoined = !resumed;
    let mut requested = None;
    let mut count = 0_u8;
    let mut control_period = Instant::now();
    let mut ticker = interval(STATE_EVERY);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // A host may forward eight independent 5 Hz streams. Charge even unknown/stale packets.
    let mut packets = 80_u8;
    let mut voice_packets = voice::IncomingBudget::new();
    let mut datagrams = crate::traffic::DatagramIngress::new(std::time::Instant::now());
    let mut voice_control_open = true;
    let mut refill = Instant::now();
    let result = async {
        // The organizer forgot this rider's listening consent with its old connection.
        if resumed && *voice_changes.borrow_and_update() {
            timeout(WRITE_WAIT,wire::write(&mut send,&Message::VoiceEnabled(true))).await.map_err(|_| STALLED)??;
        }
        loop {
            tokio::select! {
                Some(message) = readiness.recv() => {
                    if let Message::Enter(revision) = message {
                        if !late || entered || requested.is_some() || revision != selection_revision { continue; }
                        requested = Some(revision);
                    }
                    timeout(WRITE_WAIT,wire::write(&mut send,&message)).await.map_err(|_| STALLED)??;
                },
                changed = voice_changes.changed(), if voice_control_open => {
                    let enabled = if changed.is_ok() { *voice_changes.borrow_and_update() } else { voice_control_open = false; false };
                    timeout(WRITE_WAIT,wire::write(&mut send,&Message::VoiceEnabled(enabled))).await.map_err(|_| STALLED)??;
                    tracing::info!(peer = me.value(), enabled, "sent group voice listening change");
                },
                message = incoming.recv() => {
                    if app_lobby && control_period.elapsed()>=Duration::from_secs(1) {count=0;control_period=Instant::now();}
                    count = count.checked_add(1).filter(|n| *n <= 64).ok_or(LobbyError::Protocol)?;
                    match message.ok_or(LobbyError::Lost)? {
                        Message::Rejoined(was_entered) if !rejoined => {rejoined = true; entered = was_entered;}
                        Message::Lobby(revision) if app_lobby && revision > 0 && revision >= selection_revision => {
                            start = None; selection_revision = revision; workout_sequence = 0;
                            late = false; entered = false; requested = None;
                            inbox.clear_states(); states.send_replace([None; MAX_RIDERS]);
                            clock.send_replace(None); drop(local.borrow_and_update());
                            event(controls,LobbyEvent::Lobby {revision})?;
                        }
                        Message::Selection(selection) if app_lobby && start.is_none() && selection.revision>selection_revision => {
                            selection_revision = selection.revision; event(controls,LobbyEvent::Selection(selection))?;
                        }
                        // Nothing is chosen any more once the countdown began (#418).
                        Message::Preview(id) if app_lobby => if start.is_none() {event(controls,LobbyEvent::Preview(id))?;},
                        Message::Assistance(rules) if app_lobby => if start.is_none() {event(controls,LobbyEvent::Assistance(rules))?;},
                        Message::Readiness(id,revision,ready) if app_lobby && revision==selection_revision && roster[usize::from(id.value())].is_some() => {
                            event(controls,LobbyEvent::Readiness {id,revision,ready})?;
                        }
                        Message::Preparations(id,revision,stage,percent) if app_lobby && revision==selection_revision && percent <= 100 && roster[usize::from(id.value())].is_some() => {
                            event(controls,LobbyEvent::Preparation {id,revision,stage,percent})?;
                        }
                        Message::VoiceRoster(epochs) => voice.0.install_epochs(epochs)?,
                        Message::Latencies(values) if app_lobby && prepared.is_none() => latency.update(values),
                        Message::Joined(id, rider) if (app_lobby || start.is_none()) && roster[usize::from(id.value())].is_none() && id != me => {
                            latency.clear(id);
                            let index = usize::from(id.value()); members[index] = Some(inbox.join(id, Instant::now().into_std())?);
                            voice.0.join(members[index].ok_or(LobbyError::Protocol)?,None);
                            roster[index] = Some(rider.clone()); event(controls, LobbyEvent::Joined { id, rider })?;
                        }
                        Message::Left(id) if id != organizer && id != me => {
                            latency.clear(id);
                            // A held seat has no member left to remove when it is given up.
                            let index = usize::from(id.value());
                            match members[index].take() {
                                Some(member) => {voice.0.leave(member); inbox.remove(member);}
                                None if away[index] => {}
                                None => return Err(LobbyError::Protocol),
                            }
                            away[index] = false; roster[index] = None; event(controls, LobbyEvent::Left { id })?;
                        }
                        Message::Away(id) if app_lobby && id != organizer && id != me && !away[usize::from(id.value())] => {
                            latency.clear(id);
                            let index = usize::from(id.value());
                            let member = members[index].take().ok_or(LobbyError::Protocol)?;
                            voice.0.leave(member); inbox.remove(member);
                            away[index] = true; event(controls, LobbyEvent::Away { id })?;
                        }
                        Message::Back(id) if app_lobby && id != me && away[usize::from(id.value())] => {
                            latency.clear(id);
                            let index = usize::from(id.value()); members[index] = Some(inbox.join(id, Instant::now().into_std())?);
                            voice.0.join(members[index].ok_or(LobbyError::Protocol)?,None);
                            away[index] = false; event(controls, LobbyEvent::Back { id })?;
                        }
                        Message::Start(millis) if start.is_none() => {
                            drop(local.borrow_and_update());
                            let at = Instant::now() + Duration::from_millis(u64::from(millis)).saturating_sub(connection.rtt()/2);
                            entered = true; start = Some(at); event(controls, LobbyEvent::Start { at })?;
                        }
                        Message::Handover(message) if app_lobby => {
                            if let crate::handover::Message::Commit {epoch, ref proof} = message && let Some(plan) = prepared.as_ref() {
                                let members = roster.iter().enumerate().fold(0_u8, |mask, (id, rider)| if rider.is_some() {mask | (1 << id)} else {mask});
                                // A standby's commit may name fewer riders than planned, never others (#426).
                                let complete = if standby {members & !plan.context.members == 0} else {members == plan.context.members};
                                if epoch != plan.context.epoch || !plan.accepts(proof) || !complete {return Err(LobbyError::Protocol);}
                                drop(local.borrow_and_update());
                                entered = true; voice.0.activate(); *prepared = None;
                            }
                            event(controls, LobbyEvent::Handover {sender: organizer, message})?;
                        }
                        Message::Workout(_) if prepared.is_some() => {},
                        Message::Workout(sample) if app_lobby && sample.revision == selection_revision && sample.sequence > workout_sequence => {
                            let start = start.ok_or(LobbyError::Protocol)?;
                            let current = sample.clock(start.into_std())?;
                            workout_sequence = sample.sequence;
                            clock.send_replace(Some(current));
                        }
                        Message::Running(age) if app_lobby && rejoined && start.is_none() && selection_revision > 0 => {
                            let now = Instant::now();
                            let at = if age >= 0 {now.checked_sub(Duration::from_millis(age.unsigned_abs()) + connection.rtt()/2)}
                                else {now.checked_add(Duration::from_millis(age.unsigned_abs()).saturating_sub(connection.rtt()/2))}.ok_or(LobbyError::Protocol)?;
                            start = Some(at); late = !entered;
                            // A reconnected rider who had entered rides on; nothing starts again.
                            if late {event(controls, LobbyEvent::Running {at})?;}
                        }
                        Message::Entry(revision, progress) if app_lobby && late && !entered && requested == Some(revision) => {
                            requested = None; entered = progress.is_some();
                            event(controls, LobbyEvent::Entry {revision, progress})?;
                        }
                        _ => return Err(LobbyError::Protocol),
                    }
                },
                bytes = connection.read_datagram() => {
                    let bytes = bytes?;
                    if !datagrams.accept(std::time::Instant::now()).map_err(|_| LobbyError::DatagramFlood)? {continue;}
                    if bytes.first() == Some(&voice::TAG) {
                        if !voice_packets.accept() {continue;}
                        if prepared.is_some() {continue;}
                        let id = RiderId::new(*bytes.get(2).ok_or(LobbyError::Protocol)?)?;
                        if id == me { return Err(LobbyError::Protocol); }
                        if let Some(member) = members[usize::from(id.value())] {
                            voice.0.ingest(member,&bytes)?;
                        }
                        continue;
                    }
                    let added = u8::try_from((Instant::now() - refill).as_millis()/25).unwrap_or(80);
                    if added > 0 {
                        packets = packets.saturating_add(added).min(80);
                        let now = Instant::now();
                        let remainder = (now-refill).as_nanos() % Duration::from_millis(25).as_nanos();
                        refill = now - Duration::from_nanos(u64::try_from(remainder).expect("under 25 ms"));
                    }
                    if packets == 0 { continue; } packets -= 1;
                    if prepared.is_some() {continue;}
                    let Some(bytes) = activity_state(&bytes, if app_lobby {selection_revision} else {0}) else {continue;};
                    let id = RiderId::new(*bytes.get(1).ok_or(LobbyError::Protocol)?)?;
                    let Some(elapsed) = peer_elapsed(start) else {if app_lobby {continue;} return Err(LobbyError::Protocol);};
                    if id == me && !app_lobby { return Err(LobbyError::Protocol); }
                    // A datagram sent before a reliable departure may arrive after it.
                    if let Some(member) = members[usize::from(id.value())] { inbox.ingest(member, bytes, elapsed, Instant::now().into_std())?; }
                },
                _ = ticker.tick() => if let Some(elapsed) = elapsed(start) {
                    if entered && local.has_changed().unwrap_or(false)
                        && let Some(state) = *local.borrow_and_update() {
                            let bytes = state.datagram(me)?;
                            RiderState::from_datagram(&bytes, me, elapsed)?;
                            connection.send_datagram(&state_datagram(state,me,if app_lobby {selection_revision} else {0})?)?;
                    }
                    states.send_replace(inbox.drain(Instant::now().into_std()));
                },
                // A reader's own error tells a lost stream from an invalid control.
                result = readers.join_next() => return Err(match result {
                    Some(Ok(Err(error))) => error,
                    _ => LobbyError::Lost,
                }),
                stream = connection.accept_bi() => {
                    let _ = stream?;
                    return Err(LobbyError::Protocol);
                },
            }
        }
    }
    .await;
    for member in members.into_iter().flatten() {
        if member.rider() != me {
            voice.0.leave(member);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn all_riders_see_organizer_measured_pings_without_ride_state() {
        let organizer = RiderId::new(2).unwrap();
        let host = Host::open("Host", "Choose together", RouteHash::LOBBY, &options())
            .await
            .unwrap();
        let mut host = HostLobby::open_as(host, rider("Host"), organizer).unwrap();
        let mut anna = join(&host, "Anna").await;
        let ben = join(&host, "Ben").await;
        let a = usize::from(anna.me().value());
        let b = usize::from(ben.me().value());
        timeout(Duration::from_secs(3), async {
            while anna.latencies()[a].is_none()
                || anna.latencies()[b].is_none()
                || ben.latencies()[a].is_none()
                || ben.latencies()[b].is_none()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        for values in [host.latencies(), anna.latencies(), ben.latencies()] {
            assert!(values[a].is_some());
            assert!(values[b].is_some());
            assert_eq!(values[2], None);
            assert_eq!(values.iter().flatten().count(), 2);
        }
        let gone = ben.me();
        ben.close().await;
        host_event(
            &mut host,
            |event| matches!(event, LobbyEvent::Left {id} if *id == gone),
        )
        .await;
        client_event(
            &mut anna,
            |event| matches!(event, LobbyEvent::Left {id} if *id == gone),
        )
        .await;
        assert_eq!(host.latencies()[b], None);
        assert_eq!(anna.latencies()[b], None);
        lose(&anna);
        assert_eq!(anna.latencies(), [None; MAX_RIDERS]);
        host_event(
            &mut host,
            |event| matches!(event, LobbyEvent::Away {id} if usize::from(id.value()) == a),
        )
        .await;
        assert_eq!(host.latencies()[a], None);
        drop(anna);
        host.close().await;
    }

    #[tokio::test]
    async fn peers_cannot_supply_latency_tables() {
        let path = directory("forged-pings");
        let (mut host, _) = workout_lobby(&path).await;
        let (connection, mut send, _recv, id) = raw_join(&host).await;
        wire::write(
            &mut send,
            &Message::Latencies([Some(Duration::from_millis(1)); MAX_RIDERS]),
        )
        .await
        .unwrap();
        host_event(&mut host, |event| matches!(event, LobbyEvent::Failed {peer: Some(peer), error, ..} if *peer == id && matches!(error.as_ref(), LobbyError::Protocol))).await;
        host_event(
            &mut host,
            |event| matches!(event, LobbyEvent::Left {id: peer} if *peer == id),
        )
        .await;
        assert_eq!(host.latencies()[usize::from(id.value())], None);
        drop(connection);
        host.close().await;
        std::fs::remove_dir_all(path).unwrap();
    }
    use super::*;
    use crate::discovery::GatherOptions;
    use crate::link::ConnectOptions;
    use crate::policy::AddressPolicy;
    use orqwa_domain::units::{Meters, MetersPerSecond, Rpm, Watts};

    #[tokio::test]
    async fn enabling_peer_listening_preserves_lobby_and_subsequent_controls() {
        for app_lobby in [false, true] {
            let transport = if app_lobby {
                Host::lobby("Host", &options()).await.unwrap()
            } else {
                Host::open("Host", "Voice test", RouteHash([9; 32]), &options())
                    .await
                    .unwrap()
            };
            let host = HostLobby::open(transport, rider("Host")).unwrap();
            let mut peer = join(&host, "Audio peer").await;
            let port = peer.take_voice().unwrap();
            port.enable(true);
            tokio::time::sleep(Duration::from_secs(6)).await;
            assert!(port.alive(), "enabling listening must preserve the group");
            let observer = join(&host, "Observer").await;
            client_event(
                &mut peer,
                |event| matches!(event, LobbyEvent::Joined { id, .. } if *id == observer.me()),
            )
            .await;
            port.disable();
            observer.close().await;
            peer.close().await;
            host.close().await;
        }
    }

    #[tokio::test]
    async fn organizer_closure_does_not_invent_an_invalid_control_message() {
        for _ in 0..16 {
            let host = host().await;
            let mut peer = join(&host, "Peer").await;
            host.close().await;
            timeout(Duration::from_secs(2), async {
                loop {
                    match peer.next().await {
                        Some(LobbyEvent::Failed { error, .. }) => {
                            assert!(
                                !matches!(error.as_ref(), LobbyError::Protocol),
                                "ordinary close was reported as invalid control traffic"
                            );
                        }
                        Some(LobbyEvent::HostLeft) | None => break,
                        _ => {}
                    }
                }
            })
            .await
            .unwrap();
            peer.close().await;
        }
    }

    #[tokio::test]
    async fn organizer_authority_does_not_require_rider_zero() {
        let organizer = RiderId::new(3).unwrap();
        let mut host = HostLobby::open_as(
            Host::lobby("Host", &options()).await.unwrap(),
            rider("Host"),
            organizer,
        )
        .unwrap();
        let mut peer = join(&host, "Original organizer").await;
        assert_eq!(host.organizer(), organizer);
        assert_eq!(peer.me(), RiderId::HOST);
        assert_eq!(peer.organizer(), organizer);
        client_event(
            &mut peer,
            |event| matches!(event, LobbyEvent::Joined {id, ..} if *id == organizer),
        )
        .await;
        assert!(host.remove(organizer).is_err());
        host.remove(peer.me()).unwrap();
        host_event(
            &mut host,
            |event| matches!(event, LobbyEvent::Left {id} if *id == peer.me()),
        )
        .await;
        host.close().await;
        peer.close().await;
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)] // one real three-peer lifecycle proves preparation and entry preserve existing riders
    async fn late_riders_prepare_independently_and_enter_once_at_current_progress() {
        use crate::activity::{Assistance, ContentId, ContentKind, Selection};
        let directory = std::env::temp_dir().join(format!(
            "orqwa-late-{:x?}",
            crate::crypto::random::<8>().unwrap()
        ));
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("plan.oqw");
        let bytes = b"selected immutable workout";
        std::fs::write(&file, bytes).unwrap();
        let id = ContentId::of(ContentKind::Workout, bytes).unwrap();
        let mut host = HostLobby::open(
            Host::lobby("Host", &options()).await.unwrap(),
            rider("Host"),
        )
        .unwrap();
        let mut first = join(&host, "First").await;
        host.select(SharedActivity {
            selection: Selection {
                revision: 1,
                name: "Together".into(),
                course: None,
                course_key: None,
                workout: Some(id),
                assistance: Assistance::default(),
            },
            course: None,
            workout: Some(file),
        })
        .await
        .unwrap();
        client_event(&mut first, |event| {
            matches!(event, LobbyEvent::Selection(_))
        })
        .await;
        host.ready(1, true).await.unwrap();
        first.ready(1, true).unwrap();
        host_event(&mut host, |event| matches!(event, LobbyEvent::Readiness {id, ready: true, ..} if *id == first.me())).await;
        let start = host.start(Duration::from_secs(1)).await.unwrap();
        client_event(&mut first, |event| {
            matches!(event, LobbyEvent::Start { .. })
        })
        .await;
        tokio::time::sleep_until(start).await;

        let mut late = join(&host, "Late").await;
        client_event(
            &mut first,
            |event| matches!(event, LobbyEvent::Joined {id, ..} if *id == late.me()),
        )
        .await;
        client_event(&mut late, |event| matches!(event, LobbyEvent::Selection(_))).await;
        client_event(&mut late, |event| {
            matches!(event, LobbyEvent::Running { .. })
        })
        .await;
        host.send_state(RiderState {
            at: start.elapsed(),
            distance: Meters(4200.0),
            ..RiderState::default()
        })
        .unwrap();
        client_event(&mut first, |event| {
            matches!(event, LobbyEvent::State { .. })
        })
        .await;
        // Preparation does not gate existing riders or change the frozen selection.
        let received = crate::content::fetch(&late.content_link().unwrap(), id, &directory, None)
            .await
            .unwrap();
        assert_eq!(std::fs::read(received.path()).unwrap(), bytes);
        drop(received);
        late.enter(1).unwrap();
        assert!(matches!(
            client_event(&mut late, |event| matches!(event, LobbyEvent::Entry { .. })).await,
            LobbyEvent::Entry { progress: None, .. }
        ));
        let progress = RideProgress {
            distance: Meters(4300.0),
            workout: Some(Duration::from_secs(740)),
        };
        host.workout_skip(Duration::from_secs(740)).await.unwrap();
        host.workout_pause(true).await.unwrap();
        host.progress(Some(progress)).unwrap();
        late.ready(1, true).unwrap();
        late.enter(1).unwrap();
        let LobbyEvent::Entry {
            progress: Some(entry),
            ..
        } = client_event(&mut late, |event| matches!(event, LobbyEvent::Entry { .. })).await
        else {
            panic!("prepared rider enters");
        };
        assert_eq!(entry.distance, progress.distance);
        assert!(
            entry.workout.unwrap() >= Duration::from_secs(740)
                && entry.workout.unwrap() < Duration::from_secs(742)
        );
        late.send_state(RiderState {
            at: start.elapsed(),
            distance: Meters(4295.0),
            ..RiderState::default()
        })
        .unwrap();
        host_event(
            &mut host,
            |event| matches!(event, LobbyEvent::State {id, ..} if *id == late.me()),
        )
        .await;
        late.enter(1).unwrap();
        assert!(
            timeout(Duration::from_millis(250), async {
                loop {
                    if matches!(late.next().await, Some(LobbyEvent::Entry { .. })) {
                        break;
                    }
                }
            })
            .await
            .is_err(),
            "entry cannot restart a local ride"
        );
        host.progress(None).unwrap();
        let mut waiting = join(&host, "Waiting").await;
        client_event(&mut waiting, |event| {
            matches!(event, LobbyEvent::Running { .. })
        })
        .await;
        waiting.ready(1, true).unwrap();
        waiting.enter(1).unwrap();
        assert!(matches!(
            client_event(&mut waiting, |event| matches!(
                event,
                LobbyEvent::Entry { .. }
            ))
            .await,
            LobbyEvent::Entry { progress: None, .. }
        ));
        drop(waiting);
        drop(late);
        drop(first);
        host.close().await;
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn empty_app_lobbies_distribute_selected_content_and_gate_countdown_on_readiness() {
        use crate::activity::{Assistance, ContentKind, Selection};
        use crate::content::{self, Source};
        let path = std::env::temp_dir().join(format!(
            "orqwa-app-lobby-{:x?}",
            crate::crypto::random::<8>().unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        let file = path.join("source.oqw");
        std::fs::write(&file, b"normalized workout bytes").unwrap();
        let source = Source::open(&file, ContentKind::Workout).await.unwrap();
        let mut host = HostLobby::open(
            Host::lobby("Host", &options()).await.unwrap(),
            rider("Host"),
        )
        .unwrap();
        let mut client = join(&host, "Friend").await;
        assert!(matches!(
            host.start(Duration::from_secs(1)).await,
            Err(LobbyError::Activity(_))
        ));
        let selection = Selection {
            revision: 1,
            name: "Together".into(),
            course: None,
            course_key: None,
            workout: Some(source.id()),
            assistance: Assistance::default(),
        };
        host.select(SharedActivity {
            selection: selection.clone(),
            course: None,
            workout: Some(file),
        })
        .await
        .unwrap();
        client_event(&mut client, |e| matches!(e, LobbyEvent::Selection(_))).await;
        let link = client.content_link().unwrap();
        let received = content::fetch(&link, source.id(), &path, None)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(received.path()).unwrap(),
            b"normalized workout bytes"
        );
        drop(received);
        // Everyone sees how far the others have got (#419); stale revisions say nothing.
        client.preparation(7, Stage::Downloading, 10).unwrap();
        client.preparation(1, Stage::Preparing, 60).unwrap();
        let reported = host_event(&mut host, |e| matches!(e, LobbyEvent::Preparation { .. })).await;
        assert!(matches!(
            reported,
            LobbyEvent::Preparation { id, revision: 1, stage: Stage::Preparing, percent: 60 }
                if id == client.me()
        ));
        host.preparation(1, Stage::Preparing, 90).unwrap();
        let organizer = client_event(
            &mut client,
            |e| matches!(e, LobbyEvent::Preparation { id, .. } if *id == RiderId::HOST),
        )
        .await;
        assert!(matches!(
            organizer,
            LobbyEvent::Preparation {
                percent: 90,
                stage: Stage::Preparing,
                ..
            }
        ));
        host.ready(1, true).await.unwrap();
        assert!(host.start(Duration::from_secs(1)).await.is_err());
        client.ready(1, true).unwrap();
        timeout(Duration::from_secs(2),async {loop {if matches!(host.next().await,Some(LobbyEvent::Readiness {id,ready:true,..}) if id==client.me()) {break;}}}).await.unwrap();
        let start = host.start(Duration::from_secs(1)).await.unwrap();
        client_event(&mut client, |e| matches!(e, LobbyEvent::Start { .. })).await;
        assert!(
            host.select(SharedActivity {
                selection,
                course: None,
                workout: Some(path.join("source.oqw"))
            })
            .await
            .is_err()
        );
        assert_accepted_echoes(&mut host, &mut client, start).await;
        drop(client);
        drop(host);
        std::fs::remove_dir_all(path).unwrap();
    }

    async fn assert_accepted_echoes(
        host: &mut HostLobby,
        client: &mut JoinerLobby,
        start: Instant,
    ) {
        async fn accepted(events: &mut Events) -> States {
            let mut seen = [None; MAX_RIDERS];
            while seen.iter().flatten().count() < 2 {
                if let Some(LobbyEvent::State { id, state }) = events.next().await {
                    seen[usize::from(id.value())] = Some(state);
                }
            }
            seen
        }
        tokio::time::sleep_until(start).await;
        let state = RiderState {
            at: start.elapsed(),
            distance: Meters(100.0),
            speed: MetersPerSecond(10.0),
            power: Watts(180.0),
            cadence: Rpm(85.0),
        };
        host.send_state(state).unwrap();
        client
            .send_state(RiderState {
                distance: Meters(100.5),
                ..state
            })
            .unwrap();
        let (organizer, friend) = timeout(Duration::from_secs(2), async {
            tokio::join!(accepted(&mut host.events), accepted(&mut client.events))
        })
        .await
        .unwrap();
        assert_eq!(
            organizer, friend,
            "both riders receive the same accepted samples, including themselves"
        );
        assert_eq!(organizer[0].unwrap().distance, Meters(100.0));
        assert_eq!(organizer[1].unwrap().distance, Meters(100.5));
    }

    #[tokio::test]
    async fn an_organizer_shares_its_own_copy_of_the_course_but_only_the_selected_workout() {
        use crate::activity::{Assistance, ContentId, ContentKind, CourseKey, Selection};
        use crate::content;
        let path = std::env::temp_dir().join(format!(
            "orqwa-own-copy-{:x?}",
            crate::crypto::random::<8>().unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        // #435: an organizer after a handover rides the copy they renamed; so may a host.
        let course = path.join("renamed.oqc");
        std::fs::write(&course, b"renamed course").unwrap();
        let workout = path.join("changed.oqw");
        std::fs::write(&workout, b"changed workout").unwrap();
        let selection = Selection {
            revision: 1,
            name: "Together".into(),
            course: Some(ContentId::of(ContentKind::Course, b"selected course").unwrap()),
            course_key: Some(CourseKey([1; 32])),
            workout: Some(ContentId::of(ContentKind::Workout, b"selected workout").unwrap()),
            assistance: Assistance::default(),
        };
        let host = HostLobby::open(
            Host::lobby("Host", &options()).await.unwrap(),
            rider("Host"),
        )
        .unwrap();
        let mut client = join(&host, "Friend").await;
        host.select(SharedActivity {
            selection: selection.clone(),
            course: Some(course),
            workout: Some(workout),
        })
        .await
        .unwrap();
        client_event(&mut client, |e| matches!(e, LobbyEvent::Selection(_))).await;
        let link = client.content_link().unwrap();
        let received = content::fetch(&link, selection.course.unwrap(), &path, None)
            .await
            .unwrap();
        assert_eq!(
            received.id(),
            ContentId::of(ContentKind::Course, b"renamed course").unwrap()
        );
        assert_eq!(std::fs::read(received.path()).unwrap(), b"renamed course");
        drop(received);
        assert!(
            content::fetch(&link, selection.workout.unwrap(), &path, None)
                .await
                .is_err()
        );
        drop(client);
        drop(host);
        std::fs::remove_dir_all(path).unwrap();
    }

    fn previewed(name: &str) -> crate::preview::Preview {
        crate::preview::Preview {
            course: Some(crate::preview::CoursePreview {
                name: name.into(),
                length: Meters(18_400.0),
                climbing: Meters(1210.0),
                steepest: 12.5,
                track: vec![[0.0, 0.0], [120.0, 450.0]],
                profile: vec![[0.0, 434.0], [18_400.0, 1607.0]],
                picture: Vec::new(),
            }),
            workout: None,
        }
    }
    fn preview_id(name: &str) -> crate::activity::ContentId {
        use crate::activity::{ContentId, ContentKind};
        ContentId::of(ContentKind::Preview, &previewed(name).encode().unwrap()).unwrap()
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)] // one lobby follows a preview from browsing to the countdown
    async fn friends_see_what_the_organizer_chooses_beside_a_download_until_the_countdown() {
        use crate::activity::{Assistance, ContentKind, CourseKey, Selection};
        use crate::content::{self, Progress, Source};
        use crate::preview::Preview;
        let path = directory("previews");
        let mut host = HostLobby::open(
            Host::lobby("Host", &options()).await.unwrap(),
            rider("Host"),
        )
        .unwrap();
        let mut client = join(&host, "Friend").await;
        let link = client.content_link().unwrap();
        // Before anything is shared, friends see what the organizer is looking at.
        host.preview(Some(&previewed("Chasseral"))).unwrap();
        let shown = preview_id("Chasseral");
        client_event(
            &mut client,
            |e| matches!(e, LobbyEvent::Preview(Some(id)) if *id == shown),
        )
        .await;
        let fetched = content::fetch_preview(&link, shown).await.unwrap();
        assert_eq!(Preview::decode(&fetched).unwrap(), previewed("Chasseral"));
        // Browsing on, friends hear of the latest; what it replaced is no longer served.
        for name in ["Weissenstein", "Grenchenberg", "Balmberg"] {
            host.preview(Some(&previewed(name))).unwrap();
        }
        let latest = preview_id("Balmberg");
        client_event(
            &mut client,
            |e| matches!(e, LobbyEvent::Preview(Some(id)) if *id == latest),
        )
        .await;
        assert!(content::fetch_preview(&link, shown).await.is_err());
        // A friend joining now sees it at once.
        let mut late = join(&host, "Late").await;
        client_event(
            &mut late,
            |e| matches!(e, LobbyEvent::Preview(Some(id)) if *id == latest),
        )
        .await;
        // A preview arrives beside a download of the shared course, which it leaves going.
        let file = path.join("course.oqc");
        let bytes: Vec<u8> = (0..1024 * 1024_u32)
            .map(|n| u8::try_from(n % 251).unwrap())
            .collect();
        std::fs::write(&file, &bytes).unwrap();
        let source = Source::open(&file, ContentKind::Course).await.unwrap();
        host.select(SharedActivity {
            selection: Selection {
                revision: 1,
                name: "Balmberg".into(),
                course: Some(source.id()),
                course_key: Some(CourseKey::of(b"Balmberg")),
                workout: None,
                assistance: Assistance::default(),
            },
            course: Some(file),
            workout: None,
        })
        .await
        .unwrap();
        client_event(&mut client, |e| matches!(e, LobbyEvent::Selection(_))).await;
        let (progress, mut seen) = watch::channel(Progress {
            received: 0,
            total: 0,
        });
        let download = content::fetch(&link, source.id(), &path, Some(&progress));
        tokio::pin!(download);
        tokio::select! {
            _ = &mut download => panic!("the whole course arrived at once"),
            _ = seen.wait_for(|p| p.received > 0) => {},
        }
        host.preview(Some(&previewed("Hasenmatt"))).unwrap();
        let next = preview_id("Hasenmatt");
        client_event(
            &mut client,
            |e| matches!(e, LobbyEvent::Preview(Some(id)) if *id == next),
        )
        .await;
        let fetched = content::fetch_preview(&link, next).await.unwrap();
        assert_eq!(Preview::decode(&fetched).unwrap(), previewed("Hasenmatt"));
        let received = download.await.unwrap();
        assert_eq!(std::fs::read(received.path()).unwrap(), bytes);
        drop(received);
        // Putting it away tells everyone.
        host.preview(None).unwrap();
        client_event(&mut client, |e| matches!(e, LobbyEvent::Preview(None))).await;
        // From the countdown on, the activity is frozen and nothing more is previewed.
        host.ready(1, true).await.unwrap();
        for rider in [&client, &late] {
            rider.ready(1, true).unwrap();
        }
        all_ready(&mut host, vec![client.me(), late.me()]).await;
        host.start(Duration::from_secs(1)).await.unwrap();
        client_event(&mut client, |e| matches!(e, LobbyEvent::Start { .. })).await;
        host.preview(Some(&previewed("Too late"))).unwrap();
        let quiet = timeout(Duration::from_secs(1), async {
            loop {
                if matches!(client.next().await, Some(LobbyEvent::Preview(_))) {
                    break;
                }
            }
        })
        .await;
        assert!(quiet.is_err(), "a preview after the countdown");
        assert!(
            content::fetch_preview(&link, preview_id("Too late"))
                .await
                .is_err()
        );
        drop((client, late, host));
        std::fs::remove_dir_all(&path).unwrap();
    }

    /// An app lobby sharing a short workout, so it can be started.
    async fn workout_lobby(path: &std::path::Path) -> (HostLobby, crate::activity::Selection) {
        use crate::activity::{Assistance, ContentKind, Selection};
        let file = path.join("source.oqw");
        std::fs::write(&file, b"normalized workout bytes").unwrap();
        let source = crate::content::Source::open(&file, ContentKind::Workout)
            .await
            .unwrap();
        let host = HostLobby::open(
            Host::lobby("Host", &options()).await.unwrap(),
            rider("Host"),
        )
        .unwrap();
        let selection = Selection {
            revision: 1,
            name: "Together".into(),
            course: None,
            course_key: None,
            workout: Some(source.id()),
            assistance: Assistance::default(),
        };
        (host, selection)
    }
    fn directory(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "orqwa-{name}-{:x?}",
            crate::crypto::random::<8>().unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }
    // The network gives up on this rider's connection; nothing says goodbye.
    fn lose(rider: &JoinerLobby) {
        current(&rider.connection).close_for_reconnect();
    }

    #[tokio::test]
    async fn successive_activities_keep_connections_and_reject_previous_activity_motion() {
        let path = directory("reuse-lobby");
        let (mut host, mut selection) = workout_lobby(&path).await;
        let mut peer = join(&host, "Anna").await;
        let connection = current(&peer.connection).clone();
        for revision in 1..=2 {
            selection.revision = revision;
            host.select(SharedActivity {
                selection: selection.clone(),
                course: None,
                workout: Some(path.join("source.oqw")),
            })
            .await
            .unwrap();
            client_event(
                &mut peer,
                |e| matches!(e, LobbyEvent::Selection(s) if s.revision == revision),
            )
            .await;
            assert!(
                host.start(Duration::from_secs(1)).await.is_err(),
                "fresh readiness is required"
            );
            host.ready(revision, true).await.unwrap();
            peer.ready(revision, true).unwrap();
            host_event(
                &mut host,
                |e| matches!(e, LobbyEvent::Readiness {id,ready:true,..} if *id == peer.me()),
            )
            .await;
            let at = host.start(Duration::from_secs(1)).await.unwrap();
            client_event(&mut peer, |e| matches!(e, LobbyEvent::Start { .. })).await;
            tokio::time::sleep_until(at + Duration::from_millis(350)).await;
            if revision == 2 {
                assert!(
                    host.end_activity(1).await.is_err(),
                    "a delayed end cannot stop the next ride"
                );
                connection
                    .send_datagram(
                        &state_datagram(
                            RiderState {
                                at: Duration::from_millis(200),
                                ..RiderState::default()
                            },
                            peer.me(),
                            1,
                        )
                        .unwrap(),
                    )
                    .unwrap();
            }
            let fresh = RiderState {
                at: Duration::from_millis(100),
                ..RiderState::default()
            };
            peer.send_state(fresh).unwrap();
            host_event(&mut host, |e| matches!(e, LobbyEvent::State {id,state} if *id == peer.me() && state.at == fresh.at)).await;
            connection
                .send_datagram(&state_datagram(fresh, peer.me(), revision).unwrap())
                .unwrap();
            host.end_activity(revision).await.unwrap();
            client_event(
                &mut peer,
                |e| matches!(e, LobbyEvent::Lobby {revision:r} if *r == revision),
            )
            .await;
        }
        // Rejoining learns the end even if there is no next selection yet.
        lose(&peer);
        client_event(&mut peer, |e| matches!(e, LobbyEvent::Reconnected)).await;
        client_event(&mut peer, |e| {
            matches!(e, LobbyEvent::Lobby { revision: 2 })
        })
        .await;
        peer.close().await;
        host.close().await;
        std::fs::remove_dir_all(path).unwrap();
    }

    #[tokio::test]
    async fn a_lost_rider_keeps_the_start_waiting_until_back_or_removed_but_a_goodbye_frees_the_seat()
     {
        let path = directory("held-seat");
        let (mut host, selection) = workout_lobby(&path).await;
        let mut anna = join(&host, "Anna").await;
        let mut ben = join(&host, "Ben").await;
        host.select(SharedActivity {
            selection: selection.clone(),
            course: None,
            workout: Some(path.join("source.oqw")),
        })
        .await
        .unwrap();
        for rider in [&mut anna, &mut ben] {
            client_event(rider, |e| matches!(e, LobbyEvent::Selection(_))).await;
            rider.ready(1, true).unwrap();
        }
        host.ready(1, true).await.unwrap();
        all_ready(&mut host, vec![anna.me(), ben.me()]).await;
        // #434: Anna's network drops and she never gets back.
        let gone = anna.me();
        lose(&anna);
        anna.tasks.abort_all();
        host_event(
            &mut host,
            |e| matches!(e, LobbyEvent::Away {id} if *id == gone),
        )
        .await;
        client_event(
            &mut ben,
            |e| matches!(e, LobbyEvent::Away {id} if *id == gone),
        )
        .await;
        assert!(matches!(
            host.start(Duration::from_secs(1)).await,
            Err(LobbyError::Activity(
                crate::activity::ActivityError::NotReady
            ))
        ));
        host.remove(gone).unwrap();
        host_event(
            &mut host,
            |e| matches!(e, LobbyEvent::Left {id} if *id == gone),
        )
        .await;
        client_event(
            &mut ben,
            |e| matches!(e, LobbyEvent::Left {id} if *id == gone),
        )
        .await;
        host.start(Duration::from_secs(1)).await.unwrap();
        // Leaving says goodbye: Ben's seat is not held.
        let left = ben.me();
        drop(ben);
        let event = host_event(&mut host, |e| {
            matches!(e, LobbyEvent::Left { .. } | LobbyEvent::Away { .. })
        })
        .await;
        assert!(matches!(event, LobbyEvent::Left {id} if id == left));
        drop((anna, host));
        std::fs::remove_dir_all(path).unwrap();
    }

    #[tokio::test]
    async fn a_lost_rider_reconnects_to_the_same_seat_and_rides_on_without_new_codes() {
        let path = directory("reconnect");
        let (mut host, selection) = workout_lobby(&path).await;
        let mut anna = join(&host, "Anna").await;
        let mut ben = join(&host, "Ben").await;
        host.select(SharedActivity {
            selection,
            course: None,
            workout: Some(path.join("source.oqw")),
        })
        .await
        .unwrap();
        for rider in [&mut anna, &mut ben] {
            client_event(rider, |e| matches!(e, LobbyEvent::Selection(_))).await;
            rider.ready(1, true).unwrap();
        }
        host.ready(1, true).await.unwrap();
        all_ready(&mut host, vec![anna.me(), ben.me()]).await;
        host.start(Duration::from_millis(1000)).await.unwrap();
        for rider in [&mut anna, &mut ben] {
            client_event(rider, |e| matches!(e, LobbyEvent::Start { .. })).await;
        }
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let riding = |metres: f64| RiderState {
            at: Duration::from_secs(1),
            distance: Meters(metres),
            ..RiderState::default()
        };
        let seat = anna.me();
        // #434: Anna's connection is lost mid-ride; she gets her seat back by herself.
        lose(&anna);
        let LobbyEvent::Reconnecting { until } =
            client_event(&mut anna, |e| matches!(e, LobbyEvent::Reconnecting { .. })).await
        else {
            unreachable!()
        };
        // She keeps trying until the organizer has surely released her seat (#536).
        assert!(until > Instant::now() + RECONNECT_WINDOW);
        host_event(
            &mut host,
            |e| matches!(e, LobbyEvent::Away {id} if *id == seat),
        )
        .await;
        client_event(
            &mut ben,
            |e| matches!(e, LobbyEvent::Away {id} if *id == seat),
        )
        .await;
        // Her first attempt may have come before the organizer noticed; try again now.
        anna.reconnect();
        let resumed = client_event(&mut anna, |e| {
            matches!(
                e,
                LobbyEvent::Reconnected | LobbyEvent::Running { .. } | LobbyEvent::HostLeft
            )
        })
        .await;
        assert!(matches!(resumed, LobbyEvent::Reconnected), "{resumed:?}");
        assert_eq!(anna.me(), seat);
        host_event(
            &mut host,
            |e| matches!(e, LobbyEvent::Back {id} if *id == seat),
        )
        .await;
        client_event(
            &mut ben,
            |e| matches!(e, LobbyEvent::Back {id} if *id == seat),
        )
        .await;
        // She had entered the ride, so her state goes straight to the others again.
        anna.send_state(riding(40.0)).unwrap();
        let LobbyEvent::State { state, .. } = client_event(
            &mut ben,
            |e| matches!(e, LobbyEvent::State {id, ..} if *id == seat),
        )
        .await
        else {
            unreachable!()
        };
        assert_eq!(state.distance, Meters(40.0));
        assert!(!current(&anna.connection).lost());
        drop((anna, ben));
        host.close().await;
        std::fs::remove_dir_all(path).unwrap();
    }

    #[tokio::test]
    async fn a_removed_rider_added_again_gets_their_old_seat() {
        let mut host = HostLobby::open(
            Host::lobby("Host", &options()).await.unwrap(),
            rider("Host"),
        )
        .unwrap();
        let anna = join(&host, "Anna").await;
        let credential = crate::link::Credential::generate().unwrap();
        let ben = join_as(&host, "Ben", &credential).await;
        let (first, seat) = (anna.me(), ben.me());
        for gone in [first, seat] {
            host.remove(gone).unwrap();
            host_event(
                &mut host,
                |e| matches!(e, LobbyEvent::Left {id} if *id == gone),
            )
            .await;
        }
        anna.close().await;
        ben.close().await;
        // Anna's seat comes first, but the organizer adding Ben's certificate again gives
        // him his own back (#536); a newcomer takes the free one.
        let ben = join_as(&host, "Ben", &credential).await;
        assert_eq!(ben.me(), seat);
        let carl = join(&host, "Carl").await;
        assert_eq!(carl.me(), first);
        drop((ben, carl));
        host.close().await;
    }

    #[tokio::test]
    async fn a_held_seat_is_given_up_after_the_reconnection_window() {
        let path = directory("seat-expiry");
        let (mut host, _) = workout_lobby(&path).await;
        let anna = join(&host, "Anna").await;
        let gone = anna.me();
        lose(&anna);
        let mut anna = anna;
        anna.tasks.abort_all();
        host_event(
            &mut host,
            |e| matches!(e, LobbyEvent::Away {id} if *id == gone),
        )
        .await;
        tokio::time::pause();
        tokio::time::advance(RECONNECT_WINDOW + Duration::from_secs(1)).await;
        host_event(
            &mut host,
            |e| matches!(e, LobbyEvent::Left {id} if *id == gone),
        )
        .await;
        drop((anna, host));
        std::fs::remove_dir_all(path).unwrap();
    }

    fn options() -> ConnectOptions {
        ConnectOptions {
            gather: GatherOptions {
                policy: AddressPolicy {
                    lan: false,
                    loopback: true,
                },
                stun_servers: vec![],
                ..GatherOptions::default()
            },
            window: Duration::from_secs(2),
            ..ConnectOptions::default()
        }
    }
    fn rider(name: &str) -> Rider {
        Rider {
            name: name.into(),
            mass: Kilograms(83.0),
            weight: Kilograms(75.0),
            avatar: Avatar::Female,
        }
    }
    async fn host() -> HostLobby {
        HostLobby::open(
            Host::open("Host", "Test course", RouteHash([7; 32]), &options())
                .await
                .unwrap(),
            rider("Host"),
        )
        .unwrap()
    }
    async fn join_as(
        host: &HostLobby,
        name: &str,
        credential: &crate::link::Credential,
    ) -> JoinerLobby {
        let joiner = Joiner::lobby_as(host.invite().clone(), &options(), credential)
            .await
            .unwrap();
        host.admit(joiner.reply()).await.unwrap();
        JoinerLobby::join(joiner, rider(name))
            .await
            .unwrap_or_else(|error| panic!("join {name}: {error}"))
    }
    async fn join(host: &HostLobby, name: &str) -> JoinerLobby {
        let joiner = Joiner::open(host.invite().clone(), host.invite().route, &options())
            .await
            .unwrap();
        host.admit(joiner.reply()).await.unwrap();
        JoinerLobby::join(joiner, rider(name))
            .await
            .unwrap_or_else(|error| panic!("join {name}: {error}"))
    }
    async fn client_event(
        client: &mut JoinerLobby,
        predicate: impl Fn(&LobbyEvent) -> bool,
    ) -> LobbyEvent {
        timeout(Duration::from_secs(2), async {
            loop {
                let event = client.next().await.expect("client event");
                if predicate(&event) {
                    return event;
                }
            }
        })
        .await
        .unwrap()
    }
    /// Waits for every rider's readiness in whatever order the organizer receives it: waiting
    /// for one rider at a time would discard another's that arrived first.
    async fn all_ready(host: &mut HostLobby, mut riders: Vec<RiderId>) {
        while !riders.is_empty() {
            let LobbyEvent::Readiness { id, .. } = host_event(
                host,
                |e| matches!(e, LobbyEvent::Readiness {id, ready: true, ..} if riders.contains(id)),
            )
            .await
            else {
                unreachable!("only readiness matches");
            };
            riders.retain(|rider| *rider != id);
        }
    }
    async fn host_event(
        host: &mut HostLobby,
        predicate: impl Fn(&LobbyEvent) -> bool,
    ) -> LobbyEvent {
        timeout(Duration::from_secs(2), async {
            loop {
                let event = host.next().await.expect("host event");
                if predicate(&event) {
                    return event;
                }
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn a_peer_clock_slightly_ahead_counts_from_the_start_but_lobby_state_does_not() {
        let start = Instant::now() + Duration::from_secs(1);
        assert_eq!(peer_elapsed(None), None);
        assert_eq!(peer_elapsed(Some(start)), Some(Duration::ZERO));
        assert_eq!(
            peer_elapsed(Some(Instant::now() + crate::state::MAX_FUTURE * 2)),
            None
        );
        tokio::time::advance(Duration::from_millis(1500)).await;
        assert_eq!(peer_elapsed(Some(start)), Some(Duration::from_millis(500)));
    }

    #[tokio::test]
    async fn state_arriving_just_before_the_hosts_start_keeps_the_rider_at_the_start_line() {
        let mut host = host().await;
        let anna = join(&host, "Anna").await;
        let mut ben = join(&host, "Ben").await;
        let cara = join(&host, "Cara").await;
        let early = |me: RiderId| {
            RiderState {
                at: Duration::from_millis(1),
                distance: Meters(3.0),
                ..RiderState::default()
            }
            .datagram(me)
            .unwrap()
        };
        // Ride state during the lobby is still a protocol violation.
        current(&cara.connection)
            .send_datagram(&early(cara.me()))
            .unwrap();
        let LobbyEvent::Left { id } =
            host_event(&mut host, |e| matches!(e, LobbyEvent::Left { .. })).await
        else {
            panic!("left");
        };
        assert_eq!(id, cara.me());
        host.start(Duration::from_secs(1)).await.unwrap();
        client_event(&mut ben, |e| matches!(e, LobbyEvent::Start { .. })).await;
        // Anna's start estimate runs ahead of the host's: her first state arrives early.
        current(&anna.connection)
            .send_datagram(&early(anna.me()))
            .unwrap();
        let LobbyEvent::State { id, state } = client_event(&mut ben, |e| {
            matches!(e, LobbyEvent::State { .. } | LobbyEvent::Left { .. })
        })
        .await
        else {
            panic!("Anna was disconnected at the start line");
        };
        assert_eq!(id, anna.me());
        assert_eq!(state.distance, Meters(3.0));
        drop((anna, cara));
        ben.close().await;
        host.close().await;
    }

    #[tokio::test]
    async fn riders_share_countdown_latest_states_and_departures_without_remote_ride_control() {
        let mut host = host().await;
        let mut anna = join(&host, "Anna").await;
        let mut ben = join(&host, "Ben").await;
        let at = host.start(Duration::from_secs(1)).await.unwrap();
        let LobbyEvent::Start { at: anna_at } =
            client_event(&mut anna, |e| matches!(e, LobbyEvent::Start { .. })).await
        else {
            panic!("start");
        };
        assert!(anna_at.max(at) - anna_at.min(at) < Duration::from_millis(100));
        client_event(&mut ben, |e| matches!(e, LobbyEvent::Start { .. })).await;
        tokio::time::sleep_until(at + Duration::from_millis(5)).await;
        // High-frequency local updates replace one watch value before the 5 Hz publisher.
        for distance in 0..1000 {
            anna.send_state(RiderState {
                at: Duration::from_millis(1),
                distance: Meters(f64::from(distance)),
                power: Watts(222.0),
                ..RiderState::default()
            })
            .unwrap();
        }
        let LobbyEvent::State { id, state } =
            client_event(&mut ben, |e| matches!(e, LobbyEvent::State { .. })).await
        else {
            panic!("state");
        };
        assert_eq!(id, anna.me());
        assert_eq!(state.distance, Meters(999.0));
        assert_eq!(state.power, Watts(222.0));
        assert!(
            timeout(Duration::from_millis(250), ben.next())
                .await
                .is_err(),
            "no duplicate backlog"
        );
        anna.close().await;
        client_event(&mut ben, |e| matches!(e, LobbyEvent::Left { .. })).await;
        host_event(&mut host, |e| matches!(e, LobbyEvent::Left { .. })).await;
        drop(host);
        client_event(&mut ben, |e| matches!(e, LobbyEvent::HostLeft)).await;
        assert!(ben.next().await.is_none());
        // Local ride code can continue producing state even after all networking ends.
        ben.send_state(RiderState::default()).unwrap();
        ben.close().await;
    }

    #[tokio::test]
    async fn full_rosters_and_started_lobbies_cannot_admit_late_riders() {
        let host = host().await;
        let mut clients = Vec::new();
        for number in 1..MAX_RIDERS {
            clients.push(join(&host, &format!("Rider {number}")).await);
        }
        let ninth = Joiner::open(host.invite().clone(), host.invite().route, &options())
            .await
            .unwrap();
        host.admit(ninth.reply()).await.unwrap();
        assert!(JoinerLobby::join(ninth, rider("Ninth")).await.is_err());
        assert_eq!(clients.len(), MAX_RIDERS - 1);
        host.start(Duration::from_secs(1)).await.unwrap();
        assert!(matches!(
            host.start(Duration::from_secs(1)).await,
            Err(LobbyError::Started)
        ));
        let late = Joiner::open(host.invite().clone(), host.invite().route, &options())
            .await
            .unwrap();
        assert!(matches!(
            host.admit(late.reply()).await,
            Err(LobbyError::Started)
        ));
        drop(late);
        drop(clients);
        host.close().await;
    }

    async fn raw_join(host: &HostLobby) -> (Connection, SendStream, RecvStream, RiderId) {
        let joiner = Joiner::open(host.invite().clone(), host.invite().route, &options())
            .await
            .unwrap();
        host.admit(joiner.reply()).await.unwrap();
        let connection = joiner.connect().await.unwrap();
        let (mut send, mut recv) = connection.open_bi().await.unwrap();
        wire::write(
            &mut send,
            &Message::Hello(host.invite().route, rider("Peer")),
        )
        .await
        .unwrap();
        let Message::Welcome(id, _, _, _, _) = wire::read(&mut recv).await.unwrap() else {
            panic!("welcome");
        };
        (connection, send, recv, id)
    }

    async fn raw_organizer() -> (Host, Connection, SendStream, RecvStream, JoinerLobby) {
        let mut host = Host::open("Host", "Test course", RouteHash([9; 32]), &options())
            .await
            .unwrap();
        let joiner = Joiner::open(host.invite().clone(), host.invite().route, &options())
            .await
            .unwrap();
        host.admit(joiner.reply()).unwrap();
        let joining = tokio::spawn(JoinerLobby::join(joiner, rider("Peer")));
        let Some(HostEvent::Joined(connection)) = host.next().await else {
            panic!("connection");
        };
        let (mut send, mut recv) = connection.accept_bi().await.unwrap();
        assert!(matches!(
            wire::read(&mut recv).await.unwrap(),
            Message::Hello(..)
        ));
        let me = rider_id(1);
        let roster =
            std::array::from_fn(|slot| Some(rider(if slot == 1 { "Peer" } else { "Rider" })));
        wire::write(
            &mut send,
            &Message::Welcome(me, RiderId::HOST, host.invite().ride, 1, roster),
        )
        .await
        .unwrap();
        wire::write(
            &mut send,
            &Message::VoiceRoster(std::array::from_fn(|slot| u64::try_from(slot + 1).unwrap())),
        )
        .await
        .unwrap();
        let peer = joining.await.unwrap().unwrap();
        (host, connection, send, recv, peer)
    }

    #[tokio::test]
    async fn forwarded_voice_bursts_recover_without_losing_voice_or_group_controls() {
        let (host, connection, mut send, mut recv, mut peer) = raw_organizer().await;
        let voice = peer.take_voice().unwrap();
        voice.enable(true);
        assert!(matches!(
            wire::read(&mut recv).await.unwrap(),
            Message::VoiceEnabled(true)
        ));
        let speech = |slot: usize, sequence: u32| {
            let (mut bytes, len) = voice::VoiceFrame::new(
                rider_id(slot),
                1,
                sequence,
                &[u8::try_from(sequence).unwrap()],
            )
            .unwrap()
            .datagram();
            bytes[11..19].copy_from_slice(&u64::try_from(slot + 1).unwrap().to_be_bytes());
            bytes[..len].to_vec()
        };
        connection.send_datagram(&speech(0, 0)).unwrap();
        wait_for_voice(&voice, RiderId::HOST, 0).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        // Simulate a network/receiver stall releasing several speakers together.
        for sequence in 1..=12 {
            for slot in 0..MAX_RIDERS {
                if slot != 1 {
                    connection.send_datagram(&speech(slot, sequence)).unwrap();
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
        connection.send_datagram(&speech(0, 13)).unwrap();
        wait_for_voice(&voice, RiderId::HOST, 13).await;
        wire::write(&mut send, &Message::Start(0)).await.unwrap();
        client_event(&mut peer, |event| matches!(event, LobbyEvent::Start { .. })).await;
        assert!(
            voice.alive(),
            "voice overload must not disconnect the group"
        );
        peer.close().await;
        host.close().await;
    }

    async fn wait_for_voice(port: &VoicePort, speaker: RiderId, payload: u8) {
        timeout(Duration::from_secs(2), async {
            loop {
                assert!(port.alive(), "valid voice burst disconnected the listener");
                let playout = port.playout();
                if matches!(&playout[usize::from(speaker.value())].action, voice::Playout::Frame(frame) if frame.payload() == [payload]) {break;}
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.unwrap();
    }

    #[tokio::test]
    async fn peer_voice_and_state_bursts_keep_both_rosters_and_other_riders_connected() {
        let mut host = host().await;
        let voice = host.take_voice().unwrap();
        voice.enable(true);
        let mut healthy = join(&host, "Listening rider").await;
        let listening = healthy.take_voice().unwrap();
        listening.enable(true);
        let (connection, mut send, _recv, id) = raw_join(&host).await;
        wire::write(&mut send, &Message::VoiceEnabled(true))
            .await
            .unwrap();
        let speech = |sequence: u32| {
            let (bytes, len) =
                voice::VoiceFrame::new(id, 1, sequence, &[u8::try_from(sequence).unwrap()])
                    .unwrap()
                    .datagram();
            bytes[..len].to_vec()
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        connection.send_datagram(&speech(0)).unwrap();
        wait_for_voice(&voice, id, 0).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        for sequence in 1..=25 {
            connection.send_datagram(&speech(sequence)).unwrap();
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
        connection.send_datagram(&speech(26)).unwrap();
        wait_for_voice(&voice, id, 26).await;
        wait_for_voice(&listening, id, 26).await;
        let at = host.start(Duration::from_secs(1)).await.unwrap();
        client_event(&mut healthy, |event| {
            matches!(event, LobbyEvent::Start { .. })
        })
        .await;
        tokio::time::sleep_until(at).await;
        for millis in 0..25 {
            connection
                .send_datagram(
                    &RiderState {
                        at: Duration::from_millis(millis),
                        ..RiderState::default()
                    }
                    .datagram(id)
                    .unwrap(),
                )
                .unwrap();
        }
        tokio::time::sleep(STATE_EVERY + Duration::from_millis(20)).await;
        let fresh = RiderState {
            at: Duration::from_millis(200),
            distance: Meters(12.0),
            ..RiderState::default()
        };
        connection
            .send_datagram(&fresh.datagram(id).unwrap())
            .unwrap();
        host_event(&mut host, |event| matches!(event,LobbyEvent::State {id: source, state} if *source == id && state.distance == fresh.distance)).await;
        client_event(&mut healthy, |event| matches!(event,LobbyEvent::State {id: source, state} if *source == id && state.distance == fresh.distance)).await;
        assert!(voice.alive() && listening.alive());
        connection.close();
        healthy.close().await;
        host.close().await;
    }

    #[tokio::test]
    async fn forwarded_state_bursts_keep_the_group_and_resume_fresh_positions() {
        let (host, connection, mut send, _recv, mut peer) = raw_organizer().await;
        wire::write(&mut send, &Message::Start(0)).await.unwrap();
        client_event(&mut peer, |event| matches!(event, LobbyEvent::Start { .. })).await;
        for millis in 0..15 {
            for slot in 0..MAX_RIDERS {
                if slot != 1 {
                    connection
                        .send_datagram(
                            &RiderState {
                                at: Duration::from_millis(millis),
                                ..RiderState::default()
                            }
                            .datagram(rider_id(slot))
                            .unwrap(),
                        )
                        .unwrap();
                }
            }
        }
        tokio::time::sleep(STATE_EVERY + Duration::from_millis(20)).await;
        let fresh = RiderState {
            at: Duration::from_millis(200),
            distance: Meters(19.0),
            ..RiderState::default()
        };
        connection
            .send_datagram(&fresh.datagram(RiderId::HOST).unwrap())
            .unwrap();
        client_event(&mut peer, |event| matches!(event,LobbyEvent::State {id, state} if *id == RiderId::HOST && state.distance == fresh.distance)).await;
        peer.close().await;
        host.close().await;
    }

    #[tokio::test]
    async fn a_joiner_cannot_send_organizer_workout_time_or_stop_the_lobby() {
        for message in [
            Message::Lobby(1),
            Message::Workout(timing::WorkoutSample {
                revision: 1,
                sequence: 1,
                group: Duration::ZERO,
                elapsed: Duration::from_mins(15),
                paused: true,
            }),
        ] {
            let mut host = HostLobby::open(
                Host::lobby("Host", &options()).await.unwrap(),
                rider("Host"),
            )
            .unwrap();
            let (_connection, mut send, _recv, id) = raw_join(&host).await;
            wire::write(&mut send, &message).await.unwrap();
            host_event(
                &mut host,
                |event| matches!(event, LobbyEvent::Left {id: gone} if *gone == id),
            )
            .await;
            let mut other = join(&host, "Other").await;
            client_event(
                &mut other,
                |event| matches!(event, LobbyEvent::Joined {id, ..} if *id == RiderId::HOST),
            )
            .await;
            other.close().await;
            host.close().await;
        }
    }

    #[tokio::test]
    async fn forged_senders_invalid_physics_future_timestamps_and_floods_disconnect_only_the_offender()
     {
        for attack in 0..4 {
            let mut host = host().await;
            let (connection, _send, _recv, id) = raw_join(&host).await;
            let at = host.start(Duration::from_secs(1)).await.unwrap();
            tokio::time::sleep_until(at).await;
            let mut bytes = RiderState::default().datagram(id).unwrap();
            match attack {
                0 => bytes[1] = RiderId::HOST.value(),
                1 => bytes[6..10].copy_from_slice(&f32::NAN.to_be_bytes()),
                2 => bytes[2..6].copy_from_slice(&100_000_u32.to_be_bytes()),
                _ => {}
            }
            if attack == 3 {
                flood_until_closed(&connection, &bytes).await;
            } else {
                connection.send_datagram(&bytes).unwrap();
            }
            timeout(Duration::from_secs(1), connection.closed())
                .await
                .unwrap();
            host_event(&mut host, |e| matches!(e, LobbyEvent::Left { .. })).await;
            host.send_state(RiderState::default()).unwrap();
            assert!(!host.tasks.is_empty());
            host.close().await;
        }
    }

    #[tokio::test]
    async fn a_pinned_host_cannot_extend_the_join_deadline_by_stalling_its_lobby_welcome() {
        let mut settings = options();
        settings.window = Duration::from_millis(250);
        let mut host = Host::open("Host", "Test course", RouteHash([7; 32]), &settings)
            .await
            .unwrap();
        let joiner = Joiner::open(host.invite().clone(), host.invite().route, &settings)
            .await
            .unwrap();
        host.admit(joiner.reply()).unwrap();
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            let result = JoinerLobby::join(joiner, rider("Anna")).await;
            assert!(matches!(result, Err(LobbyError::Link(LinkError::Timeout))));
        });
        let Some(HostEvent::Joined(connection)) = host.next().await else {
            panic!("connection");
        };
        let (_send, mut recv) = connection.accept_bi().await.unwrap();
        assert!(matches!(
            wire::read(&mut recv).await,
            Ok(Message::Hello(_, _))
        ));
        timeout(Duration::from_millis(500), tasks.join_next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        host.close().await;
    }

    #[tokio::test]
    async fn dropping_lobby_owners_releases_their_actual_udp_ports() {
        let host = host().await;
        let mut addresses: Vec<_> = host
            .invite()
            .candidates
            .iter()
            .map(|candidate| candidate.addr())
            .collect();
        let joiner = Joiner::open(host.invite().clone(), host.invite().route, &options())
            .await
            .unwrap();
        addresses.extend(
            joiner
                .reply()
                .candidates
                .iter()
                .map(|candidate| candidate.addr()),
        );
        host.admit(joiner.reply()).await.unwrap();
        let client = JoinerLobby::join(joiner, rider("Anna")).await.unwrap();
        drop(client);
        drop(host);
        timeout(Duration::from_secs(1), async {
            loop {
                if addresses
                    .iter()
                    .all(|address| std::net::UdpSocket::bind(address).is_ok())
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn slow_event_consumers_coalesce_updates_and_do_not_resurrect_departed_riders() {
        let (send, receive) = mpsc::channel(CONTROL_CAPACITY);
        let (states, updates) = watch::channel([None; MAX_RIDERS]);
        let (clock, clock_updates) = watch::channel(None);
        let mut events = events(RiderId::HOST, receive, updates, clock_updates, false);
        let id = rider_id(1);
        send.try_send(LobbyEvent::Joined {
            id,
            rider: rider("Anna"),
        })
        .unwrap();
        assert!(matches!(
            events.next().await,
            Some(LobbyEvent::Joined { .. })
        ));
        for sequence in 1..=1000 {
            clock.send_replace(Some(WorkoutClock {
                revision: 1,
                sequence,
                elapsed: Duration::from_secs(sequence),
                at: std::time::Instant::now(),
                paused: false,
            }));
        }
        assert!(matches!(
            events.next().await,
            Some(LobbyEvent::Workout(WorkoutClock { sequence: 1000, .. }))
        ));
        for sample in 0..1000 {
            let mut snapshot = [None; MAX_RIDERS];
            snapshot[1] = Some(RiderState {
                distance: Meters(f64::from(sample)),
                ..RiderState::default()
            });
            states.send_replace(snapshot);
        }
        let Some(LobbyEvent::State { state, .. }) = events.next().await else {
            panic!("state");
        };
        assert_eq!(state.distance, Meters(999.0));
        states.send_replace([Some(RiderState::default()); MAX_RIDERS]);
        send.try_send(LobbyEvent::Left { id }).unwrap();
        assert!(matches!(events.next().await, Some(LobbyEvent::Left { .. })));
        assert!(
            timeout(Duration::from_millis(50), events.next())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn cancelling_lobby_admission_releases_the_joiners_socket_while_the_host_stalls() {
        let mut host = Host::open("Host", "Test course", RouteHash([7; 32]), &options())
            .await
            .unwrap();
        let joiner = Joiner::open(host.invite().clone(), host.invite().route, &options())
            .await
            .unwrap();
        let addresses: Vec<_> = joiner
            .reply()
            .candidates
            .iter()
            .map(|candidate| candidate.addr())
            .collect();
        host.admit(joiner.reply()).unwrap();
        let mut tasks = JoinSet::new();
        tasks.spawn(JoinerLobby::join(joiner, rider("Anna")));
        let Some(HostEvent::Joined(connection)) = host.next().await else {
            panic!("connection");
        };
        let (_send, mut recv) = connection.accept_bi().await.unwrap();
        assert!(matches!(
            wire::read(&mut recv).await,
            Ok(Message::Hello(_, _))
        ));
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        timeout(Duration::from_secs(1), async {
            while !addresses
                .iter()
                .all(|address| std::net::UdpSocket::bind(address).is_ok())
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        host.close().await;
    }

    #[tokio::test]
    async fn voice_sender_forgery_packet_floods_and_control_floods_close_only_the_offending_peer() {
        for attack in 0..3 {
            let mut host = host().await;
            let mut healthy = join(&host, "Healthy").await;
            let (connection, mut send, _recv, id) = raw_join(&host).await;
            if attack == 2 {
                for _ in 0..12 {
                    let _ = wire::write(&mut send, &Message::VoiceEnabled(true)).await;
                }
            } else {
                let source = if attack == 0 { RiderId::HOST } else { id };
                let frame = voice::VoiceFrame::new(source, 1, 0, &[1]).unwrap();
                let (bytes, len) = frame.datagram();
                if attack == 1 {
                    flood_until_closed(&connection, &bytes[..len]).await;
                } else {
                    connection.send_datagram(&bytes[..len]).unwrap();
                }
            }
            timeout(Duration::from_secs(1), connection.closed())
                .await
                .unwrap();
            let failure = host_event(&mut host, |event| {
                matches!(event, LobbyEvent::Failed { .. })
            })
            .await;
            let LobbyEvent::Failed {
                peer,
                error,
                operation,
            } = failure
            else {
                panic!("diagnostic");
            };
            assert_eq!(peer, Some(id));
            assert_eq!(operation, "receiving peer controls");
            assert!(
                matches!(
                    (attack, error.as_ref()),
                    (0, LobbyError::Voice(voice::VoiceError::Invalid))
                        | (1, LobbyError::DatagramFlood)
                        | (2, LobbyError::ControlFlood)
                ),
                "original reader/voice error must reach the organizer: {error}"
            );
            host_event(&mut host, |event| matches!(event, LobbyEvent::Left { .. })).await;
            assert!(!host.tasks.is_empty());
            host.start(Duration::from_secs(1)).await.unwrap();
            client_event(&mut healthy, |event| {
                matches!(event, LobbyEvent::Start { .. })
            })
            .await;
            healthy.close().await;
            host.close().await;
        }
    }

    // Yield between small batches so loopback QUIC actually delivers sustained
    // excess, rather than merely replacing the sender's bounded datagram queue.
    async fn flood_until_closed(connection: &Connection, bytes: &[u8]) {
        timeout(Duration::from_secs(6), async {
            let mut tick = interval(Duration::from_millis(2));
            loop {
                tokio::select! {
                    () = connection.closed() => break,
                    _ = tick.tick() => for _ in 0..32 {
                        let _ = connection.send_datagram(bytes);
                    },
                }
            }
        })
        .await
        .expect("persistent raw datagram flood must close its connection");
    }

    #[tokio::test]
    async fn sustained_organizer_datagram_abuse_is_distinct_from_a_voice_delivery_burst() {
        let (host, connection, _send, _recv, mut peer) = raw_organizer().await;
        let (mut bytes, len) = voice::VoiceFrame::new(RiderId::HOST, 1, 0, &[1])
            .unwrap()
            .datagram();
        bytes[11..19].copy_from_slice(&1_u64.to_be_bytes());
        flood_until_closed(&connection, &bytes[..len]).await;
        let failure = client_event(&mut peer, |event| {
            matches!(event, LobbyEvent::Failed { .. })
        })
        .await;
        assert!(
            matches!(failure, LobbyEvent::Failed {error, ..} if matches!(error.as_ref(), LobbyError::DatagramFlood))
        );
        client_event(&mut peer, |event| matches!(event, LobbyEvent::HostLeft)).await;
        peer.close().await;
        host.close().await;
    }

    #[tokio::test]
    async fn a_partial_control_reports_its_timeout_before_the_peer_departs() {
        let mut host = host().await;
        let (connection, mut send, _recv, id) = raw_join(&host).await;
        send.write_all(&[0]).await.unwrap();
        let failure = timeout(Duration::from_secs(4), async {
            loop {
                if let Some(LobbyEvent::Failed { peer, error, .. }) = host.next().await {
                    break (peer, error);
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(failure.0, Some(id));
        assert!(matches!(failure.1.as_ref(), LobbyError::ControlTimeout));
        host_event(
            &mut host,
            |event| matches!(event, LobbyEvent::Left {id: peer} if *peer == id),
        )
        .await;
        connection.close();
        let mut healthy = join(&host, "Healthy").await;
        host.start(Duration::from_secs(1)).await.unwrap();
        client_event(&mut healthy, |event| {
            matches!(event, LobbyEvent::Start { .. })
        })
        .await;
        healthy.close().await;
        host.close().await;
    }
}
