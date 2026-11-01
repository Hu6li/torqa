//! Eight-rider star lobby with bounded controls, coalesced state and owned background tasks.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use orqwa_domain::units::Kilograms;
use quinn::{RecvStream, SendStream};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinSet;
use tokio::time::{Instant, interval, timeout, timeout_at};

use crate::code::{Invite, Reply, RideId, RouteHash, valid_name};
use crate::link::{Connection, Host, HostEvent, Joiner, LinkError};
use crate::state::{
    MAX_RIDERS, Membership, RiderId, RiderState, STATE_EVERY, StateError, StateInbox,
};
use crate::voice::{self, VoiceHandle, VoiceOwner, VoicePort};
use crate::wire::{self, Message};

mod activity;
pub(crate) mod entry;
mod replacement;
pub(crate) mod timing;
use activity::Activities;
pub use activity::SharedActivity;
pub use entry::RideProgress;
use replacement::Prepared;
pub use replacement::Replacement;
use timing::Timeline;
pub use timing::WorkoutClock;

const CONTROL_CAPACITY: usize = 32;
const WRITE_WAIT: Duration = Duration::from_secs(2);
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
}
impl Rider {
    pub(crate) fn validate(&self) -> Result<(), LobbyError> {
        if !valid_name(&self.name)
            || !self.mass.0.is_finite()
            || !(20.0..=300.0).contains(&self.mass.0)
        {
            return Err(LobbyError::Protocol);
        }
        Ok(())
    }
}

/// Reliable membership/countdown events and disposable latest-state updates.
#[derive(Clone, Debug)]
pub enum LobbyEvent {
    /// The host selected new content; previous local preparation is obsolete.
    Selection(crate::activity::Selection),
    /// One current participant verified/prepared this revision, or withdrew readiness.
    Readiness {
        /// Authenticated roster slot, including the local rider.
        id: RiderId,
        /// Selection this acknowledgment applies to.
        revision: u64,
        /// Whether local preparation completed.
        ready: bool,
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
    /// One validated latest sample, at most five times per second per rider.
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
}

/// Bounded lobby failures, without copying peer-supplied diagnostics.
#[derive(Debug, thiserror::Error)]
pub enum LobbyError {
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
    ended: bool,
    active: [bool; MAX_RIDERS],
}
impl Events {
    fn control(&mut self, event: LobbyEvent) -> LobbyEvent {
        match &event {
            LobbyEvent::Joined { id, .. } => self.active[usize::from(id.value())] = true,
            LobbyEvent::Left { id } => {
                self.active[usize::from(id.value())] = false;
                self.pending[usize::from(id.value())] = None;
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
                if let Some(state) = state
                    .take()
                    .filter(|_| id != self.me && self.active[usize::from(id.value())])
                {
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
) -> Events {
    Events {
        controls,
        states,
        clock,
        pending: [None; MAX_RIDERS],
        me,
        ended: false,
        active: [false; MAX_RIDERS],
    }
}

enum Command {
    Admit(Reply, oneshot::Sender<Result<(), LobbyError>>),
    Start(Duration, oneshot::Sender<Result<Instant, LobbyError>>),
    Select(SharedActivity, oneshot::Sender<Result<(), LobbyError>>),
    Ready(u64, bool, oneshot::Sender<Result<(), LobbyError>>),
    PeerReady(Membership, u64, bool),
    Enter(Membership, u64),
    WorkoutPause(bool, oneshot::Sender<Result<(), LobbyError>>),
    WorkoutSkip(Duration, oneshot::Sender<Result<(), LobbyError>>),
    Remove(RiderId),
    AdmitAs(Reply, RiderId, oneshot::Sender<Result<(), LobbyError>>),
    Handover(Option<RiderId>, crate::handover::Message),
    PeerHandover(Membership, crate::handover::Message),
}

/// The host's lobby owner. Dropping it aborts its actor, peer tasks and transport listeners.
pub struct HostLobby {
    organizer: RiderId,
    root: RideId,
    epoch: u64,
    invite: Invite,
    commands: mpsc::Sender<Command>,
    state: watch::Sender<Option<RiderState>>,
    progress: watch::Sender<Option<(RideProgress, Instant)>>,
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
        let (voice, owner, _changes) = voice::channel_for(organizer, organizer);
        if prepared.is_some() {
            owner.0.suspend();
        }
        let mut tasks = JoinSet::new();
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
            clock,
            organizer,
            prepared,
            (root, epoch),
        ));
        Ok(Self {
            organizer,
            root,
            epoch,
            invite,
            commands,
            state,
            progress,
            events: events(organizer, controls, updates, clock_updates),
            tasks,
            voice: Some(voice),
        })
    }
    /// The organizer's stable rider identity, independent of the hosting role.
    #[must_use]
    pub const fn organizer(&self) -> RiderId {
        self.organizer
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
    me: RiderId,
    organizer: RiderId,
    root: RideId,
    epoch: u64,
    state: watch::Sender<Option<RiderState>>,
    events: Events,
    tasks: JoinSet<()>,
    voice: Option<VoicePort>,
    connection: Connection,
    readiness: mpsc::Sender<Message>,
    app_lobby: bool,
}
impl JoinerLobby {
    /// Connects, describes the rider and validates the welcome within the original deadline.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for invalid metadata, transport, timeout or protocol failures.
    pub async fn join(joiner: Joiner, rider: Rider) -> Result<Self, LobbyError> {
        Self::join_inner(joiner, rider, None).await
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
        Self::join_inner(joiner, rider, Some((id, plan, roster))).await
    }
    async fn join_inner(
        joiner: Joiner,
        rider: Rider,
        prepared: Option<(RiderId, crate::handover::Plan, Roster)>,
    ) -> Result<Self, LobbyError> {
        rider.validate()?;
        let route = joiner.reply().route;
        let connection = joiner.connect().await?;
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
        let content_connection = connection.clone();
        tasks.spawn(async move {
            let _ = client_actor(
                connection,
                me,
                roster,
                send,
                recv,
                control,
                states,
                local,
                owner,
                changes,
                ready,
                route == RouteHash::LOBBY,
                clock,
                organizer,
                prepared,
            )
            .await;
        });
        Ok(Self {
            me,
            organizer,
            root,
            epoch,
            state,
            events: events(me, controls, updates, clock_updates),
            tasks,
            voice: Some(voice),
            connection: content_connection,
            readiness,
            app_lobby: route == RouteHash::LOBBY,
        })
    }
    /// This connection's host-assigned number.
    #[must_use]
    pub const fn me(&self) -> RiderId {
        self.me
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

    /// An authenticated content link for one locally owned download task in app-lobby mode.
    #[must_use]
    pub fn content_link(&self) -> Option<Connection> {
        self.app_lobby.then(|| self.connection.clone())
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
        self.connection.close();
    }
}

struct Mailbox {
    inbox: StateInbox,
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
    Hello(Result<(Connection, SendStream, RecvStream, Rider), LobbyError>),
    Left(Membership),
}

fn event(control: &mpsc::Sender<LobbyEvent>, event: LobbyEvent) -> Result<(), LobbyError> {
    control.try_send(event).map_err(|_| LobbyError::Lost)
}
fn broadcast(peers: &[Option<Peer>; MAX_RIDERS], message: &Message) {
    for peer in peers.iter().flatten() {
        if peer
            .control
            .try_send(Control::Message(message.clone()))
            .is_err()
        {
            peer.connection.close();
        }
    }
}

async fn hello(
    connection: Connection,
    route: RouteHash,
) -> Result<(Connection, SendStream, RecvStream, Rider), LobbyError> {
    let (send, recv, rider) = timeout_at(connection.admission_deadline(), async {
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
    .map_err(|_| LinkError::Timeout)??;
    Ok((connection, send, recv, rider))
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
    clock: watch::Sender<Option<WorkoutClock>>,
    organizer: RiderId,
    prepared: Option<Prepared>,
    authority: (RideId, u64),
) {
    let _ = host_loop(
        &mut host,
        me,
        &mut commands,
        &controls,
        &states,
        &mut local,
        &voice.0,
        &command_sender,
        &progress,
        &clock,
        organizer,
        prepared,
        authority,
    )
    .await;
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
    clock: &watch::Sender<Option<WorkoutClock>>,
    organizer: RiderId,
    mut prepared: Option<Prepared>,
    authority: (RideId, u64),
) -> Result<(), LobbyError> {
    let shared = Arc::new(Mutex::new(Mailbox {
        inbox: StateInbox::new(Instant::now().into_std()),
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
    let mut ticker = interval(STATE_EVERY);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(Command::Admit(reply, result)) => {
                    let answer = if prepared.as_ref().is_some_and(|prepared| !prepared.active) { Err(LobbyError::Protocol) } else if !activities.enabled && lock(&shared).start.is_some() { Err(LobbyError::Started) } else { host.admit(&reply).map_err(Into::into) };
                    let _ = result.send(answer);
                }
                Some(Command::Start(countdown, result)) => {
                    if lock(&shared).start.is_some() { let _ = result.send(Err(LobbyError::Started)); continue; }
                    if let Err(error) = activities.start() { let _ = result.send(Err(error)); continue; }
                    let at = Instant::now() + countdown;
                    lock(&shared).start = Some(at);
                    if let Some(selected) = activities.selected.borrow().as_ref().filter(|selected| selected.selection.workout.is_some()) {
                        timeline = Some(Timeline::new(selected.selection.revision, at.into_std()));
                    }
                    if !activities.enabled { host.seal(); }
                    lock(&shared).entered = std::array::from_fn(|id| roster[id].is_some());
                    for peer in peers.iter().flatten() {
                        if peer.control.try_send(Control::Start(at)).is_err() { peer.connection.close(); }
                    }
                    event(controls, LobbyEvent::Start { at })?;
                    let _ = result.send(Ok(at));
                }
                Some(Command::Select(selected,result)) => {
                    let answer = activities.select(selected.clone());
                    if answer.is_ok() {
                        broadcast(&peers,&Message::Selection(selected.selection.clone()));
                        event(controls,LobbyEvent::Selection(selected.selection))?;
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
                                let revision = prepared.replacement.plan.context.revision;
                                for (slot, rider) in roster.iter().enumerate() {if rider.is_some() {activities.ready(rider_id(slot), revision, true)?;}}
                                {let mut mailbox = lock(&shared); mailbox.active = true; mailbox.entered = std::array::from_fn(|id| roster[id].is_some());}
                                drop(local.borrow_and_update());
                                voice.activate();
                                broadcast(&peers, &Message::Handover(message.clone()));
                                event(controls, LobbyEvent::Handover {sender: organizer, message})?;
                            }
                            _ => if let Some(peer) = &peers[slot] {peer.connection.close();},
                        }
                    } else {event(controls, LobbyEvent::Handover {sender: member.rider(), message})?;}
                }
                Some(Command::Remove(id)) => if let Some(peer) = &peers[usize::from(id.value())] {peer.connection.close();},
                None => return Ok(()),
            },
            incoming = host.next() => match incoming {
                Some(HostEvent::Joined(connection)) if (activities.enabled || lock(&shared).start.is_none()) && tasks.len() < 16 => {
                    let route = host.invite().route;
                    tasks.spawn(async move { Done::Hello(hello(connection, route).await) });
                }
                Some(HostEvent::Joined(connection)) => connection.close(),
                Some(HostEvent::Expired) => event(controls, LobbyEvent::Expired)?,
                None => return Err(LobbyError::Lost),
            },
            done = tasks.join_next(), if !tasks.is_empty() => match done {
                Some(Ok(Done::Hello(Ok((connection, send, recv, rider))))) if activities.enabled || lock(&shared).start.is_none() => {
                    let id = if let Some(prepared) = prepared.as_mut().filter(|prepared| !prepared.active) {
                        if let Ok(id) = prepared.assign(&connection, &rider, &roster) {id} else {connection.close(); continue;}
                    } else {
                        let Some(slot) = roster.iter().position(Option::is_none) else {connection.close(); continue;}; rider_id(slot)
                    };
                    let slot = usize::from(id.value());
                    activities.joined(id)?;
                    let member = lock(&shared).inbox.join(id, Instant::now().into_std())?;
                    voice.join(member,Some(connection.clone()));
                    broadcast(&peers, &Message::Joined(id, rider.clone()));
                    broadcast(&peers, &Message::VoiceRoster(voice.epochs()));
                    event(controls, LobbyEvent::Joined { id, rider: rider.clone() })?;
                    roster[slot] = Some(rider);
                    let (control, receive) = mpsc::channel(16);
                    peers[slot] = Some(Peer { connection: connection.clone(), member, control });
                    let shared = shared.clone(); let welcome = Message::Welcome(id, organizer, authority.0, authority.1, roster.clone()); let updates = states.subscribe();
                    let voice = voice.clone();
                    let epochs = voice.epochs();
                    let mut initial = activities.initial();
                    if let Some(at) = lock(&shared).start {
                        initial.push(running(at)?);
                        if at <= Instant::now() && let Some(timeline) = &mut timeline {initial.push(Message::Workout(timeline.sample(at.into_std())?));}
                    }
                    lock(&shared).entered[slot] = false;
                    let initial = initial; let selection = activities.selected.subscribe(); let commands = command_sender.clone();
                    tasks.spawn(async move { let _ = serve_peer(&connection, send, recv, welcome, member, shared, receive, updates, voice, epochs,initial,selection,commands).await; connection.close(); Done::Left(member) });
                }
                Some(Ok(Done::Left(member))) => {
                    let slot = usize::from(member.rider().value());
                    if peers[slot].as_ref().is_some_and(|p| p.member == member) {
                        voice.leave(member);
                        activities.left(member.rider());
                        lock(&shared).entered[slot] = false;
                        peers[slot] = None; roster[slot] = None; lock(&shared).inbox.remove(member);
                        broadcast(&peers, &Message::Left(member.rider()));
                        broadcast(&peers, &Message::VoiceRoster(voice.epochs()));
                        event(controls, LobbyEvent::Left { id: member.rider() })?;
                    }
                }
                _ => {}
            },
            _ = ticker.tick() => {
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

#[allow(clippy::too_many_arguments)] // one bounded peer worker owns both stream halves and mailboxes
async fn serve_peer(
    connection: &Connection,
    mut send: SendStream,
    mut recv: RecvStream,
    welcome: Message,
    member: Membership,
    shared: Shared,
    mut controls: mpsc::Receiver<Control>,
    mut states: watch::Receiver<States>,
    voice: VoiceHandle,
    epochs: [u64; MAX_RIDERS],
    initial: Vec<Message>,
    selected: watch::Receiver<Option<SharedActivity>>,
    commands: mpsc::Sender<Command>,
) -> Result<(), LobbyError> {
    timeout_at(
        connection.admission_deadline(),
        wire::write(&mut send, &welcome),
    )
    .await
    .map_err(|_| LinkError::Timeout)??;
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
            .map_err(|_| LobbyError::Lost)??;
    }
    let mut readers = JoinSet::new();
    let mut transfers = JoinSet::new();
    let mut transfer_budget = voice::IncomingBudget::controls();
    let receiver_voice = voice.clone();
    let receiver_shared = shared.clone();
    readers.spawn(async move {
        let mut budget = voice::IncomingBudget::controls();
        loop {
            let message = wire::read(&mut recv).await?;
            budget.charge().map_err(|_| LobbyError::Protocol)?;
            match message {
                Message::VoiceEnabled(enabled) => {
                    if enabled && !lock(&receiver_shared).active {
                        return Err(LobbyError::Protocol);
                    }
                    receiver_voice
                        .consent(member, enabled)
                        .map_err(|_| LobbyError::Protocol)?;
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
                timeout(WRITE_WAIT, wire::write(&mut send, &message)).await.map_err(|_| LobbyError::Lost)??;
            },
            changed = states.changed() => {
                changed.map_err(|_| LobbyError::Lost)?;
                let snapshot = *states.borrow_and_update();
                for (id, state) in snapshot.iter().enumerate().filter_map(|(id,state)| state.map(|state| (rider_id(id), state))) {
                    if id != member.rider() { connection.send_datagram(&state.datagram(id)?)?; }
                }
            },
            bytes = connection.read_datagram() => {
                let bytes = bytes?;
                if !lock(&shared).active {return Err(LobbyError::Protocol);}
                if bytes.first() == Some(&voice::TAG) {
                    voice.ingest(member,&bytes).map_err(|_| LobbyError::Protocol)?;
                    continue;
                }
                let mut mailbox = lock(&shared);
                let elapsed = peer_elapsed(mailbox.start).ok_or(LobbyError::Protocol)?;
                if !mailbox.entered[usize::from(member.rider().value())] { return Err(LobbyError::Protocol); }
                mailbox.inbox.ingest(member, &bytes, elapsed, Instant::now().into_std())?;
            },
            _ = readers.join_next() => return Err(LobbyError::Protocol),
            stream = connection.accept_bi() => {
                transfer_budget.charge().map_err(|_|LobbyError::Protocol)?;
                let (send,recv) = stream?;
                if transfers.try_join_next().is_none() && !transfers.is_empty() {return Err(LobbyError::Protocol);}
                let mut changes = selected.clone();
                let selection = changes.borrow_and_update().clone().ok_or(LobbyError::Protocol)?;
                transfers.spawn(async move {tokio::select! {result = activity::transfer(send,recv,selection) => result,_ = changes.changed() => Err(crate::content::ContentError::Interrupted)}});
            },
            _ = transfers.join_next(), if !transfers.is_empty() => {},
        }
    }
}

#[allow(clippy::too_many_arguments)] // the actor owns the one connection and its bounded channels
#[allow(clippy::too_many_lines)] // ordered reliable controls and datagrams share one connection owner
async fn client_actor(
    connection: Connection,
    me: RiderId,
    mut roster: Roster,
    mut send: SendStream,
    mut recv: RecvStream,
    controls: mpsc::Sender<LobbyEvent>,
    states: watch::Sender<States>,
    mut local: watch::Receiver<Option<RiderState>>,
    voice: VoiceOwner,
    mut voice_changes: watch::Receiver<bool>,
    mut readiness: mpsc::Receiver<Message>,
    app_lobby: bool,
    clock: watch::Sender<Option<WorkoutClock>>,
    organizer: RiderId,
    mut prepared: Option<crate::handover::Plan>,
) -> Result<(), LobbyError> {
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
        if rider.is_some() && rider_id(id) != me {
            members[id] = Some(inbox.join(rider_id(id), Instant::now().into_std())?);
            voice.0.join(members[id].ok_or(LobbyError::Protocol)?, None);
        }
    }
    let mut start = None;
    let mut selection_revision = 0;
    let mut workout_sequence = 0;
    let mut late = false;
    let mut entered = false;
    let mut requested = None;
    let mut count = 0_u8;
    let mut control_period = Instant::now();
    let mut ticker = interval(STATE_EVERY);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // A host may forward eight independent 5 Hz streams. Charge even unknown/stale packets.
    let mut packets = 80_u8;
    let mut voice_packets = voice::IncomingBudget::new();
    let mut voice_control_open = true;
    let mut refill = Instant::now();
    loop {
        tokio::select! {
            Some(message) = readiness.recv() => {
                if let Message::Enter(revision) = message {
                    if !late || entered || requested.is_some() || revision != selection_revision { continue; }
                    requested = Some(revision);
                }
                timeout(WRITE_WAIT,wire::write(&mut send,&message)).await.map_err(|_| LobbyError::Lost)??;
            },
            changed = voice_changes.changed(), if voice_control_open => {
                let enabled = if changed.is_ok() { *voice_changes.borrow_and_update() } else { voice_control_open = false; false };
                timeout(WRITE_WAIT,wire::write(&mut send,&Message::VoiceEnabled(enabled))).await.map_err(|_| LobbyError::Lost)??;
            },
            message = incoming.recv() => {
                if app_lobby && control_period.elapsed()>=Duration::from_secs(1) {count=0;control_period=Instant::now();}
                count = count.checked_add(1).filter(|n| *n <= 64).ok_or(LobbyError::Protocol)?;
                match message.ok_or(LobbyError::Lost)? {
                    Message::Selection(selection) if app_lobby && start.is_none() && selection.revision>selection_revision => {
                        selection_revision = selection.revision; event(&controls,LobbyEvent::Selection(selection))?;
                    }
                    Message::Readiness(id,revision,ready) if app_lobby && revision==selection_revision && roster[usize::from(id.value())].is_some() => {
                        event(&controls,LobbyEvent::Readiness {id,revision,ready})?;
                    }
                    Message::VoiceRoster(epochs) => voice.0.install_epochs(epochs).map_err(|_| LobbyError::Protocol)?,
                    Message::Joined(id, rider) if (app_lobby || start.is_none()) && roster[usize::from(id.value())].is_none() && id != me => {
                        let index = usize::from(id.value()); members[index] = Some(inbox.join(id, Instant::now().into_std())?);
                        voice.0.join(members[index].ok_or(LobbyError::Protocol)?,None);
                        roster[index] = Some(rider.clone()); event(&controls, LobbyEvent::Joined { id, rider })?;
                    }
                    Message::Left(id) if id != organizer && id != me => {
                        let index = usize::from(id.value()); let member = members[index].take().ok_or(LobbyError::Protocol)?;
                        voice.0.leave(member);
                        inbox.remove(member); roster[index] = None; event(&controls, LobbyEvent::Left { id })?;
                    }
                    Message::Start(millis) if start.is_none() => {
                        let at = Instant::now() + Duration::from_millis(u64::from(millis)).saturating_sub(connection.rtt()/2);
                        entered = true; start = Some(at); event(&controls, LobbyEvent::Start { at })?;
                    }
                    Message::Handover(message) if app_lobby => {
                        if let crate::handover::Message::Commit {epoch, ref proof} = message && let Some(plan) = &prepared {
                            let members = roster.iter().enumerate().fold(0_u8, |mask, (id, rider)| if rider.is_some() {mask | (1 << id)} else {mask});
                            if epoch != plan.context.epoch || !plan.accepts(proof) || members != plan.context.members {return Err(LobbyError::Protocol);}
                            drop(local.borrow_and_update());
                            entered = true; voice.0.activate(); prepared = None;
                        }
                        event(&controls, LobbyEvent::Handover {sender: organizer, message})?;
                    }
                    Message::Workout(_) if prepared.is_some() => {},
                    Message::Workout(sample) if app_lobby && sample.revision == selection_revision && sample.sequence > workout_sequence => {
                        let start = start.ok_or(LobbyError::Protocol)?;
                        let current = sample.clock(start.into_std())?;
                        workout_sequence = sample.sequence;
                        clock.send_replace(Some(current));
                    }
                    Message::Running(age) if app_lobby && start.is_none() && selection_revision > 0 => {
                        let now = Instant::now();
                        let at = if age >= 0 {now.checked_sub(Duration::from_millis(age.unsigned_abs()) + connection.rtt()/2)}
                            else {now.checked_add(Duration::from_millis(age.unsigned_abs()).saturating_sub(connection.rtt()/2))}.ok_or(LobbyError::Protocol)?;
                        start = Some(at); late = true; event(&controls, LobbyEvent::Running {at})?;
                    }
                    Message::Entry(revision, progress) if app_lobby && late && !entered && requested == Some(revision) => {
                        requested = None; entered = progress.is_some();
                        event(&controls, LobbyEvent::Entry {revision, progress})?;
                    }
                    _ => return Err(LobbyError::Protocol),
                }
            },
            bytes = connection.read_datagram() => {
                let bytes = bytes?;
                if bytes.first() == Some(&voice::TAG) {
                    voice_packets.charge().map_err(|_| LobbyError::Protocol)?;
                    if prepared.is_some() {continue;}
                    let id = RiderId::new(*bytes.get(2).ok_or(LobbyError::Protocol)?)?;
                    if id == me { return Err(LobbyError::Protocol); }
                    if let Some(member) = members[usize::from(id.value())] {
                        voice.0.ingest(member,&bytes).map_err(|_| LobbyError::Protocol)?;
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
                if packets == 0 { return Err(StateError::Flood.into()); } packets -= 1;
                if prepared.is_some() {continue;}
                let id = RiderId::new(*bytes.get(1).ok_or(LobbyError::Protocol)?)?;
                let Some(elapsed) = peer_elapsed(start) else {if app_lobby {continue;} return Err(LobbyError::Protocol);};
                if id == me { return Err(LobbyError::Protocol); }
                // A datagram sent before a reliable departure may arrive after it.
                if let Some(member) = members[usize::from(id.value())] { inbox.ingest(member, &bytes, elapsed, Instant::now().into_std())?; }
            },
            _ = ticker.tick() => if let Some(elapsed) = elapsed(start) {
                if entered && local.has_changed().unwrap_or(false)
                    && let Some(state) = *local.borrow_and_update() {
                        let bytes = state.datagram(me)?;
                        RiderState::from_datagram(&bytes, me, elapsed)?;
                        connection.send_datagram(&bytes)?;
                }
                states.send_replace(inbox.drain(Instant::now().into_std()));
            },
            _ = readers.join_next() => return Err(LobbyError::Lost),
            _ = connection.accept_bi() => return Err(LobbyError::Protocol),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::GatherOptions;
    use crate::link::ConnectOptions;
    use crate::policy::AddressPolicy;
    use orqwa_domain::units::{Meters, Watts};

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
        host.ready(1, true).await.unwrap();
        assert!(host.start(Duration::from_secs(1)).await.is_err());
        client.ready(1, true).unwrap();
        timeout(Duration::from_secs(2),async {loop {if matches!(host.next().await,Some(LobbyEvent::Readiness {id,ready:true,..}) if id==client.me()) {break;}}}).await.unwrap();
        host.start(Duration::from_secs(1)).await.unwrap();
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
        drop(client);
        drop(host);
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
        }
    }
    fn rider(name: &str) -> Rider {
        Rider {
            name: name.into(),
            mass: Kilograms(83.0),
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
        cara.connection.send_datagram(&early(cara.me())).unwrap();
        let LobbyEvent::Left { id } =
            host_event(&mut host, |e| matches!(e, LobbyEvent::Left { .. })).await
        else {
            panic!("left");
        };
        assert_eq!(id, cara.me());
        host.start(Duration::from_secs(1)).await.unwrap();
        client_event(&mut ben, |e| matches!(e, LobbyEvent::Start { .. })).await;
        // Anna's start estimate runs ahead of the host's: her first state arrives early.
        anna.connection.send_datagram(&early(anna.me())).unwrap();
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

    #[tokio::test]
    async fn a_joiner_cannot_send_organizer_workout_time_or_stop_the_lobby() {
        let mut host = HostLobby::open(
            Host::lobby("Host", &options()).await.unwrap(),
            rider("Host"),
        )
        .unwrap();
        let (_connection, mut send, _recv, id) = raw_join(&host).await;
        wire::write(
            &mut send,
            &Message::Workout(timing::WorkoutSample {
                revision: 1,
                sequence: 1,
                group: Duration::ZERO,
                elapsed: Duration::from_mins(15),
                paused: true,
            }),
        )
        .await
        .unwrap();
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
            for _ in 0..if attack == 3 { 20 } else { 1 } {
                let _ = connection.send_datagram(&bytes);
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
        let mut events = events(RiderId::HOST, receive, updates, clock_updates);
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
            let (connection, mut send, _recv, id) = raw_join(&host).await;
            if attack == 2 {
                for _ in 0..12 {
                    let _ = wire::write(&mut send, &Message::VoiceEnabled(true)).await;
                }
            } else {
                let source = if attack == 0 { RiderId::HOST } else { id };
                let frame = voice::VoiceFrame::new(source, 1, 0, &[1]).unwrap();
                let (bytes, len) = frame.datagram();
                for _ in 0..if attack == 1 { 20 } else { 1 } {
                    let _ = connection.send_datagram(&bytes[..len]);
                }
            }
            timeout(Duration::from_secs(1), connection.closed())
                .await
                .unwrap();
            host_event(&mut host, |event| matches!(event, LobbyEvent::Left { .. })).await;
            assert!(!host.tasks.is_empty());
            host.close().await;
        }
    }
}
