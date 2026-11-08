//! One bounded, consenting replacement operation; the old session remains authoritative.
//! While riding, the organizer also keeps a consenting fallback on standby, prepared the same
//! way and committed by the fallback itself only if the organizer is lost (#426).

use std::time::{Duration, Instant};

use orqwa_net::code::Reachability;
use orqwa_net::handover::{Context, Message, Plan, Proof, WorkoutPosition};
use orqwa_net::link::{ConnectOptions, Host, Joiner};
use orqwa_net::lobby::{
    HostLobby, JoinerLobby, LobbyEvent, Replacement, Rider, SharedActivity, WorkoutClock,
};
use orqwa_net::state::RiderId;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use super::runtime::Session;
use super::{Handover, Snapshot};

const CONSENT: Duration = Duration::from_secs(30);
const PREPARATION: Duration = Duration::from_secs(60);
const GRACE: Duration = Duration::from_secs(5);
/// How long the organizer must stay lost after a rider's link to it timed out before that
/// rider counts it as gone and the fallback takes over (#426).
#[cfg(not(test))]
const TAKEOVER: Duration = Duration::from_secs(30);
#[cfg(test)]
const TAKEOVER: Duration = Duration::from_secs(1);
/// How long riders wait for the fallback to take over once the organizer has left for good.
const STANDBY_GRACE: Duration = Duration::from_secs(20);
/// The pause before offering a standby again after one failed, so a fallback some rider cannot
/// reach does not keep everyone preparing links.
const STANDBY_RETRY: Duration = Duration::from_secs(30);

enum Job {
    Listener(Host),
    Peer(Box<JoinerLobby>),
}
pub(super) enum Event {
    Job(Result<(), String>),
    Reply(orqwa_net::code::Reply),
    Link(Option<LobbyEvent>),
}

#[allow(clippy::struct_excessive_bools)] // independent deadline, proof, connection and announcement gates
pub(super) struct Coordinator {
    options: ConnectOptions,
    rider: Rider,
    last_epoch: u64,
    context: Option<Context>,
    plan: Option<Plan>,
    proof: Option<Proof>,
    listener: Option<Host>,
    replacement: Option<Session>,
    jobs: JoinSet<Result<Job, String>>,
    notices: mpsc::Sender<(u64, orqwa_net::code::Reply)>,
    receive: mpsc::Receiver<(u64, orqwa_net::code::Reply)>,
    deadline: Option<Instant>,
    preparing: bool,
    ready: u8,
    connected: u8,
    departed: u8,
    announced: bool,
    revealed: bool,
    pub(super) old_lost: bool,
    /// The current operation is a standby, kept for the ride rather than committed (#426).
    standby: bool,
    /// Every standby link is ready: the fallback stands by.
    standing: bool,
    /// At the organizer: riders who declined standing in, or whose standby failed, by slot.
    declined: u8,
    /// This rider agreed to stand in during this group, so a renewed standby asks no more.
    consented: bool,
    /// Since when this rider has lost its organizer: its link reconnecting (#434), or, at the
    /// organizer, every rider away.
    lost_since: Option<Instant>,
    /// This rider told the fallback it lost the organizer.
    reported: bool,
    /// At the fallback: riders who reported losing the organizer, by slot.
    lost: u8,
    /// Riders whose standby link is away, as the replacement session told, by slot.
    away: u8,
    /// The last shared workout clock heard, for the fallback to continue from.
    clock: Option<WorkoutClock>,
    /// No standby is offered before then, after one failed.
    retry_at: Option<Instant>,
}

impl Coordinator {
    pub(super) fn new(options: ConnectOptions, rider: Rider) -> Self {
        let (notices, receive) = mpsc::channel(1);
        Self {
            options,
            rider,
            last_epoch: 1,
            context: None,
            plan: None,
            proof: None,
            listener: None,
            replacement: None,
            jobs: JoinSet::new(),
            notices,
            receive,
            deadline: None,
            preparing: false,
            ready: 0,
            connected: 0,
            departed: 0,
            announced: false,
            revealed: false,
            old_lost: false,
            standby: false,
            standing: false,
            declined: 0,
            consented: false,
            lost_since: None,
            reported: false,
            lost: 0,
            away: 0,
            clock: None,
            retry_at: None,
        }
    }

    fn context(snapshot: &Snapshot, epoch: u64, successor: RiderId) -> Result<Context, String> {
        Context::new(
            snapshot.root.ok_or("The group identity is unavailable.")?,
            epoch,
            RiderId::new(snapshot.organizer).map_err(text)?,
            successor,
            &snapshot.roster,
            snapshot
                .selection
                .as_ref()
                .ok_or("Choose an activity first.")?,
        )
        .map_err(text)
    }

    pub(super) fn propose(
        &mut self,
        session: &Session,
        snapshot: &mut Snapshot,
        successor: RiderId,
    ) -> Result<(), String> {
        if !snapshot.can_handover()
            || successor.value() == snapshot.me
            || !snapshot.ready[usize::from(successor.value())]
        {
            return Err("Choose a connected rider who has prepared the ride.".into());
        }
        // A planned change replaces the standby; the new organizer chooses a fresh one.
        if self.standby {
            self.cancel(session, snapshot)?;
        }
        self.offer(session, snapshot, successor, false)
    }

    fn offer(
        &mut self,
        session: &Session,
        snapshot: &mut Snapshot,
        successor: RiderId,
        standby: bool,
    ) -> Result<(), String> {
        let epoch = self
            .last_epoch
            .max(snapshot.epoch)
            .checked_add(1)
            .ok_or("Authority epoch exhausted.")?;
        let context = Self::context(snapshot, epoch, successor)?;
        session.handover(
            None,
            Message::Offer {
                epoch,
                successor,
                standby,
            },
        )?;
        self.begin(context, snapshot, standby);
        Ok(())
    }

    fn begin(&mut self, context: Context, snapshot: &mut Snapshot, standby: bool) {
        self.last_epoch = context.epoch;
        self.standby = standby;
        if !standby {
            snapshot.successor = Some(context.successor.value());
            snapshot.handover = Handover::Offered;
        }
        self.context = Some(context);
        self.deadline = Some(Instant::now() + CONSENT);
    }

    /// The organizer asks `successor` to stand in as fallback in place of the current one,
    /// also a rider who declined before (#426).
    pub(super) fn choose_fallback(
        &mut self,
        session: &Session,
        snapshot: &mut Snapshot,
        successor: RiderId,
    ) -> Result<(), String> {
        let id = usize::from(successor.value());
        if !snapshot.host
            || !started(snapshot)
            || snapshot.handover != Handover::Idle
            || successor.value() == snapshot.me
            || snapshot.roster[id].is_none()
            || !snapshot.ready[id]
            || snapshot.away[id]
        {
            return Err("Choose a connected rider who has prepared the ride.".into());
        }
        if self.context.is_some() {
            self.cancel(session, snapshot)?;
        }
        self.declined &= !(1 << id);
        self.offer(session, snapshot, successor, true)
    }

    /// Keeps a fallback on standby while riding (#426): the organizer offers it to the first
    /// prepared, connected rider in joining order who has not declined. Each rider's own app
    /// declines at once when its discovery suggests friends could not reach it.
    fn stand_by(&mut self, session: &Session, snapshot: &mut Snapshot) -> Result<(), String> {
        if !snapshot.host
            || self.context.is_some()
            || !started(snapshot)
            || snapshot.handover != Handover::Idle
            || snapshot.away.contains(&true)
            || snapshot.selection.is_none()
            || self.retry_at.is_some_and(|at| at > Instant::now())
            // Everyone prepares a standby link, so a friend still joining the ride is waited for.
            || snapshot
                .roster
                .iter()
                .enumerate()
                .any(|(id, rider)| rider.is_some() && !snapshot.ready[id])
        {
            return Ok(());
        }
        let candidate = (0..orqwa_net::state::MAX_RIDERS).find(|&id| {
            id != usize::from(snapshot.me)
                && snapshot.roster[id].is_some()
                && snapshot.ready[id]
                && self.declined & (1 << id) == 0
        });
        match candidate {
            Some(id) => self.offer(
                session,
                snapshot,
                RiderId::new(u8::try_from(id).map_err(text)?).map_err(text)?,
                true,
            ),
            None => Ok(()),
        }
    }

    /// Whether every friend in the group can probably reach this rider as organizer (R176):
    /// from other homes, or anyone when the group rides on one home network (R197).
    fn reachable(snapshot: &Snapshot) -> bool {
        matches!(
            snapshot.reachability,
            Some(Reachability::Direct | Reachability::Mapped | Reachability::Together)
        )
    }

    pub(super) fn answer(
        &mut self,
        session: &Session,
        snapshot: &mut Snapshot,
        accepted: bool,
    ) -> Result<(), String> {
        let context = self
            .context
            .as_ref()
            .ok_or("There is no organizer request to answer.")?;
        let offered = if self.standby {
            snapshot.standby_offer
        } else {
            snapshot.handover == Handover::Offered
        };
        if context.successor.value() != snapshot.me
            || !offered
            || !snapshot.prepared
            || !started(snapshot)
        {
            return Err("Prepare and join the ride before accepting.".into());
        }
        self.decide(session, snapshot, accepted)
    }

    fn decide(
        &mut self,
        session: &Session,
        snapshot: &mut Snapshot,
        accepted: bool,
    ) -> Result<(), String> {
        let context = self
            .context
            .as_ref()
            .ok_or("There is no organizer request to answer.")?;
        snapshot.standby_offer = false;
        if self.standby && accepted {
            self.consented = true;
        }
        session.handover(
            None,
            Message::Decision {
                epoch: context.epoch,
                accepted,
            },
        )?;
        if accepted {
            if !self.standby {
                snapshot.handover = Handover::Preparing;
            }
            let options = self.options.clone();
            let name = self.rider.name.clone();
            self.jobs.spawn(async move {
                Host::lobby(&name, &options)
                    .await
                    .map(Job::Listener)
                    .map_err(text)
            });
        } else {
            self.reset(snapshot);
        }
        Ok(())
    }

    // Only authenticated old-link frames reach this method. Reply credentials are forwarded
    // privately by the predecessor and are never broadcast to the group.
    #[allow(clippy::too_many_lines)]
    pub(super) async fn message(
        &mut self,
        session: &Session,
        snapshot: &mut Snapshot,
        sender: RiderId,
        message: Message,
    ) -> Result<(), String> {
        if let Message::Offer {
            epoch,
            successor,
            standby,
        } = message
        {
            if snapshot.host
                || sender.value() != snapshot.organizer
                || self.context.is_some()
                || epoch <= self.last_epoch.max(snapshot.epoch)
                || !started(snapshot)
            {
                return Err("Unexpected organizer request.".into());
            }
            let context = Self::context(snapshot, epoch, successor)?;
            self.begin(context, snapshot, standby);
            // Asked once to stand in (#426); a renewed standby, or one this rider could not
            // serve, is answered without a question.
            if standby && successor.value() == snapshot.me {
                if !Self::reachable(snapshot) {
                    self.decide(session, snapshot, false)?;
                } else if self.consented {
                    self.decide(session, snapshot, true)?;
                } else {
                    snapshot.standby_offer = true;
                }
            }
            return Ok(());
        }
        let Some(context) = self.context.clone() else {
            return Ok(());
        };
        if message.epoch() != context.epoch {
            return Ok(());
        }
        if !snapshot.host && sender != context.predecessor {
            return Err("An organizer message came from an unauthorized rider.".into());
        }
        match message {
            Message::Decision { accepted, .. } if snapshot.host && sender == context.successor => {
                if !self.standby && snapshot.handover != Handover::Offered {
                    return Ok(());
                }
                if !accepted {
                    if self.standby {
                        self.declined |= 1 << context.successor.value();
                    }
                    self.cancel(session, snapshot)?;
                    return Ok(());
                }
                let group_at = snapshot.start.ok_or("The ride has not started.")?.elapsed();
                session.handover(
                    None,
                    Message::Preparing {
                        epoch: context.epoch,
                        group_at,
                    },
                )?;
                self.prepare_deadline(snapshot, group_at)?;
            }
            Message::Preparing { group_at, .. } if !snapshot.host => {
                self.prepare_deadline(snapshot, group_at)?;
            }
            Message::Invite { invite, .. }
                if snapshot.host
                    && sender == context.successor
                    && self.preparing
                    && self.plan.is_none() =>
            {
                let group_at = snapshot.start.ok_or("The ride has not started.")?.elapsed();
                let workout = snapshot.workout_clock.map(|clock| WorkoutPosition {
                    elapsed: clock.position(Instant::now()),
                    paused: clock.paused,
                    sequence: clock.sequence,
                });
                let (plan, proof) =
                    Plan::prepare(context, invite, group_at, workout).map_err(text)?;
                plan.check(
                    &Self::context(snapshot, plan.context.epoch, plan.context.successor)?,
                    self.options.gather.policy,
                )
                .map_err(text)?;
                session.handover(None, Message::Plan(Box::new(plan.clone())))?;
                self.proof = Some(proof);
                self.install(snapshot, plan)?;
            }
            Message::Plan(plan) if !snapshot.host && self.preparing && self.plan.is_none() => {
                self.install(snapshot, *plan)?;
            }
            Message::Reply { rider, reply, .. } if self.plan.is_some() => {
                if snapshot.host && sender == rider && sender != context.successor {
                    session.handover(
                        Some(context.successor),
                        Message::Reply {
                            epoch: context.epoch,
                            rider,
                            reply,
                        },
                    )?;
                } else if snapshot.me == context.successor.value() && sender == context.predecessor
                {
                    let Some(Session::Host(host)) = &self.replacement else {
                        return Err("The replacement listener is unavailable.".into());
                    };
                    host.admit_as(&reply, rider).await.map_err(text)?;
                } else {
                    return Err("Unexpected replacement admission.".into());
                }
            }
            Message::Ready { rider, .. }
                if snapshot.host
                    && self.plan.is_some()
                    && sender == rider
                    && context.members & (1 << rider.value()) != 0 =>
            {
                self.ready |= 1 << rider.value();
            }
            Message::Cancel { .. } if !snapshot.host && !self.revealed => self.reset(snapshot),
            Message::Cancel { .. } | Message::Failed { .. }
                if snapshot.host && sender == context.successor && !self.revealed =>
            {
                // A fallback that cannot stand by is not asked again during this ride.
                if self.standby {
                    self.declined |= 1 << context.successor.value();
                }
                self.cancel(session, snapshot)?;
            }
            // The fallback alone holds the proof, to take over if the organizer is lost.
            Message::Proof { proof, .. }
                if self.standby
                    && snapshot.me == context.successor.value()
                    && sender == context.predecessor =>
            {
                if !self.plan.as_ref().is_some_and(|plan| plan.accepts(&proof)) {
                    return Err("Invalid fallback organizer proof.".into());
                }
                self.proof = Some(proof);
            }
            Message::Standby { .. } if self.standby && sender == context.predecessor => {
                self.standing = true;
                self.deadline = None;
                snapshot.fallback = Some(context.successor.value());
            }
            Message::Failed { .. }
                if snapshot.host
                    && context.members & (1 << sender.value()) != 0
                    && !self.revealed =>
            {
                if self.standby {
                    self.retry_at = Some(Instant::now() + STANDBY_RETRY);
                }
                self.cancel(session, snapshot)?;
            }
            _ => return Err("Unexpected organizer transition message.".into()),
        }
        self.try_commit(session, snapshot)?;
        Ok(())
    }

    fn prepare_deadline(
        &mut self,
        snapshot: &mut Snapshot,
        group_at: Duration,
    ) -> Result<(), String> {
        if self.preparing {
            return Ok(());
        }
        let start = snapshot.start.ok_or("The ride has not started.")?;
        if group_at > start.elapsed() + Duration::from_secs(2) {
            return Err("Invalid handover clock.".into());
        }
        self.deadline = Some(
            start
                .checked_add(group_at + PREPARATION)
                .ok_or("Invalid handover deadline.")?,
        );
        self.preparing = true;
        if !self.standby {
            snapshot.handover = Handover::Preparing;
        }
        Ok(())
    }

    fn install(&mut self, snapshot: &Snapshot, plan: Plan) -> Result<(), String> {
        let expected = self
            .context
            .as_ref()
            .ok_or("No organizer request is pending.")?;
        // Check each rider's own destination permissions before any discovery or probing.
        plan.check(expected, self.options.gather.policy)
            .map_err(text)?;
        if Self::context(snapshot, expected.epoch, expected.successor)? != *expected
            || self.deadline.is_none_or(|at| at <= Instant::now())
            || plan.group_at
                > snapshot.start.ok_or("Missing group clock.")?.elapsed() + Duration::from_secs(2)
        {
            return Err("The group changed during organizer preparation.".into());
        }
        if snapshot.me == expected.successor.value() {
            let host = self
                .listener
                .take()
                .ok_or("The replacement listener is unavailable.")?;
            self.replacement = Some(Session::Host(
                HostLobby::prepare(
                    host,
                    Replacement {
                        plan: plan.clone(),
                        roster: snapshot.roster.clone(),
                        start: snapshot.start.ok_or("Missing group clock.")?,
                        standby: self.standby,
                        activity: SharedActivity {
                            selection: snapshot.selection.clone().ok_or("Missing activity.")?,
                            course: snapshot.course.clone(),
                            workout: snapshot.workout.clone(),
                        },
                    },
                )
                .map_err(text)?,
            ));
            self.connected = 1 << snapshot.me;
        } else {
            let options = self.options.clone();
            let rider = self.rider.clone();
            let notices = self.notices.clone();
            let plan = plan.clone();
            let roster = snapshot.roster.clone();
            let selection = snapshot.selection.clone().ok_or("Missing activity.")?;
            let me = RiderId::new(snapshot.me).map_err(text)?;
            let standby = self.standby;
            self.jobs.spawn(async move {
                let joiner = Joiner::lobby(plan.invite.clone(), &options)
                    .await
                    .map_err(text)?;
                notices
                    .send((plan.context.epoch, joiner.reply().clone()))
                    .await
                    .map_err(text)?;
                if standby {
                    JoinerLobby::standby(joiner, rider, me, plan, roster, selection).await
                } else {
                    JoinerLobby::prepare(joiner, rider, me, plan, roster, selection).await
                }
                .map(|peer| Job::Peer(Box::new(peer)))
                .map_err(text)
            });
        }
        self.plan = Some(plan);
        Ok(())
    }

    pub(super) async fn next(&mut self) -> Event {
        tokio::select! {
            result = self.jobs.join_next(), if !self.jobs.is_empty() => {
                Event::Job(match result {
                    Some(Ok(Ok(Job::Listener(host)))) => {self.listener=Some(host);Ok(())},
                    Some(Ok(Ok(Job::Peer(peer)))) => {self.replacement=Some(Session::Join(*peer));Ok(())},
                    Some(Ok(Err(error))) => Err(error),
                    Some(Err(error)) => Err(text(error)),
                    None => Ok(()),
                })
            },
            Some((epoch, reply)) = self.receive.recv() => {
                if self.context.as_ref().is_some_and(|context|context.epoch==epoch) {Event::Reply(reply)} else {Event::Job(Ok(()))}
            },
            event = replacement_event(&mut self.replacement) => Event::Link(event),
        }
    }

    pub(super) fn event(
        &mut self,
        session: &Session,
        snapshot: &mut Snapshot,
        event: Event,
    ) -> Result<Option<Session>, String> {
        let Some(context) = self.context.clone() else {
            return Ok(None);
        };
        if let Event::Link(Some(link)) = &event
            && self.standby_link(snapshot, &context, link)
        {
            return Ok(None);
        }
        match event {
            Event::Job(result) => {
                result?;
                if let Some(host) = &self.listener {
                    session.handover(
                        None,
                        Message::Invite {
                            epoch: context.epoch,
                            invite: host.invite().clone(),
                        },
                    )?;
                }
                if matches!(self.replacement, Some(Session::Join(_))) {
                    if snapshot.host {
                        self.ready |= 1 << snapshot.me;
                    } else {
                        session.handover(
                            None,
                            Message::Ready {
                                epoch: context.epoch,
                                rider: RiderId::new(snapshot.me).map_err(text)?,
                            },
                        )?;
                    }
                }
            }
            Event::Reply(reply) => {
                let message = Message::Reply {
                    epoch: context.epoch,
                    rider: RiderId::new(snapshot.me).map_err(text)?,
                    reply,
                };
                session.handover(snapshot.host.then_some(context.successor), message)?;
            }
            Event::Link(Some(LobbyEvent::Joined { id, rider })) => {
                if snapshot.roster[usize::from(id.value())].as_ref() != Some(&rider) {
                    return Err("Replacement membership does not match the group.".into());
                }
                if snapshot.me != context.successor.value() {
                    return Ok(None);
                }
                self.connected |= 1 << id.value();
                if self.connected == context.members && !self.announced {
                    session.handover(
                        None,
                        Message::Ready {
                            epoch: context.epoch,
                            rider: context.successor,
                        },
                    )?;
                    self.announced = true;
                }
            }
            Event::Link(Some(LobbyEvent::Handover {
                message: Message::Commit { epoch, proof },
                ..
            })) => return self.commit(snapshot, &context, epoch, &proof),
            Event::Link(Some(LobbyEvent::Left { .. } | LobbyEvent::HostLeft) | None) => {
                return Err("A replacement connection ended before handover completed.".into());
            }
            Event::Link(_) => {}
        }
        self.try_commit(session, snapshot)?;
        Ok(None)
    }

    /// The prepared link's authority commit: everyone switches to the successor's session.
    fn commit(
        &mut self,
        snapshot: &mut Snapshot,
        context: &Context,
        epoch: u64,
        proof: &Proof,
    ) -> Result<Option<Session>, String> {
        if epoch != context.epoch || !self.plan.as_ref().is_some_and(|plan| plan.accepts(proof)) {
            return Err("Invalid authority commit.".into());
        }
        // A fallback takes over only from riders who lost the organizer themselves.
        if self.standby && snapshot.me != context.successor.value() && self.lost_since.is_none() {
            return Err(
                "The fallback organizer took over while the organizer was still connected.".into(),
            );
        }
        let replacement = self
            .replacement
            .take()
            .ok_or("Missing replacement connection.")?;
        let plan = self.plan.take().ok_or("Missing authority plan.")?;
        snapshot.organizer = context.successor.value();
        snapshot.epoch = context.epoch;
        snapshot.host = snapshot.me == snapshot.organizer;
        snapshot.invite = if snapshot.host {
            plan.invite
                .to_code(self.options.gather.policy)
                .map_err(text)?
        } else {
            String::new()
        };
        if snapshot.host {
            snapshot.reachability = Some(plan.invite.reachability());
        }
        snapshot.reply.clear();
        snapshot.states.fill(None);
        snapshot.latencies.fill(None);
        if self.standby {
            // Riding on after losing the organizer: the new one's clock arrives next.
            snapshot.workout_clock = None;
            snapshot.reconnecting = None;
            for id in 0..orqwa_net::state::MAX_RIDERS {
                snapshot.away[id] = self.away & (1 << id) != 0;
                // The group goes on without whoever left, the old organizer among them.
                if self.departed & (1 << id) != 0 {
                    snapshot.roster[id] = None;
                    snapshot.ready[id] = false;
                    snapshot.preparation[id] = None;
                }
            }
            // The new organizer chooses its own fallback (#426).
            self.declined = 0;
            self.lost_since = None;
        } else {
            snapshot.workout_clock = plan
                .clock(snapshot.start.ok_or("Missing group clock.")?)
                .map_err(text)?;
        }
        self.reset(snapshot);
        Ok(Some(replacement))
    }

    /// Follows who is connected to the fallback, and over standby links who among them lost the
    /// organizer too (#426). True if `event` needs nothing else.
    fn standby_link(&mut self, snapshot: &Snapshot, context: &Context, event: &LobbyEvent) -> bool {
        match event {
            LobbyEvent::Handover {
                sender,
                message: message @ (Message::Lost { .. } | Message::Found { .. }),
            } if self.standby && snapshot.me == context.successor.value() => {
                if message.epoch() == context.epoch {
                    if matches!(message, Message::Lost { .. }) {
                        self.lost |= 1 << sender.value();
                    } else {
                        self.lost &= !(1 << sender.value());
                    }
                }
                true
            }
            LobbyEvent::Away { id } => {
                self.away |= 1 << id.value();
                self.connected &= !(1 << id.value());
                true
            }
            LobbyEvent::Back { id } => {
                self.away &= !(1 << id.value());
                self.connected |= 1 << id.value();
                true
            }
            // A rider leaving the fallback's standby, the organizer leaving for good among them,
            // leaves it standing: the organizer renews it if it is still there.
            LobbyEvent::Left { id } if self.standby && self.standing => {
                self.away &= !(1 << id.value());
                self.connected &= !(1 << id.value());
                self.departed |= 1 << id.value();
                true
            }
            _ => false,
        }
    }

    fn try_commit(&mut self, session: &Session, snapshot: &mut Snapshot) -> Result<(), String> {
        let Some(context) = &self.context else {
            return Ok(());
        };
        if snapshot.host && !self.revealed && self.ready == context.members {
            // A standby keeps the proof with its fallback instead of committing (#426).
            if self.standby {
                if !self.standing {
                    let proof = self.proof.clone().ok_or("Missing authority proof.")?;
                    let (epoch, successor) = (context.epoch, context.successor);
                    session.handover(Some(successor), Message::Proof { epoch, proof })?;
                    session.handover(None, Message::Standby { epoch })?;
                    self.standing = true;
                    self.deadline = None;
                    snapshot.fallback = Some(context.successor.value());
                }
                return Ok(());
            }
            if self
                .deadline
                .is_none_or(|at| at.saturating_duration_since(Instant::now()) < GRACE)
            {
                return Err("Organizer preparation took too long.".into());
            }
            let Session::Join(peer) = self
                .replacement
                .as_ref()
                .ok_or("The organizer's replacement link is not ready.")?
            else {
                return Err("Invalid replacement role.".into());
            };
            peer.handover(Message::Commit {
                epoch: context.epoch,
                proof: self.proof.clone().ok_or("Missing authority proof.")?,
            })
            .map_err(text)?;
            self.revealed = true;
            snapshot.handover = Handover::Committing;
            self.deadline = Some(Instant::now() + GRACE);
        }
        Ok(())
    }

    pub(super) fn cancel(
        &mut self,
        session: &Session,
        snapshot: &mut Snapshot,
    ) -> Result<(), String> {
        if self.revealed || self.old_lost {
            return Err("The organizer is already changing.".into());
        }
        if let Some(context) = &self.context {
            session.handover(
                None,
                Message::Cancel {
                    epoch: context.epoch,
                },
            )?;
        }
        self.reset(snapshot);
        Ok(())
    }

    pub(super) fn fail(&mut self, session: &Session, snapshot: &mut Snapshot) -> bool {
        if self.revealed || self.old_lost {
            return true;
        }
        if self.standby {
            self.retry_at = Some(Instant::now() + STANDBY_RETRY);
        }
        if let Some(context) = &self.context {
            let message = if snapshot.host {
                Message::Cancel {
                    epoch: context.epoch,
                }
            } else {
                Message::Failed {
                    epoch: context.epoch,
                }
            };
            let _ = session.handover(None, message);
        }
        self.reset(snapshot);
        false
    }

    pub(super) fn lost(&mut self) -> bool {
        self.old_lost = true;
        if self.replacement.is_none() {
            return false;
        }
        // The organizer left for good: a standing fallback takes over without waiting (#426).
        if self.standby {
            if !self.standing {
                return false;
            }
            self.lost_since = Instant::now().checked_sub(TAKEOVER);
            self.deadline = Some(Instant::now() + STANDBY_GRACE);
            return true;
        }
        let grace = Instant::now() + GRACE;
        self.deadline = Some(self.deadline.map_or(grace, |at| at.min(grace)));
        true
    }

    /// An activated successor can close an old link before every peer receives its proof.
    /// The source cancels real precommit departures; prepared peers await that decision or
    /// the new proof, retaining frozen metadata until one is known.
    pub(super) fn departure(&mut self, snapshot: &Snapshot, id: RiderId) -> bool {
        if self.revealed {
            return true;
        }
        if !self.standby && !snapshot.host && self.replacement.is_some() {
            self.departed |= 1 << id.value();
            let grace = Instant::now() + GRACE;
            self.deadline = Some(self.deadline.map_or(grace, |at| at.min(grace)));
            return true;
        }
        false
    }

    /// A planned change ran out of time, or the fallback did not take over in time after the
    /// organizer left. An unanswered or unprepared standby is renewed quietly by [`Self::tick`].
    pub(super) fn expired(&self) -> bool {
        self.deadline.is_some_and(|at| at <= Instant::now()) && (!self.standby || self.old_lost)
    }
    /// A planned organizer change is in progress; a standby never holds anything up.
    pub(super) fn busy(&self) -> bool {
        self.context.is_some() && !self.standby
    }
    /// The organizer's riders changed: a standby binds them, so it is prepared again (#426).
    pub(super) fn membership(&mut self, session: &Session, snapshot: &mut Snapshot) {
        if self.standby && snapshot.host && !self.revealed && self.context.is_some() {
            let _ = self.cancel(session, snapshot);
        }
    }
    /// This rider's link to the organizer is reconnecting (#434). A prepared replacement link
    /// carries on as when the old organizer is lost; without one the planned change cannot
    /// finish and is given up. A standby stays: its fallback may take over (#426). True if
    /// group networking must end.
    pub(super) fn reconnecting(&mut self, session: &Session, snapshot: &mut Snapshot) -> bool {
        self.lost_since.get_or_insert_with(Instant::now);
        if !self.busy() {
            return false;
        }
        if self.replacement.is_some() {
            self.lost();
            return false;
        }
        self.fail(session, snapshot)
    }
    /// This rider's link to the organizer is back (#434).
    pub(super) fn reconnected(&mut self) {
        self.lost_since = None;
    }

    /// Keeps the standby going, once a tick (#426): the organizer offers it, or renews one that
    /// went unanswered or unprepared; riders tell the fallback whether they lost the organizer;
    /// and the fallback takes over once the organizer has been lost for [`TAKEOVER`] and no
    /// rider connected to it still reaches the organizer.
    ///
    /// # Errors
    /// A failure to reach the group or the standby, after which the standby is given up.
    pub(super) fn tick(
        &mut self,
        session: &Session,
        snapshot: &mut Snapshot,
    ) -> Result<(), String> {
        if let Some(clock) = snapshot.workout_clock {
            self.clock = Some(clock);
        }
        if snapshot.host && !self.old_lost {
            let others = snapshot
                .roster
                .iter()
                .enumerate()
                .filter(|(id, rider)| rider.is_some() && *id != usize::from(snapshot.me));
            let gone =
                others.clone().count() > 0 && others.clone().all(|(id, _)| snapshot.away[id]);
            if !gone {
                self.lost_since = None;
            } else if self.lost_since.is_none() {
                self.lost_since = Some(Instant::now());
            }
        }
        if self.standby
            && !self.standing
            && !self.revealed
            && self.deadline.is_some_and(|at| at <= Instant::now())
        {
            if let Some(context) = &self.context
                && snapshot.host
            {
                self.declined |= 1 << context.successor.value();
            }
            if snapshot.host {
                self.cancel(session, snapshot)?;
            } else {
                self.fail(session, snapshot);
            }
            return Ok(());
        }
        if !self.standby || !self.standing {
            return self.stand_by(session, snapshot);
        }
        let Some(context) = self.context.clone() else {
            return Ok(());
        };
        let lost = self.lost_since.is_some_and(|at| at.elapsed() >= TAKEOVER);
        if snapshot.me != context.successor.value() {
            if lost != self.reported
                && let Some(Session::Join(link)) = &self.replacement
            {
                let epoch = context.epoch;
                link.handover(if lost {
                    Message::Lost { epoch }
                } else {
                    Message::Found { epoch }
                })
                .map_err(text)?;
                self.reported = lost;
            }
            return Ok(());
        }
        let others = self.connected & !(1 << snapshot.me);
        if lost
            && !self.revealed
            && others & !self.lost == 0
            && let (Some(proof), Some(Session::Host(host))) = (&self.proof, &self.replacement)
        {
            host.take_over(context.epoch, proof.clone(), self.clock)
                .map_err(text)?;
            self.revealed = true;
        }
        Ok(())
    }

    pub(super) fn reset(&mut self, snapshot: &mut Snapshot) {
        if self
            .context
            .as_ref()
            .is_some_and(|context| snapshot.epoch != context.epoch)
        {
            for id in 0..orqwa_net::state::MAX_RIDERS {
                if self.departed & (1 << id) != 0 {
                    snapshot.roster[id] = None;
                    snapshot.ready[id] = false;
                    snapshot.states[id] = None;
                }
            }
        }
        self.jobs.abort_all();
        self.jobs = JoinSet::new();
        while self.receive.try_recv().is_ok() {}
        self.listener = None;
        self.replacement = None;
        self.context = None;
        self.plan = None;
        self.proof = None;
        self.deadline = None;
        self.ready = 0;
        self.connected = 0;
        self.departed = 0;
        self.announced = false;
        self.preparing = false;
        self.revealed = false;
        self.old_lost = false;
        self.standby = false;
        self.standing = false;
        self.reported = false;
        self.lost = 0;
        self.away = 0;
        snapshot.handover = Handover::Idle;
        snapshot.successor = None;
        snapshot.fallback = None;
        snapshot.standby_offer = false;
    }
}

async fn replacement_event(session: &mut Option<Session>) -> Option<LobbyEvent> {
    match session {
        Some(session) => session.next().await,
        None => std::future::pending().await,
    }
}
fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn started(snapshot: &Snapshot) -> bool {
    snapshot.prepared
        && snapshot.start.is_some_and(|at| at <= Instant::now())
        && (!snapshot.late || snapshot.entry.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use orqwa_domain::units::Kilograms;
    use orqwa_net::activity::{Assistance, ContentId, ContentKind, Selection};

    #[test]
    fn a_fallback_must_be_reachable_by_everyone_the_group_has() {
        let stands_in = |reach| {
            Coordinator::reachable(&Snapshot {
                reachability: Some(reach),
                ..Snapshot::default()
            })
        };
        assert!(stands_in(Reachability::Direct) && stands_in(Reachability::Mapped));
        // On one home network, a provider's IPv6 address or none at all changes nothing.
        assert!(stands_in(Reachability::Together));
        // With friends elsewhere, home addresses alone reach none of them.
        for reach in [
            Reachability::Home,
            Reachability::Ipv6,
            Reachability::Strict,
            Reachability::Unknown,
        ] {
            assert!(!stands_in(reach), "{reach:?}");
        }
    }

    #[tokio::test]
    async fn expiry_cancels_owned_preparation_without_restoring_stale_proposals() {
        let mut options = ConnectOptions::default();
        options.gather.policy.loopback = true;
        options.gather.stun_servers.clear();
        let rider = Rider {
            name: "Rider".into(),
            mass: Kilograms(80.0),
            weight: Kilograms(70.0),
            avatar: orqwa_domain::profile::Avatar::Female,
        };
        let host = Host::lobby(&rider.name, &options).await.unwrap();
        let root = host.invite().ride;
        let session = Session::Host(HostLobby::open(host, rider.clone()).unwrap());
        let mut snapshot = Snapshot {
            host: true,
            root: Some(root),
            phase: super::super::Phase::Riding,
            prepared: true,
            start: Instant::now().checked_sub(Duration::from_secs(1)),
            selection: Some(Selection {
                revision: 1,
                name: "Workout".into(),
                course: None,
                course_key: None,
                workout: Some(ContentId::of(ContentKind::Workout, b"workout").unwrap()),
                assistance: Assistance::default(),
            }),
            ..Snapshot::default()
        };
        snapshot.roster[0] = Some(rider.clone());
        snapshot.roster[1] = Some(rider.clone());
        snapshot.ready[0] = true;
        snapshot.ready[1] = true;
        let mut source = Coordinator::new(options.clone(), rider.clone());
        source
            .propose(&session, &mut snapshot, RiderId::new(1).unwrap())
            .unwrap();
        source.deadline = Some(Instant::now());
        assert!(source.expired());
        assert!(!source.fail(&session, &mut snapshot));
        assert!(snapshot.host);
        assert_eq!(snapshot.root, Some(root));
        assert_eq!(snapshot.epoch, 1);
        source
            .propose(&session, &mut snapshot, RiderId::new(1).unwrap())
            .unwrap();
        assert_eq!(
            source.context.as_ref().unwrap().epoch,
            3,
            "timeout consumes the previous epoch"
        );
        let mut candidate = Coordinator::new(options, rider);
        snapshot.host = false;
        snapshot.me = 1;
        snapshot.handover = Handover::Idle;
        candidate
            .message(
                &session,
                &mut snapshot,
                RiderId::HOST,
                Message::Offer {
                    epoch: 3,
                    successor: RiderId::new(1).unwrap(),
                    standby: false,
                },
            )
            .await
            .unwrap();
        candidate.answer(&session, &mut snapshot, true).unwrap();
        assert!(!candidate.jobs.is_empty());
        // A failed setup drops discovery and any listener without touching the ride.
        assert!(!candidate.fail(&session, &mut snapshot));
        assert!(candidate.jobs.is_empty());
        assert!(candidate.listener.is_none());
        assert!(snapshot.start.is_some());
        assert!(snapshot.prepared);
        assert!(
            candidate
                .message(
                    &session,
                    &mut snapshot,
                    RiderId::HOST,
                    Message::Offer {
                        epoch: 3,
                        successor: RiderId::new(1).unwrap(),
                        standby: false,
                    }
                )
                .await
                .is_err()
        );
        assert!(
            !candidate.lost(),
            "an unprepared request cannot become crash recovery"
        );
    }
}
