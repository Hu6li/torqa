//! Certificate-bound stable identities for one prepared successor listener.

use std::time::Instant;

use crate::code::{Fingerprint, Reply};
use crate::handover::{Context, Plan, Proof};
use crate::link::{Connection, Host};
use crate::state::{MAX_RIDERS, RiderId};

use super::{LobbyError, Rider, Roster, SharedActivity};

/// Sources and authority proof prepared for a consenting successor. Constructing this value
/// opens no sockets and does not activate a new organizer.
pub struct Replacement {
    /// Exact old authority commitment, checked against the current group before construction.
    pub plan: Plan,
    /// Frozen current roster; connection credentials retain these rider identities.
    pub roster: Roster,
    /// Already verified local sources for the frozen selection.
    pub activity: SharedActivity,
    /// Original shared start on this computer's monotonic clock.
    pub start: Instant,
    /// Kept for the ride as the organizer's fallback, which takes over itself only if the
    /// organizer is lost; until then it holds the seats of riders the network loses (#426).
    pub standby: bool,
}

pub(super) struct Prepared {
    pub(super) replacement: Replacement,
    pub(super) active: bool,
    credentials: [Option<Fingerprint>; MAX_RIDERS],
}
impl Prepared {
    pub(super) fn new(replacement: Replacement, host: &Host) -> Result<Self, LobbyError> {
        let plan = &replacement.plan;
        let context = Context::new(
            plan.context.root,
            plan.context.epoch,
            plan.context.predecessor,
            plan.context.successor,
            &replacement.roster,
            &replacement.activity.selection,
        )?;
        // The local listener was already opened under its own destination policy. The plan
        // must pin that exact listener; parsing a plan never widens network permission.
        if plan.context != context || host.invite() != &plan.invite {
            return Err(LobbyError::Protocol);
        }
        plan.clock(replacement.start)?;
        Ok(Self {
            replacement,
            active: false,
            credentials: std::array::from_fn(|_| None),
        })
    }
    pub(super) fn admit(
        &mut self,
        host: &mut Host,
        roster: &Roster,
        id: RiderId,
        reply: &Reply,
    ) -> Result<(), LobbyError> {
        let slot = usize::from(id.value());
        if self.active
            || id == self.replacement.plan.context.successor
            || self.replacement.roster[slot].is_none()
            || roster[slot].is_some()
            || self.credentials[slot].is_some()
        {
            return Err(LobbyError::Protocol);
        }
        host.admit(reply)?;
        self.credentials[slot] = Some(reply.client);
        Ok(())
    }
    pub(super) fn assign(
        &mut self,
        connection: &Connection,
        rider: &Rider,
        roster: &Roster,
    ) -> Result<RiderId, LobbyError> {
        let credential = connection.client().ok_or(LobbyError::Protocol)?;
        let slot = self
            .credentials
            .iter()
            .position(|candidate| *candidate == Some(credential))
            .ok_or(LobbyError::Protocol)?;
        if roster[slot].is_some() || self.replacement.roster[slot].as_ref() != Some(rider) {
            return Err(LobbyError::Protocol);
        }
        self.credentials[slot] = None;
        Ok(RiderId::new(
            u8::try_from(slot).map_err(|_| LobbyError::Protocol)?,
        )?)
    }
    pub(super) fn activate(
        &mut self,
        epoch: u64,
        proof: &Proof,
        roster: &Roster,
    ) -> Result<bool, LobbyError> {
        let members = roster.iter().enumerate().fold(0_u8, |mask, (slot, rider)| {
            if rider.is_some() {
                mask | (1 << slot)
            } else {
                mask
            }
        });
        // A standby takes over after losing riders, the organizer among them, never with
        // anyone unplanned (#426); a planned change needs everyone.
        let planned = self.replacement.plan.context.members;
        let complete = if self.replacement.standby {
            members & !planned == 0
        } else {
            members == planned
        };
        if epoch != self.replacement.plan.context.epoch
            || !self.replacement.plan.accepts(proof)
            || !complete
        {
            return Err(LobbyError::Protocol);
        }
        let changed = !self.active;
        self.active = true;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{Assistance, ContentId, ContentKind, Selection};
    use crate::discovery::GatherOptions;
    use crate::handover::{Message, WorkoutPosition};
    use crate::link::{ConnectOptions, Joiner};
    use crate::lobby::{HostLobby, JoinerLobby, LobbyEvent, RideProgress};
    use crate::policy::AddressPolicy;
    use crate::voice::{Playout, VoiceFrame};
    use orqwa_domain::units::{Kilograms, Meters};
    use std::time::Duration;
    use tokio::time::timeout;

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
    async fn client_event(
        client: &mut JoinerLobby,
        matches: impl Fn(&LobbyEvent) -> bool,
    ) -> LobbyEvent {
        timeout(Duration::from_secs(2), async {
            loop {
                let event = client.next().await.unwrap();
                if matches(&event) {
                    return event;
                }
            }
        })
        .await
        .unwrap()
    }
    async fn host_event(host: &mut HostLobby, matches: impl Fn(&LobbyEvent) -> bool) {
        timeout(Duration::from_secs(2), async {
            loop {
                if matches(&host.next().await.unwrap()) {
                    return;
                }
            }
        })
        .await
        .unwrap();
    }
    async fn join(
        host: &HostLobby,
        id: RiderId,
        plan: &Plan,
        roster: &Roster,
        selection: &Selection,
    ) -> JoinerLobby {
        let joiner = Joiner::lobby(plan.invite.clone(), &options())
            .await
            .unwrap();
        host.admit_as(joiner.reply(), id).await.unwrap();
        JoinerLobby::prepare(
            joiner,
            roster[usize::from(id.value())].clone().unwrap(),
            id,
            plan.clone(),
            roster.clone(),
            selection.clone(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)] // a pinned but premature successor must not buffer media or poison clock ordering
    async fn prepared_clients_discard_uncommitted_clock_state_and_voice() {
        use crate::link::HostEvent;
        use crate::state::RiderState;
        use crate::wire::{self, Message as Frame};
        use orqwa_domain::units::{MetersPerSecond, Rpm, Watts};

        let organizer = RiderId::new(3).unwrap();
        let mut roster: Roster = std::array::from_fn(|_| None);
        for (id, name) in [(0, "Previous"), (3, "Successor")] {
            roster[id] = Some(Rider {
                name: name.into(),
                mass: Kilograms(80.0),
                weight: Kilograms(70.0),
                avatar: orqwa_domain::profile::Avatar::Female,
            });
        }
        let selection = Selection {
            revision: 1,
            name: "Workout".into(),
            course: None,
            course_key: None,
            workout: Some(ContentId::of(ContentKind::Workout, b"workout").unwrap()),
            assistance: Assistance::default(),
        };
        let mut transport = Host::lobby("Successor", &options()).await.unwrap();
        let root = crate::code::RideId([1; 16]);
        let context = Context::new(root, 2, RiderId::HOST, organizer, &roster, &selection).unwrap();
        let (plan, proof) = Plan::prepare(
            context,
            transport.invite().clone(),
            Duration::from_secs(30),
            Some(WorkoutPosition {
                elapsed: Duration::from_secs(30),
                paused: true,
                sequence: 4,
            }),
        )
        .unwrap();
        let joiner = Joiner::lobby(transport.invite().clone(), &options())
            .await
            .unwrap();
        transport.admit(joiner.reply()).unwrap();
        let metadata = roster.clone();
        let chosen = selection.clone();
        let server = tokio::spawn(async move {
            let HostEvent::Joined(connection) = transport.next().await.unwrap() else {
                panic!("expected pinned connection");
            };
            let (mut send, mut receive) = connection.accept_bi().await.unwrap();
            assert!(matches!(
                wire::read(&mut receive).await.unwrap(),
                Frame::Hello(..)
            ));
            wire::write(
                &mut send,
                &Frame::Welcome(RiderId::HOST, organizer, root, 2, metadata),
            )
            .await
            .unwrap();
            wire::write(&mut send, &Frame::Selection(chosen))
                .await
                .unwrap();
            wire::write(&mut send, &Frame::Running(30_000))
                .await
                .unwrap();
            let mut epochs = [0; MAX_RIDERS];
            epochs[0] = 1;
            epochs[3] = 2;
            wire::write(&mut send, &Frame::VoiceRoster(epochs))
                .await
                .unwrap();
            (transport, connection, send, receive)
        });
        let mut client = JoinerLobby::prepare(
            joiner,
            roster[0].clone().unwrap(),
            RiderId::HOST,
            plan,
            roster,
            selection,
        )
        .await
        .unwrap();
        let (_transport, connection, mut send, _receive) = server.await.unwrap();
        let voice = client.take_voice().unwrap();
        assert!(!voice.alive());
        let premature = super::super::timing::WorkoutSample {
            revision: 1,
            sequence: 1_000,
            group: Duration::from_secs(30),
            elapsed: Duration::from_mins(15),
            paused: true,
        };
        wire::write(&mut send, &Frame::Workout(premature))
            .await
            .unwrap();
        let state = RiderState {
            at: Duration::from_secs(30),
            distance: Meters(10.0),
            speed: MetersPerSecond(2.0),
            power: Watts(100.0),
            cadence: Rpm(80.0),
        };
        connection
            .send_datagram(&state.datagram(organizer).unwrap())
            .unwrap();
        let (mut speech, len) = VoiceFrame::new(organizer, 1, 0, b"early speech")
            .unwrap()
            .datagram();
        speech[11..19].copy_from_slice(&2_u64.to_be_bytes());
        connection.send_datagram(&speech[..len]).unwrap();
        let end = tokio::time::Instant::now() + Duration::from_millis(250);
        while let Ok(Some(event)) = tokio::time::timeout_at(end, client.next()).await {
            assert!(
                !matches!(event, LobbyEvent::State { .. } | LobbyEvent::Workout(_)),
                "uncommitted data reached the app"
            );
        }
        wire::write(
            &mut send,
            &Frame::Handover(Message::Commit { epoch: 2, proof }),
        )
        .await
        .unwrap();
        client_event(&mut client, |event| {
            matches!(
                event,
                LobbyEvent::Handover {
                    message: Message::Commit { .. },
                    ..
                }
            )
        })
        .await;
        voice.enable(true);
        assert!(
            voice
                .playout()
                .iter()
                .all(|item| matches!(item.action, Playout::Silence))
        );
        let committed = super::super::timing::WorkoutSample {
            sequence: 5,
            ..premature
        };
        wire::write(&mut send, &Frame::Workout(committed))
            .await
            .unwrap();
        client_event(
            &mut client,
            |event| matches!(event,LobbyEvent::Workout(clock) if clock.sequence==5),
        )
        .await;
        client.close().await;
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)] // real stable-ID replacement, commit barrier, voice forwarding, late admission and socket cleanup share one owned listener
    async fn prepared_links_require_the_authorized_proof_and_preserve_ids_voice_and_late_admission()
    {
        let directory = std::env::temp_dir().join(format!(
            "orqwa-handover-{:x?}",
            crate::crypto::random::<8>().unwrap()
        ));
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("workout.oqw");
        std::fs::write(&file, b"frozen workout").unwrap();
        let workout_id = ContentId::of(ContentKind::Workout, b"frozen workout").unwrap();
        let mut roster: Roster = std::array::from_fn(|_| None);
        for (slot, name) in [(0, "Original"), (3, "Successor"), (5, "Friend")] {
            roster[slot] = Some(Rider {
                name: name.into(),
                mass: Kilograms(83.0),
                weight: Kilograms(70.0),
                avatar: orqwa_domain::profile::Avatar::Female,
            });
        }
        let selection = Selection {
            revision: 7,
            name: "Together".into(),
            course: None,
            course_key: None,
            workout: Some(workout_id),
            assistance: Assistance::default(),
        };
        let organizer = RiderId::new(3).unwrap();
        let friend = RiderId::new(5).unwrap();
        let transport = Host::lobby("Successor", &options()).await.unwrap();
        let addresses: Vec<_> = transport
            .invite()
            .candidates
            .iter()
            .map(|candidate| candidate.addr())
            .collect();
        let root = crate::code::RideId([1; 16]);
        let context = Context::new(root, 2, RiderId::HOST, organizer, &roster, &selection).unwrap();
        let workout = Some(WorkoutPosition {
            elapsed: Duration::from_mins(10),
            paused: true,
            sequence: 4,
        });
        let (plan, proof) = Plan::prepare(
            context.clone(),
            transport.invite().clone(),
            Duration::from_mins(5),
            workout,
        )
        .unwrap();
        let (_, wrong) = Plan::prepare(
            context,
            transport.invite().clone(),
            Duration::from_mins(5),
            workout,
        )
        .unwrap();
        let mut host = HostLobby::prepare(
            transport,
            Replacement {
                plan: plan.clone(),
                roster: roster.clone(),
                activity: SharedActivity {
                    selection: selection.clone(),
                    course: None,
                    workout: Some(file),
                },
                start: Instant::now().checked_sub(Duration::from_mins(5)).unwrap(),
                standby: false,
            },
        )
        .unwrap();
        let host_voice = host.take_voice().unwrap();
        assert!(!host_voice.alive());
        let mut first = join(&host, RiderId::HOST, &plan, &roster, &selection).await;
        assert_eq!(first.me(), RiderId::HOST);
        assert_eq!(first.organizer(), organizer);
        assert_eq!(first.root(), root);
        assert_eq!(first.epoch(), 2);
        first
            .handover(Message::Commit {
                epoch: 2,
                proof: proof.clone(),
            })
            .unwrap();
        client_event(&mut first, |event| matches!(event, LobbyEvent::HostLeft)).await;
        host_event(
            &mut host,
            |event| matches!(event, LobbyEvent::Left {id} if *id == RiderId::HOST),
        )
        .await;
        assert!(
            !host_voice.alive(),
            "proof cannot activate before all required links exist"
        );
        first.close().await;
        let mut first = join(&host, RiderId::HOST, &plan, &roster, &selection).await;
        let mut other = join(&host, friend, &plan, &roster, &selection).await;
        other
            .handover(Message::Commit {
                epoch: 2,
                proof: wrong,
            })
            .unwrap();
        client_event(&mut other, |event| matches!(event, LobbyEvent::HostLeft)).await;
        host_event(
            &mut host,
            |event| matches!(event, LobbyEvent::Left {id} if *id == friend),
        )
        .await;
        assert!(
            !host_voice.alive(),
            "a prepared successor has no usable early commit proof"
        );
        other.close().await;
        let mut other = join(&host, friend, &plan, &roster, &selection).await;
        let first_voice = first.take_voice().unwrap();
        let other_voice = other.take_voice().unwrap();
        assert!(!first_voice.alive() && !other_voice.alive());
        first.handover(Message::Commit { epoch: 2, proof }).unwrap();
        client_event(&mut first, |event| {
            matches!(
                event,
                LobbyEvent::Handover {
                    message: Message::Commit { .. },
                    ..
                }
            )
        })
        .await;
        client_event(&mut other, |event| {
            matches!(
                event,
                LobbyEvent::Handover {
                    message: Message::Commit { .. },
                    ..
                }
            )
        })
        .await;
        host_event(&mut host, |event| {
            matches!(
                event,
                LobbyEvent::Handover {
                    message: Message::Commit { .. },
                    ..
                }
            )
        })
        .await;
        assert!(host_voice.alive() && first_voice.alive() && other_voice.alive());
        host_voice.enable(true);
        first_voice.enable(true);
        other_voice.enable(true);
        let spurt = first_voice.microphone(true).unwrap();
        timeout(Duration::from_secs(2), async {
            let (mut host_heard, mut other_heard) = (false, false);
            for sequence in 0..80 {
                first_voice
                    .send(VoiceFrame::new(RiderId::HOST, spurt, sequence, b"speech").unwrap())
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(20)).await;
                host_heard |= matches!(host_voice.playout()[0].action, Playout::Frame(_));
                other_heard |= matches!(other_voice.playout()[0].action, Playout::Frame(_));
                if host_heard && other_heard {
                    return;
                }
            }
            panic!("rider zero must send uplink and the nonzero organizer must forward");
        })
        .await
        .unwrap();
        first_voice.microphone(false).unwrap();
        let joiner = Joiner::lobby(plan.invite.clone(), &options())
            .await
            .unwrap();
        host.admit(joiner.reply()).await.unwrap();
        let mut late = JoinerLobby::join(
            joiner,
            Rider {
                name: "Late".into(),
                mass: Kilograms(75.0),
                weight: Kilograms(70.0),
                avatar: orqwa_domain::profile::Avatar::Female,
            },
        )
        .await
        .unwrap();
        assert_eq!(late.root(), root);
        assert_eq!(late.epoch(), 2);
        assert_eq!(late.organizer(), organizer);
        client_event(&mut late, |event| {
            matches!(event, LobbyEvent::Running { .. })
        })
        .await;
        let downloaded =
            crate::content::fetch(&late.content_link().unwrap(), workout_id, &directory, None)
                .await
                .unwrap();
        assert_eq!(std::fs::read(downloaded.path()).unwrap(), b"frozen workout");
        drop(downloaded);
        host.progress(Some(RideProgress {
            distance: Meters(1200.0),
            workout: Some(Duration::from_mins(10)),
        }))
        .unwrap();
        late.ready(7, true).unwrap();
        late.enter(7).unwrap();
        assert!(matches!(
            client_event(&mut late, |event| matches!(event, LobbyEvent::Entry { .. })).await,
            LobbyEvent::Entry {
                progress: Some(RideProgress {
                    distance: Meters(1200.0),
                    ..
                }),
                ..
            }
        ));
        late.close().await;
        other.close().await;
        first.close().await;
        host.close().await;
        timeout(Duration::from_secs(2), async {
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
        std::fs::remove_dir_all(directory).unwrap();
    }

    async fn standby(
        host: &HostLobby,
        id: RiderId,
        plan: &Plan,
        roster: &Roster,
        selection: &Selection,
    ) -> JoinerLobby {
        let joiner = Joiner::lobby(plan.invite.clone(), &options())
            .await
            .unwrap();
        host.admit_as(joiner.reply(), id).await.unwrap();
        JoinerLobby::standby(
            joiner,
            roster[usize::from(id.value())].clone().unwrap(),
            id,
            plan.clone(),
            roster.clone(),
            selection.clone(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)] // one standby's whole life: preparation, loss, takeover and return
    async fn a_standby_holds_lost_riders_takes_over_by_itself_and_commits_them_when_back() {
        let directory = std::env::temp_dir().join(format!(
            "orqwa-standby-{:x?}",
            crate::crypto::random::<8>().unwrap()
        ));
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("workout.oqw");
        std::fs::write(&file, b"frozen workout").unwrap();
        let mut roster: Roster = std::array::from_fn(|_| None);
        for (slot, name) in [(0, "Original"), (3, "Fallback"), (5, "Friend")] {
            roster[slot] = Some(Rider {
                name: name.into(),
                mass: Kilograms(83.0),
                weight: Kilograms(70.0),
                avatar: orqwa_domain::profile::Avatar::Female,
            });
        }
        let selection = Selection {
            revision: 7,
            name: "Together".into(),
            course: None,
            course_key: None,
            workout: Some(ContentId::of(ContentKind::Workout, b"frozen workout").unwrap()),
            assistance: Assistance::default(),
        };
        let fallback = RiderId::new(3).unwrap();
        let transport = Host::lobby("Fallback", &options()).await.unwrap();
        let root = crate::code::RideId([1; 16]);
        let context = Context::new(root, 2, RiderId::HOST, fallback, &roster, &selection).unwrap();
        let workout = Some(WorkoutPosition {
            elapsed: Duration::from_mins(1),
            paused: false,
            sequence: 4,
        });
        let (plan, proof) = Plan::prepare(
            context.clone(),
            transport.invite().clone(),
            Duration::from_mins(1),
            workout,
        )
        .unwrap();
        let (_, wrong) = Plan::prepare(
            context,
            transport.invite().clone(),
            Duration::from_mins(1),
            workout,
        )
        .unwrap();
        let mut host = HostLobby::prepare(
            transport,
            Replacement {
                plan: plan.clone(),
                roster: roster.clone(),
                activity: SharedActivity {
                    selection: selection.clone(),
                    course: None,
                    workout: Some(file),
                },
                start: Instant::now().checked_sub(Duration::from_mins(2)).unwrap(),
                standby: true,
            },
        )
        .unwrap();
        let host_voice = host.take_voice().unwrap();
        let original = standby(&host, RiderId::HOST, &plan, &roster, &selection).await;
        let mut friend = standby(&host, RiderId::new(5).unwrap(), &plan, &roster, &selection).await;
        // #426: the organizer's network fails; its standby link goes with it, without a goodbye.
        super::super::current(&original.connection).close_for_reconnect();
        host_event(
            &mut host,
            |event| matches!(event, LobbyEvent::Away {id} if *id == RiderId::HOST),
        )
        .await;
        // Only the proof the organizer gave in advance takes over; the ready answer is a barrier.
        host.take_over(2, wrong, None).unwrap();
        let _ = host.ready(7, true).await;
        assert!(!host_voice.alive(), "a wrong proof takes nothing over");
        host.take_over(2, proof, None).unwrap();
        host_event(&mut host, |event| {
            matches!(
                event,
                LobbyEvent::Handover {
                    message: Message::Commit { .. },
                    ..
                }
            )
        })
        .await;
        assert!(host_voice.alive());
        client_event(&mut friend, |event| {
            matches!(
                event,
                LobbyEvent::Handover {
                    message: Message::Commit { .. },
                    ..
                }
            )
        })
        .await;
        // The organizer's ORQWA is back: its standby link reconnects and learns of the takeover.
        let mut original = original;
        original.reconnect();
        client_event(&mut original, |event| {
            matches!(
                event,
                LobbyEvent::Handover {
                    message: Message::Commit { .. },
                    ..
                }
            )
        })
        .await;
        assert_eq!(original.organizer(), fallback);
        drop((original, friend));
        host.close().await;
        std::fs::remove_dir_all(directory).unwrap();
    }
}
