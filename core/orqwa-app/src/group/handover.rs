//! One bounded, consenting replacement operation; the old session remains authoritative.

use std::time::{Duration, Instant};

use orqwa_net::handover::{Context, Message, Plan, Proof, WorkoutPosition};
use orqwa_net::link::{ConnectOptions, Host, Joiner};
use orqwa_net::lobby::{HostLobby, JoinerLobby, LobbyEvent, Replacement, Rider, SharedActivity};
use orqwa_net::state::RiderId;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use super::runtime::Session;
use super::{Handover, Snapshot};

const CONSENT: Duration = Duration::from_secs(30);
const PREPARATION: Duration = Duration::from_secs(60);
const GRACE: Duration = Duration::from_secs(5);

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
        let epoch = self
            .last_epoch
            .max(snapshot.epoch)
            .checked_add(1)
            .ok_or("Authority epoch exhausted.")?;
        let context = Self::context(snapshot, epoch, successor)?;
        session.handover(None, Message::Offer { epoch, successor })?;
        self.begin(context, snapshot);
        Ok(())
    }

    fn begin(&mut self, context: Context, snapshot: &mut Snapshot) {
        self.last_epoch = context.epoch;
        snapshot.successor = Some(context.successor.value());
        snapshot.handover = Handover::Offered;
        self.context = Some(context);
        self.deadline = Some(Instant::now() + CONSENT);
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
        if context.successor.value() != snapshot.me
            || snapshot.handover != Handover::Offered
            || !snapshot.prepared
            || !started(snapshot)
        {
            return Err("Prepare and join the ride before accepting.".into());
        }
        session.handover(
            None,
            Message::Decision {
                epoch: context.epoch,
                accepted,
            },
        )?;
        if accepted {
            snapshot.handover = Handover::Preparing;
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
        if let Message::Offer { epoch, successor } = message {
            if snapshot.host
                || sender.value() != snapshot.organizer
                || self.context.is_some()
                || epoch <= self.last_epoch.max(snapshot.epoch)
                || !started(snapshot)
            {
                return Err("Unexpected organizer request.".into());
            }
            let context = Self::context(snapshot, epoch, successor)?;
            self.begin(context, snapshot);
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
                if snapshot.handover != Handover::Offered {
                    return Ok(());
                }
                if !accepted {
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
                self.cancel(session, snapshot)?;
            }
            Message::Failed { .. }
                if snapshot.host
                    && context.members & (1 << sender.value()) != 0
                    && !self.revealed =>
            {
                self.cancel(session, snapshot)?;
            }
            _ => return Err("Unexpected organizer transition message.".into()),
        }
        self.try_commit(snapshot)?;
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
        snapshot.handover = Handover::Preparing;
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
            self.jobs.spawn(async move {
                let joiner = Joiner::lobby(plan.invite.clone(), &options)
                    .await
                    .map_err(text)?;
                notices
                    .send((plan.context.epoch, joiner.reply().clone()))
                    .await
                    .map_err(text)?;
                JoinerLobby::prepare(joiner, rider, me, plan, roster, selection)
                    .await
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
            })) => {
                if epoch != context.epoch
                    || !self.plan.as_ref().is_some_and(|plan| plan.accepts(&proof))
                {
                    return Err("Invalid authority commit.".into());
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
                snapshot.workout_clock = plan
                    .clock(snapshot.start.ok_or("Missing group clock.")?)
                    .map_err(text)?;
                self.reset(snapshot);
                return Ok(Some(replacement));
            }
            Event::Link(Some(LobbyEvent::Left { .. } | LobbyEvent::HostLeft) | None) => {
                return Err("A replacement connection ended before handover completed.".into());
            }
            Event::Link(_) => {}
        }
        self.try_commit(snapshot)?;
        Ok(None)
    }

    fn try_commit(&mut self, snapshot: &mut Snapshot) -> Result<(), String> {
        let Some(context) = &self.context else {
            return Ok(());
        };
        if snapshot.host && !self.revealed && self.ready == context.members {
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
        if !snapshot.host && self.replacement.is_some() {
            self.departed |= 1 << id.value();
            let grace = Instant::now() + GRACE;
            self.deadline = Some(self.deadline.map_or(grace, |at| at.min(grace)));
            return true;
        }
        false
    }

    pub(super) fn expired(&self) -> bool {
        self.deadline.is_some_and(|at| at <= Instant::now())
    }
    pub(super) fn busy(&self) -> bool {
        self.context.is_some()
    }
    fn reset(&mut self, snapshot: &mut Snapshot) {
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
        snapshot.handover = Handover::Idle;
        snapshot.successor = None;
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
                        successor: RiderId::new(1).unwrap()
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
