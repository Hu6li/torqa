//! Bounded planned authority changes over existing authenticated group links.
//!
//! A prepared successor cannot commit until the current organizer reveals a fresh secret.
//! The commitment binds that proof to the exact group, epoch, membership, activity and pin.
//! A standby is prepared the same way but kept for the ride: the organizer gives its fallback
//! the proof in advance, to take over only if the organizer is lost (#426).

use std::fmt;
use std::time::Duration;

use crate::activity::Selection;
use crate::code::{Invite, Reply, RideId, RouteHash};
use crate::lobby::{LobbyError, Roster, WorkoutClock};
use crate::policy::AddressPolicy;
use crate::state::RiderId;

const DOMAIN: &[u8] = b"orqwa planned handover v1\0";

/// Frozen group identity and membership, checked against each rider's current authenticated view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    /// Original logical group identity, retained across fresh transport invitations.
    pub root: RideId,
    /// Strictly newer proposal epoch; cancelled proposals still consume their epoch.
    pub epoch: u64,
    /// Current organizer's stable rider number.
    pub predecessor: RiderId,
    /// Consenting successor's stable rider number.
    pub successor: RiderId,
    /// Exactly the current occupied roster slots.
    pub members: u8,
    /// Frozen selection revision.
    pub revision: u64,
    /// Whether the selected activity includes a structured workout.
    pub workout: bool,
    /// Successor name from the frozen roster, also required in the new invitation.
    pub successor_name: String,
    bindings: [u8; 32],
}
impl Context {
    /// Binds a proposed successor to the current roster and complete selected activity.
    ///
    /// # Errors
    /// Returns [`LobbyError::Protocol`] for invalid identities, metadata or an incomplete roster.
    pub fn new(
        root: RideId,
        epoch: u64,
        predecessor: RiderId,
        successor: RiderId,
        roster: &Roster,
        selection: &Selection,
    ) -> Result<Self, LobbyError> {
        selection.validate()?;
        let mut bytes = selection.encode()?;
        let mut members = 0_u8;
        for (slot, rider) in roster.iter().enumerate() {
            if let Some(rider) = rider {
                rider.validate()?;
                let slot = u8::try_from(slot).map_err(|_| LobbyError::Protocol)?;
                members |= 1 << slot;
                bytes.extend_from_slice(&[
                    slot,
                    u8::try_from(rider.name.len()).map_err(|_| LobbyError::Protocol)?,
                ]);
                bytes.extend_from_slice(rider.name.as_bytes());
                bytes.extend_from_slice(&rider.mass.0.to_be_bytes());
            }
        }
        let successor_name = roster[usize::from(successor.value())]
            .as_ref()
            .ok_or(LobbyError::Protocol)?
            .name
            .clone();
        let context = Self {
            root,
            epoch,
            predecessor,
            successor,
            members,
            revision: selection.revision,
            workout: selection.workout.is_some(),
            successor_name,
            bindings: crate::crypto::sha256(&bytes),
        };
        context.validate()?;
        Ok(context)
    }
    fn validate(&self) -> Result<(), LobbyError> {
        if self.root.0 == [0; 16]
            || self.epoch < 2
            || self.predecessor == self.successor
            || self.members.count_ones() < 2
            || self.members & (1 << self.predecessor.value()) == 0
            || self.members & (1 << self.successor.value()) == 0
            || self.revision == 0
            || !crate::code::valid_name(&self.successor_name)
        {
            return Err(LobbyError::Protocol);
        }
        Ok(())
    }
    fn write(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&self.root.0);
        bytes.extend_from_slice(&[
            self.predecessor.value(),
            self.successor.value(),
            self.members,
            u8::from(self.workout),
        ]);
        bytes.extend_from_slice(&self.revision.to_be_bytes());
        bytes.push(u8::try_from(self.successor_name.len()).expect("validated bounded name"));
        bytes.extend_from_slice(self.successor_name.as_bytes());
        bytes.extend_from_slice(&self.bindings);
    }
}

/// Transferable proof of the old organizer's authorized commit; never printed in diagnostics.
#[derive(Clone, PartialEq, Eq)]
pub struct Proof([u8; 32]);
impl fmt::Debug for Proof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Proof([REDACTED])")
    }
}

/// The shared workout position at a specified point on the original group clock.
#[derive(Clone, Copy, Debug)]
pub struct WorkoutPosition {
    /// Current position including previous skips.
    pub elapsed: Duration,
    /// Whether the organizer paused the workout.
    pub paused: bool,
    /// Last update of the old authority; the next owner retains ordering within its new epoch.
    pub sequence: u64,
}

/// Fresh pinned transport and frozen timeline, prepared before any authority switch.
/// This intentionally has no Debug representation because it contains connection addresses.
#[derive(Clone)]
pub struct Plan {
    /// Expected group and member binding.
    pub context: Context,
    /// Successor's fresh certificate, transport ride identity and advertised addresses.
    pub invite: Invite,
    /// Point on the original group clock at which `workout` was sampled.
    pub group_at: Duration,
    /// Shared structured-workout time, absent on course-only rides.
    pub workout: Option<WorkoutPosition>,
    commitment: [u8; 32],
}
impl Plan {
    /// Creates a fresh commitment. Only the current organizer retains the returned proof.
    ///
    /// # Errors
    /// Returns [`LobbyError`] for invalid bindings or unavailable cryptographic entropy.
    pub fn prepare(
        context: Context,
        invite: Invite,
        group_at: Duration,
        workout: Option<WorkoutPosition>,
    ) -> Result<(Self, Proof), LobbyError> {
        let proof = Proof(crate::crypto::random().map_err(|_| LobbyError::Protocol)?);
        let mut plan = Self {
            context,
            invite,
            group_at,
            workout,
            commitment: [0; 32],
        };
        plan.validate(codec_policy())?;
        plan.commitment = plan.digest(&proof)?;
        Ok((plan, proof))
    }
    /// Checks group identity and destination policy without starting discovery or probes.
    ///
    /// # Errors
    /// Returns [`LobbyError::Protocol`] if membership/content changed or the pin is malformed;
    /// forbidden destinations retain their normal policy error.
    pub fn check(&self, expected: &Context, policy: AddressPolicy) -> Result<(), LobbyError> {
        self.validate(policy)?;
        if &self.context != expected {
            return Err(LobbyError::Protocol);
        }
        Ok(())
    }
    fn validate(&self, policy: AddressPolicy) -> Result<(), LobbyError> {
        self.context.validate()?;
        self.invite
            .encode(policy)
            .map_err(crate::link::LinkError::from)?;
        if self.invite.route != RouteHash::LOBBY
            || self.invite.host != self.context.successor_name
            || self.group_at > crate::workout::MAX_DURATION
            || self.workout.is_some() != self.context.workout
            || self.workout.is_some_and(|clock| {
                clock.elapsed > crate::workout::MAX_DURATION || clock.sequence == 0
            })
        {
            return Err(LobbyError::Protocol);
        }
        Ok(())
    }
    /// Whether the revealed proof authorizes this exact replacement authority.
    #[must_use]
    pub fn accepts(&self, proof: &Proof) -> bool {
        self.digest(proof)
            .is_ok_and(|digest| digest == self.commitment)
    }
    /// Maps the transferred workout time onto the already established local group clock.
    ///
    /// # Errors
    /// Returns [`LobbyError::Protocol`] on an overflowing or out-of-range clock.
    pub fn clock(&self, start: std::time::Instant) -> Result<Option<WorkoutClock>, LobbyError> {
        self.workout
            .map(|clock| {
                let at = start
                    .checked_add(self.group_at)
                    .ok_or(LobbyError::Protocol)?;
                Ok(WorkoutClock {
                    revision: self.context.revision,
                    sequence: clock.sequence,
                    elapsed: clock.elapsed,
                    at,
                    paused: clock.paused,
                })
            })
            .transpose()
    }
    fn digest(&self, proof: &Proof) -> Result<[u8; 32], LobbyError> {
        self.validate(codec_policy())?;
        let mut bytes = Vec::with_capacity(1024);
        bytes.extend_from_slice(DOMAIN);
        bytes.extend_from_slice(&proof.0);
        bytes.extend_from_slice(&self.context.epoch.to_be_bytes());
        self.write(&mut bytes)?;
        Ok(crate::crypto::sha256(&bytes))
    }
    fn write(&self, bytes: &mut Vec<u8>) -> Result<(), LobbyError> {
        self.context.write(bytes);
        blob(
            bytes,
            &self
                .invite
                .encode(codec_policy())
                .map_err(crate::link::LinkError::from)?,
        )?;
        duration(bytes, self.group_at)?;
        bytes.push(u8::from(self.workout.is_some()));
        if let Some(clock) = self.workout {
            duration(bytes, clock.elapsed)?;
            bytes.push(u8::from(clock.paused));
            bytes.extend_from_slice(&clock.sequence.to_be_bytes());
        }
        Ok(())
    }
}

/// Small typed handover controls over existing authenticated streams. Admission replies
/// are sent privately to the successor, never broadcast to other riders.
#[derive(Clone)]
pub enum Message {
    /// Ask this authenticated member for group-wide fallback consent before preparing links.
    FallbackConsent {
        /// Fresh request epoch, consumed even if cancelled or declined.
        epoch: u64,
    },
    /// The requested member's fallback decision; it authorizes no authority change itself.
    FallbackDecision {
        /// Exact request being answered.
        epoch: u64,
        /// Whether the member agreed to stand in for this group.
        accepted: bool,
    },
    /// Ask a connected prepared rider to take the role.
    Offer {
        /// Proposed next authority epoch.
        epoch: u64,
        /// Intended successor.
        successor: RiderId,
        /// To stand in only if the organizer is lost, rather than to take over now (#426).
        standby: bool,
    },
    /// Explicit consent or rejection from the offered rider.
    Decision {
        /// Proposal being answered.
        epoch: u64,
        /// Local consent.
        accepted: bool,
    },
    /// The current organizer starts the bounded replacement-preparation interval.
    Preparing {
        /// Accepted proposal.
        epoch: u64,
        /// Start of its one-minute interval on the original group clock.
        group_at: Duration,
    },
    /// Fresh successor listener, sent only to the current organizer.
    Invite {
        /// Proposal being prepared.
        epoch: u64,
        /// Fresh public invitation.
        invite: Invite,
    },
    /// Complete authority commitment; reveals no admission credentials or commit secret.
    Plan(Box<Plan>),
    /// Private replacement admission, forwarded only by the old organizer.
    Reply {
        /// Proposal being prepared.
        epoch: u64,
        /// Authenticated original rider identity.
        rider: RiderId,
        /// Fresh one-use reply naming the rider's replacement-link certificate.
        reply: Reply,
    },
    /// A participant owns its replacement link; the successor also confirms accepted links.
    Ready {
        /// Proposal being prepared.
        epoch: u64,
        /// Stable identity whose link is ready.
        rider: RiderId,
    },
    /// Cancel before the proof is revealed; all prepared owners are discarded.
    Cancel {
        /// Proposal being cancelled.
        epoch: u64,
    },
    /// A bounded preparation failure, without peer-supplied diagnostic text.
    Failed {
        /// Proposal whose preparation failed.
        epoch: u64,
    },
    /// Authorized irreversible commit, forwarded through prepared pinned links.
    Commit {
        /// New authority epoch.
        epoch: u64,
        /// Current organizer's revealed proof.
        proof: Proof,
    },
    /// Every standby link is ready: the proof for the fallback to take over if the organizer is
    /// lost, sent privately to the fallback only (#426).
    Proof {
        /// Standby being established.
        epoch: u64,
        /// The commit secret, held back until a takeover.
        proof: Proof,
    },
    /// The standby is established; everyone keeps their link to the fallback (#426).
    Standby {
        /// Established standby.
        epoch: u64,
    },
    /// Over a standby link: this rider has lost the organizer (#426).
    Lost {
        /// Standby this report is for.
        epoch: u64,
    },
    /// Over a standby link: this rider reaches the organizer again.
    Found {
        /// Standby this report is for.
        epoch: u64,
    },
}
impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HandoverMessage")
            .field("epoch", &self.epoch())
            .finish_non_exhaustive()
    }
}
impl Message {
    /// Authority epoch to which this control belongs.
    #[must_use]
    pub const fn epoch(&self) -> u64 {
        match self {
            Self::Offer { epoch, .. }
            | Self::FallbackConsent { epoch }
            | Self::FallbackDecision { epoch, .. }
            | Self::Decision { epoch, .. }
            | Self::Invite { epoch, .. }
            | Self::Reply { epoch, .. }
            | Self::Ready { epoch, .. }
            | Self::Cancel { epoch }
            | Self::Failed { epoch }
            | Self::Commit { epoch, .. }
            | Self::Proof { epoch, .. }
            | Self::Standby { epoch }
            | Self::Lost { epoch }
            | Self::Found { epoch }
            | Self::Preparing { epoch, .. } => *epoch,
            Self::Plan(plan) => plan.context.epoch,
        }
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>, LobbyError> {
        let kind = match self {
            Self::Offer { .. } => 1,
            Self::Decision { .. } => 2,
            Self::Invite { .. } => 3,
            Self::Plan(_) => 4,
            Self::Reply { .. } => 5,
            Self::Ready { .. } => 6,
            Self::Cancel { .. } => 7,
            Self::Failed { .. } => 8,
            Self::Commit { .. } => 9,
            Self::Preparing { .. } => 10,
            Self::Proof { .. } => 11,
            Self::Standby { .. } => 12,
            Self::Lost { .. } => 13,
            Self::Found { .. } => 14,
            Self::FallbackConsent { .. } => 15,
            Self::FallbackDecision { .. } => 16,
        };
        if self.epoch() < 2 {
            return Err(LobbyError::Protocol);
        }
        let mut bytes = vec![kind];
        bytes.extend_from_slice(&self.epoch().to_be_bytes());
        match self {
            Self::Offer {
                successor, standby, ..
            } => bytes.extend_from_slice(&[successor.value(), u8::from(*standby)]),
            Self::Decision { accepted, .. } | Self::FallbackDecision { accepted, .. } => {
                bytes.push(u8::from(*accepted));
            }
            Self::Invite { invite, .. } => blob(
                &mut bytes,
                &invite
                    .encode(codec_policy())
                    .map_err(crate::link::LinkError::from)?,
            )?,
            Self::Plan(plan) => {
                plan.validate(codec_policy())?;
                plan.write(&mut bytes)?;
                bytes.extend_from_slice(&plan.commitment);
            }
            Self::Reply { rider, reply, .. } => {
                bytes.push(rider.value());
                blob(
                    &mut bytes,
                    &reply
                        .encode(codec_policy())
                        .map_err(crate::link::LinkError::from)?,
                )?;
            }
            Self::Ready { rider, .. } => bytes.push(rider.value()),
            Self::Commit { proof, .. } | Self::Proof { proof, .. } => {
                bytes.extend_from_slice(&proof.0);
            }
            Self::Preparing { group_at, .. } => duration(&mut bytes, *group_at)?,
            Self::Cancel { .. }
            | Self::FallbackConsent { .. }
            | Self::Failed { .. }
            | Self::Standby { .. }
            | Self::Lost { .. }
            | Self::Found { .. } => {}
        }
        if bytes.len() > 1000 {
            return Err(LobbyError::Protocol);
        }
        Ok(bytes)
    }
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, LobbyError> {
        if bytes.len() > 1000 {
            return Err(LobbyError::Protocol);
        }
        let mut reader = Reader(bytes);
        let kind = reader.byte()?;
        let epoch = reader.number()?;
        if epoch < 2 {
            return Err(LobbyError::Protocol);
        }
        let message = match kind {
            1 => Self::Offer {
                epoch,
                successor: reader.rider()?,
                standby: reader.boolean()?,
            },
            2 => Self::Decision {
                epoch,
                accepted: reader.boolean()?,
            },
            3 => Self::Invite {
                epoch,
                invite: Invite::decode(reader.blob()?, codec_policy())
                    .map_err(crate::link::LinkError::from)?,
            },
            4 => Self::Plan(Box::new(reader.plan(epoch)?)),
            5 => Self::Reply {
                epoch,
                rider: reader.rider()?,
                reply: Reply::decode(reader.blob()?, codec_policy())
                    .map_err(crate::link::LinkError::from)?,
            },
            6 => Self::Ready {
                epoch,
                rider: reader.rider()?,
            },
            7 => Self::Cancel { epoch },
            8 => Self::Failed { epoch },
            9 => Self::Commit {
                epoch,
                proof: Proof(reader.take()?),
            },
            10 => Self::Preparing {
                epoch,
                group_at: reader.duration()?,
            },
            11 => Self::Proof {
                epoch,
                proof: Proof(reader.take()?),
            },
            12 => Self::Standby { epoch },
            13 => Self::Lost { epoch },
            14 => Self::Found { epoch },
            15 => Self::FallbackConsent { epoch },
            16 => Self::FallbackDecision {
                epoch,
                accepted: reader.boolean()?,
            },
            _ => return Err(LobbyError::Protocol),
        };
        if !reader.0.is_empty() {
            return Err(LobbyError::Protocol);
        }
        Ok(message)
    }
}

// This policy is for bounded binary parsing only. Plan::check, Joiner::open and Host::admit
// each recheck the actual local policy before discovery or probing; decoding grants no access.
fn codec_policy() -> AddressPolicy {
    AddressPolicy {
        lan: true,
        loopback: true,
    }
}
fn blob(bytes: &mut Vec<u8>, value: &[u8]) -> Result<(), LobbyError> {
    if value.len() > crate::code::MAX_PAYLOAD_BYTES {
        return Err(LobbyError::Protocol);
    }
    bytes.extend_from_slice(
        &u16::try_from(value.len())
            .map_err(|_| LobbyError::Protocol)?
            .to_be_bytes(),
    );
    bytes.extend_from_slice(value);
    Ok(())
}
fn duration(bytes: &mut Vec<u8>, value: Duration) -> Result<(), LobbyError> {
    if value > crate::workout::MAX_DURATION {
        return Err(LobbyError::Protocol);
    }
    bytes.extend_from_slice(
        &u64::try_from(value.as_millis())
            .map_err(|_| LobbyError::Protocol)?
            .to_be_bytes(),
    );
    Ok(())
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], LobbyError> {
        let bytes = self
            .0
            .get(..N)
            .ok_or(LobbyError::Protocol)?
            .try_into()
            .map_err(|_| LobbyError::Protocol)?;
        self.0 = &self.0[N..];
        Ok(bytes)
    }
    fn byte(&mut self) -> Result<u8, LobbyError> {
        Ok(self.take::<1>()?[0])
    }
    fn number(&mut self) -> Result<u64, LobbyError> {
        Ok(u64::from_be_bytes(self.take()?))
    }
    fn rider(&mut self) -> Result<RiderId, LobbyError> {
        Ok(RiderId::new(self.byte()?)?)
    }
    fn boolean(&mut self) -> Result<bool, LobbyError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(LobbyError::Protocol),
        }
    }
    fn blob(&mut self) -> Result<&'a [u8], LobbyError> {
        let size = usize::from(u16::from_be_bytes(self.take()?));
        if size > crate::code::MAX_PAYLOAD_BYTES {
            return Err(LobbyError::Protocol);
        }
        let bytes = self.0.get(..size).ok_or(LobbyError::Protocol)?;
        self.0 = &self.0[size..];
        Ok(bytes)
    }
    fn duration(&mut self) -> Result<Duration, LobbyError> {
        let value = Duration::from_millis(self.number()?);
        if value > crate::workout::MAX_DURATION {
            return Err(LobbyError::Protocol);
        }
        Ok(value)
    }
    fn plan(&mut self, epoch: u64) -> Result<Plan, LobbyError> {
        let root = RideId(self.take()?);
        let predecessor = self.rider()?;
        let successor = self.rider()?;
        let members = self.byte()?;
        let workout = self.boolean()?;
        let revision = self.number()?;
        let len = usize::from(self.byte()?);
        if len > crate::code::MAX_NAME_BYTES {
            return Err(LobbyError::Protocol);
        }
        let name = std::str::from_utf8(self.0.get(..len).ok_or(LobbyError::Protocol)?)
            .map_err(|_| LobbyError::Protocol)?
            .to_owned();
        self.0 = &self.0[len..];
        let context = Context {
            root,
            epoch,
            predecessor,
            successor,
            members,
            revision,
            workout,
            successor_name: name,
            bindings: self.take()?,
        };
        let invite =
            Invite::decode(self.blob()?, codec_policy()).map_err(crate::link::LinkError::from)?;
        let group_at = self.duration()?;
        let workout = if self.boolean()? {
            Some(WorkoutPosition {
                elapsed: self.duration()?,
                paused: self.boolean()?,
                sequence: self.number()?,
            })
        } else {
            None
        };
        let plan = Plan {
            context,
            invite,
            group_at,
            workout,
            commitment: self.take()?,
        };
        plan.validate(codec_policy())?;
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{Assistance, ContentId, ContentKind};
    use crate::code::{Candidate, CandidateKind, Fingerprint, NatObservation};
    use crate::lobby::Rider;
    use orqwa_domain::units::Kilograms;

    fn fixture() -> (Context, Invite, Roster, Selection) {
        let mut roster: Roster = std::array::from_fn(|_| None);
        roster[0] = Some(Rider {
            name: "Original".into(),
            mass: Kilograms(83.0),
            weight: Kilograms(70.0),
            avatar: orqwa_domain::profile::Avatar::Female,
        });
        roster[3] = Some(Rider {
            name: "Successor".into(),
            mass: Kilograms(75.0),
            weight: Kilograms(70.0),
            avatar: orqwa_domain::profile::Avatar::Female,
        });
        let selection = Selection {
            revision: 7,
            name: "Ride".into(),
            course: None,
            course_key: None,
            workout: Some(ContentId::of(ContentKind::Workout, b"workout").unwrap()),
            assistance: Assistance::default(),
        };
        let context = Context::new(
            RideId([1; 16]),
            2,
            RiderId::HOST,
            RiderId::new(3).unwrap(),
            &roster,
            &selection,
        )
        .unwrap();
        let invite = Invite {
            ride: RideId([2; 16]),
            fingerprint: Fingerprint([3; 32]),
            route: RouteHash::LOBBY,
            host: "Successor".into(),
            course: "Group ride".into(),
            candidates: vec![
                Candidate::new(
                    CandidateKind::Local,
                    "127.0.0.1:45454".parse().unwrap(),
                    codec_policy(),
                )
                .unwrap(),
            ],
            nat: NatObservation::None,
            setup: crate::code::Setup::default(),
        };
        (context, invite, roster, selection)
    }
    fn prepared() -> (Plan, Proof) {
        let (context, invite, _, _) = fixture();
        Plan::prepare(
            context,
            invite,
            Duration::from_mins(5),
            Some(WorkoutPosition {
                elapsed: Duration::from_mins(10),
                paused: true,
                sequence: 4,
            }),
        )
        .unwrap()
    }

    #[test]
    fn commit_proof_binds_the_group_epoch_members_activity_pin_and_clock() {
        let (plan, proof) = prepared();
        assert!(plan.accepts(&proof));
        assert!(!plan.accepts(&Proof([9; 32])));
        for change in 0..8 {
            let mut other = plan.clone();
            match change {
                0 => other.context.root = RideId([9; 16]),
                1 => other.context.epoch += 1,
                2 => other.context.bindings[0] ^= 1,
                3 => other.invite.fingerprint.0[0] ^= 1,
                4 => other.invite.ride.0[0] ^= 1,
                5 => other.group_at += Duration::from_secs(1),
                6 => other.workout.as_mut().unwrap().paused = false,
                _ => other.workout.as_mut().unwrap().elapsed += Duration::from_secs(1),
            }
            assert!(!other.accepts(&proof), "changed binding {change}");
        }
        let (mut expected, _, mut roster, selection) = fixture();
        plan.check(&expected, codec_policy()).unwrap();
        assert!(
            plan.check(&expected, AddressPolicy::default()).is_err(),
            "binary parsing grants no loopback permission"
        );
        roster[0].as_mut().unwrap().mass = Kilograms(90.0);
        expected = Context::new(
            expected.root,
            expected.epoch,
            expected.predecessor,
            expected.successor,
            &roster,
            &selection,
        )
        .unwrap();
        assert!(plan.check(&expected, codec_policy()).is_err());
        assert_eq!(format!("{proof:?}"), "Proof([REDACTED])");
    }

    #[test]
    fn handover_frames_reject_truncation_trailing_data_and_invalid_ids_flags_or_epochs() {
        let (plan, proof) = prepared();
        let reply = Reply {
            ride: plan.invite.ride,
            fingerprint: plan.invite.fingerprint,
            route: RouteHash::LOBBY,
            client: crate::code::Fingerprint([7; 32]),
            candidates: plan.invite.candidates.clone(),
            nat: NatObservation::None,
        };
        for message in [
            Message::FallbackConsent { epoch: 2 },
            Message::FallbackDecision {
                epoch: 2,
                accepted: true,
            },
            Message::FallbackDecision {
                epoch: 2,
                accepted: false,
            },
            Message::Offer {
                epoch: 2,
                successor: RiderId::new(3).unwrap(),
                standby: false,
            },
            Message::Offer {
                epoch: 2,
                successor: RiderId::new(3).unwrap(),
                standby: true,
            },
            Message::Decision {
                epoch: 2,
                accepted: true,
            },
            Message::Preparing {
                epoch: 2,
                group_at: Duration::from_secs(30),
            },
            Message::Invite {
                epoch: 2,
                invite: plan.invite.clone(),
            },
            Message::Plan(Box::new(plan)),
            Message::Reply {
                epoch: 2,
                rider: RiderId::HOST,
                reply,
            },
            Message::Ready {
                epoch: 2,
                rider: RiderId::HOST,
            },
            Message::Cancel { epoch: 2 },
            Message::Failed { epoch: 2 },
            Message::Commit {
                epoch: 2,
                proof: proof.clone(),
            },
            Message::Proof { epoch: 2, proof },
            Message::Standby { epoch: 2 },
            Message::Lost { epoch: 2 },
            Message::Found { epoch: 2 },
        ] {
            let bytes = message.encode().unwrap();
            assert_eq!(Message::decode(&bytes).unwrap().epoch(), 2);
            for len in 0..bytes.len() {
                assert!(Message::decode(&bytes[..len]).is_err());
            }
            let mut extra = bytes;
            extra.push(0);
            assert!(Message::decode(&extra).is_err());
        }
        let offer = Message::Offer {
            epoch: 2,
            successor: RiderId::HOST,
            standby: true,
        }
        .encode()
        .unwrap();
        let mut nobody = offer.clone();
        nobody[9] = 8;
        assert!(Message::decode(&nobody).is_err());
        let mut maybe = offer;
        maybe[10] = 2;
        assert!(Message::decode(&maybe).is_err(), "a standby or not");
        let mut decision = Message::Decision {
            epoch: 2,
            accepted: true,
        }
        .encode()
        .unwrap();
        decision[9] = 2;
        assert!(Message::decode(&decision).is_err());
        decision[0] = 16;
        assert!(
            Message::decode(&decision).is_err(),
            "fallback consent is a boolean"
        );
        decision[1..9].copy_from_slice(&1_u64.to_be_bytes());
        assert!(Message::decode(&decision).is_err());
        assert!(Message::decode(&vec![0; 1001]).is_err());
    }
}
