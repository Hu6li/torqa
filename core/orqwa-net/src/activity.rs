//! Activity selection and revision-bound readiness for the app integration (ADR 0037).
//!
//! Admission authenticates a lobby, not its mutable activity. File names never select a
//! local path: content is identified by its complete SHA-256 and byte length. A course also
//! carries a [`CourseKey`], so a joiner rides a copy they already have instead (#435).

use crate::code::{MAX_NAME_BYTES, valid_name};
use crate::crypto::sha256;
use crate::state::{MAX_RIDERS, RiderId};

/// Largest shared course archive, before expansion: 512 MiB.
pub const MAX_COURSE_BYTES: u64 = 512 * 1024 * 1024;
/// Largest encoded structured workout: 1 MiB.
pub const MAX_WORKOUT_BYTES: u64 = 1024 * 1024;
/// Maximum selection payload, independent of the lobby's existing control framing.
pub const MAX_SELECTION_BYTES: usize = 256;
const VERSION: u8 = 2;

/// Whether content is a portable course archive or a normalized structured workout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentKind {
    /// A 3D `.tqc` course, including its inputs, without referenced video.
    Course,
    /// A structured workout preserving FTP shares and absolute watt targets.
    Workout,
    /// What the organizer is choosing (#418), served from memory: shown, never saved.
    Preview,
}

impl ContentKind {
    /// Maximum encoded file size for this content kind.
    #[must_use]
    pub const fn max_bytes(self) -> u64 {
        match self {
            Self::Course => MAX_COURSE_BYTES,
            Self::Workout => MAX_WORKOUT_BYTES,
            Self::Preview => crate::preview::MAX_PREVIEW_BYTES,
        }
    }
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Self::Course => 1,
            Self::Workout => 2,
            Self::Preview => 3,
        }
    }
    pub(crate) fn from_tag(tag: u8) -> Result<Self, ActivityError> {
        match tag {
            1 => Ok(Self::Course),
            2 => Ok(Self::Workout),
            3 => Ok(Self::Preview),
            _ => Err(ActivityError::Invalid),
        }
    }
}

/// Immutable identity of the exact bytes selected by the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentId {
    kind: ContentKind,
    bytes: u64,
    hash: [u8; 32],
}

impl ContentId {
    /// Checks a content identity received from a peer before scheduling a transfer.
    ///
    /// # Errors
    /// Returns [`ActivityError::Invalid`] for empty or oversized content.
    pub fn new(kind: ContentKind, bytes: u64, hash: [u8; 32]) -> Result<Self, ActivityError> {
        if bytes == 0 || bytes > kind.max_bytes() {
            return Err(ActivityError::Invalid);
        }
        Ok(Self { kind, bytes, hash })
    }
    /// Identifies an in-memory workout or small course by its complete bytes.
    ///
    /// # Errors
    /// Returns [`ActivityError::Invalid`] if the content exceeds its kind's size limit.
    pub fn of(kind: ContentKind, bytes: &[u8]) -> Result<Self, ActivityError> {
        Self::new(
            kind,
            u64::try_from(bytes.len()).map_err(|_| ActivityError::Invalid)?,
            sha256(bytes),
        )
    }
    /// What the receiving application must validate before import.
    #[must_use]
    pub const fn kind(self) -> ContentKind {
        self.kind
    }
    /// Exact encoded length, enforced while streaming rather than after buffering.
    #[must_use]
    pub const fn bytes(self) -> u64 {
        self.bytes
    }
    /// SHA-256 of the complete encoded file.
    #[must_use]
    pub const fn hash(self) -> [u8; 32] {
        self.hash
    }
    /// Safe local basename derived only from authenticated content bytes.
    #[must_use]
    pub fn basename(self) -> String {
        use std::fmt::Write as _;
        let mut name = String::with_capacity(70);
        for byte in self.hash {
            let _ = write!(name, "{byte:02x}");
        }
        name.push_str(match self.kind {
            ContentKind::Course => ".tqc",
            ContentKind::Workout => ".oqw",
            ContentKind::Preview => ".preview",
        });
        name
    }
    pub(crate) fn write(self, out: &mut Vec<u8>) {
        out.push(self.kind.tag());
        out.extend_from_slice(&self.bytes.to_be_bytes());
        out.extend_from_slice(&self.hash);
    }
    pub(crate) fn read(input: &mut Input<'_>) -> Result<Self, ActivityError> {
        Self::new(
            ContentKind::from_tag(input.byte()?)?,
            u64::from_be_bytes(input.take()?),
            input.take()?,
        )
    }
}

/// What a course rides on, however each rider named and showed their copy (#435): SHA-256 of
/// its storage-layer key material, which leaves out the name, map picture and countries. A
/// joiner whose library holds a course with this key rides that file and downloads nothing.
/// It only ever selects a course the rider already has; transfers still check [`ContentId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CourseKey(pub [u8; 32]);
impl CourseKey {
    /// The key of the course described by `material`.
    #[must_use]
    pub fn of(material: &[u8]) -> Self {
        Self(sha256(material))
    }
}

/// Independent group rules. Assistance amounts are computed individually, not sent by peers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assistance {
    /// Aerodynamic shelter from nearby group riders.
    pub drafting: bool,
    /// Elastic-band assistance around the moving group while pedaling.
    pub keep_together: bool,
}
impl Default for Assistance {
    fn default() -> Self {
        Self {
            drafting: true,
            keep_together: true,
        }
    }
}

/// One host-selected course, workout, or workout on a course.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Strictly increasing within one lobby, never reused even after a failed selection.
    pub revision: u64,
    /// Printable display name, never a file-system path.
    pub name: String,
    /// Portable course, if the ride has one.
    pub course: Option<ContentId>,
    /// The course's key, present exactly when `course` is.
    pub course_key: Option<CourseKey>,
    /// Normalized structured workout, if the ride has one.
    pub workout: Option<ContentId>,
    /// Shared rules, frozen together with this selection at countdown.
    pub assistance: Assistance,
}
impl Selection {
    /// Checks kinds, revision, name and the requirement to select at least one activity.
    ///
    /// # Errors
    /// Returns [`ActivityError::Invalid`] for malformed or empty selections.
    pub fn validate(&self) -> Result<(), ActivityError> {
        if self.revision == 0
            || !valid_name(&self.name)
            || (self.course.is_none() && self.workout.is_none())
            || self.course.is_some() != self.course_key.is_some()
            || self.course.is_some_and(|id| id.kind != ContentKind::Course)
            || self
                .workout
                .is_some_and(|id| id.kind != ContentKind::Workout)
        {
            return Err(ActivityError::Invalid);
        }
        Ok(())
    }
    /// Bounded binary representation for reliable controls; no general deserializer is used.
    ///
    /// # Errors
    /// Returns [`ActivityError::Invalid`] if validation fails.
    pub fn encode(&self) -> Result<Vec<u8>, ActivityError> {
        self.validate()?;
        let mut out = vec![VERSION];
        out.extend_from_slice(&self.revision.to_be_bytes());
        out.push(u8::try_from(self.name.len()).map_err(|_| ActivityError::Invalid)?);
        out.extend_from_slice(self.name.as_bytes());
        out.push(
            u8::from(self.assistance.drafting) | (u8::from(self.assistance.keep_together) << 1),
        );
        out.push(u8::from(self.course.is_some()));
        if let (Some(id), Some(key)) = (self.course, self.course_key) {
            id.write(&mut out);
            out.extend_from_slice(&key.0);
        }
        out.push(u8::from(self.workout.is_some()));
        if let Some(id) = self.workout {
            id.write(&mut out);
        }
        Ok(out)
    }
    /// Parses a selection, rejecting excessive bytes before allocating names or content.
    ///
    /// # Errors
    /// Returns [`ActivityError::Invalid`] for unknown versions, malformed or trailing bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, ActivityError> {
        if bytes.len() > MAX_SELECTION_BYTES {
            return Err(ActivityError::Invalid);
        }
        let mut input = Input(bytes);
        if input.byte()? != VERSION {
            return Err(ActivityError::Invalid);
        }
        let revision = u64::from_be_bytes(input.take()?);
        let name = input.string(MAX_NAME_BYTES)?;
        let flags = input.byte()?;
        if flags > 3 {
            return Err(ActivityError::Invalid);
        }
        let (course, course_key) = match input.byte()? {
            0 => (None, None),
            1 => (
                Some(ContentId::read(&mut input)?),
                Some(CourseKey(input.take()?)),
            ),
            _ => return Err(ActivityError::Invalid),
        };
        let workout = match input.byte()? {
            0 => None,
            1 => Some(ContentId::read(&mut input)?),
            _ => return Err(ActivityError::Invalid),
        };
        let selection = Self {
            revision,
            name,
            course,
            course_key,
            workout,
            assistance: Assistance {
                drafting: flags & 1 != 0,
                keep_together: flags & 2 != 0,
            },
        };
        if !input.0.is_empty() {
            return Err(ActivityError::Invalid);
        }
        selection.validate()?;
        Ok(selection)
    }
}

/// Invalid activity input or an operation inconsistent with the lobby phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ActivityError {
    /// Unknown version, malformed descriptor, invalid target or excessive size.
    #[error("The shared activity is invalid or exceeds its limits.")]
    Invalid,
    /// Only current members may acknowledge readiness.
    #[error("The rider is not a current lobby member.")]
    Membership,
    /// A preparation completed for content that is no longer selected.
    #[error("The selected activity changed. Prepare the current selection.")]
    Revision,
    /// A start was requested before every participant was ready.
    #[error("Wait until every participant is ready.")]
    NotReady,
    /// Countdown has frozen activity selection and group rules.
    #[error("The activity cannot change after the countdown starts.")]
    Started,
}

/// Opaque membership generation; old preparation jobs cannot ready a reused roster slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Participant {
    id: RiderId,
    generation: u64,
}

/// Pure host-owned readiness state; it never operates devices or starts network tasks.
pub struct ActivityLobby {
    organizer: RiderId,
    selection: Option<Selection>,
    slots: [Option<(u64, bool)>; MAX_RIDERS],
    next_generation: u64,
    started: bool,
}
impl Default for ActivityLobby {
    fn default() -> Self {
        Self::new(RiderId::HOST)
    }
}
impl ActivityLobby {
    /// Creates readiness state for the organizer's stable rider identity.
    #[must_use]
    pub fn new(organizer: RiderId) -> Self {
        Self {
            organizer,
            selection: None,
            slots: [None; MAX_RIDERS],
            next_generation: 0,
            started: false,
        }
    }

    /// Current activity, or none while the host is still choosing one.
    #[must_use]
    pub const fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }
    /// Replaces content and rules before countdown, invalidating every readiness acknowledgment.
    ///
    /// # Errors
    /// Returns [`ActivityError`] for invalid, reused revisions or a frozen selection.
    pub fn select(&mut self, selection: Selection) -> Result<(), ActivityError> {
        if self.started {
            return Err(ActivityError::Started);
        }
        selection.validate()?;
        if self
            .selection
            .as_ref()
            .is_some_and(|old| old.revision >= selection.revision)
        {
            return Err(ActivityError::Revision);
        }
        self.selection = Some(selection);
        for (_, ready) in self.slots.iter_mut().flatten() {
            *ready = false;
        }
        Ok(())
    }
    /// Reserves a roster slot, including during a ride for late-join preparation.
    ///
    /// # Errors
    /// Returns [`ActivityError::Membership`] for an occupied slot or exhausted generations.
    pub fn join(&mut self, id: RiderId) -> Result<Participant, ActivityError> {
        let slot = &mut self.slots[usize::from(id.value())];
        if slot.is_some() {
            return Err(ActivityError::Membership);
        }
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or(ActivityError::Membership)?;
        *slot = Some((self.next_generation, false));
        Ok(Participant {
            id,
            generation: self.next_generation,
        })
    }
    /// Removes exactly this membership, without affecting a successor in the same slot.
    pub fn leave(&mut self, member: Participant) {
        let slot = &mut self.slots[usize::from(member.id.value())];
        if slot.is_some_and(|(generation, _)| generation == member.generation) {
            *slot = None;
        }
    }
    /// Acknowledges local verification and preparation for the selected revision.
    ///
    /// # Errors
    /// Returns [`ActivityError`] for departed/replaced members or obsolete content.
    pub fn ready(
        &mut self,
        member: Participant,
        revision: u64,
        ready: bool,
    ) -> Result<(), ActivityError> {
        if self
            .selection
            .as_ref()
            .is_none_or(|s| s.revision != revision)
        {
            return Err(ActivityError::Revision);
        }
        let slot = self.slots[usize::from(member.id.value())]
            .as_mut()
            .ok_or(ActivityError::Membership)?;
        if slot.0 != member.generation {
            return Err(ActivityError::Membership);
        }
        slot.1 = ready;
        Ok(())
    }
    /// Whether initial countdown may begin; late joiners never gate an existing ride.
    #[must_use]
    pub fn can_start(&self) -> bool {
        !self.started
            && self.selection.is_some()
            && self.slots[usize::from(self.organizer.value())].is_some()
            && self.slots.iter().flatten().all(|(_, ready)| *ready)
    }
    /// Freezes selection and settings only once all current participants are prepared.
    ///
    /// # Errors
    /// Returns [`ActivityError::Started`] for a repeated start or `NotReady` otherwise.
    pub fn start(&mut self) -> Result<(), ActivityError> {
        if self.started {
            return Err(ActivityError::Started);
        }
        if !self.can_start() {
            return Err(ActivityError::NotReady);
        }
        self.started = true;
        Ok(())
    }
}

pub(crate) struct Input<'a>(pub(crate) &'a [u8]);
impl Input<'_> {
    pub(crate) fn take<const N: usize>(&mut self) -> Result<[u8; N], ActivityError> {
        let bytes = self
            .0
            .get(..N)
            .ok_or(ActivityError::Invalid)?
            .try_into()
            .map_err(|_| ActivityError::Invalid)?;
        self.0 = &self.0[N..];
        Ok(bytes)
    }
    pub(crate) fn byte(&mut self) -> Result<u8, ActivityError> {
        Ok(self.take::<1>()?[0])
    }
    pub(crate) fn string(&mut self, max: usize) -> Result<String, ActivityError> {
        let len = usize::from(self.byte()?);
        if len > max {
            return Err(ActivityError::Invalid);
        }
        let value = std::str::from_utf8(self.0.get(..len).ok_or(ActivityError::Invalid)?)
            .map_err(|_| ActivityError::Invalid)?;
        self.0 = &self.0[len..];
        Ok(value.into())
    }
}
impl<'a> Input<'a> {
    pub(crate) fn bytes(&mut self, len: usize) -> Result<&'a [u8], ActivityError> {
        let bytes = self.0.get(..len).ok_or(ActivityError::Invalid)?;
        self.0 = &self.0[len..];
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn selection(revision: u64) -> Selection {
        Selection {
            revision,
            name: "Twann – Chasseral".into(),
            course: Some(ContentId::of(ContentKind::Course, b"course").unwrap()),
            course_key: Some(CourseKey::of(b"course")),
            workout: None,
            assistance: Assistance::default(),
        }
    }
    #[test]
    fn selections_are_bounded_and_strictly_decoded() {
        for course in [false, true] {
            for workout in [false, true] {
                let mut s = selection(1);
                s.course = course.then(|| ContentId::of(ContentKind::Course, b"course").unwrap());
                s.course_key = course.then(|| CourseKey::of(b"course"));
                s.workout =
                    workout.then(|| ContentId::of(ContentKind::Workout, b"workout").unwrap());
                if !course && !workout {
                    assert!(s.encode().is_err());
                    continue;
                }
                let bytes = s.encode().unwrap();
                assert_eq!(Selection::decode(&bytes), Ok(s));
                for end in 0..bytes.len() {
                    assert!(Selection::decode(&bytes[..end]).is_err());
                }
                let mut trailing = bytes;
                trailing.push(0);
                assert!(Selection::decode(&trailing).is_err());
            }
        }
        // A course comes with its key, so every joiner can look for their own copy (#435).
        let mut keyless = selection(1);
        keyless.course_key = None;
        assert!(keyless.encode().is_err());
        let mut stray = selection(1);
        stray.course = None;
        stray.workout = Some(ContentId::of(ContentKind::Workout, b"workout").unwrap());
        assert!(stray.encode().is_err());
        assert!(Selection::decode(&[0; MAX_SELECTION_BYTES + 1]).is_err());
        assert!(ContentId::new(ContentKind::Course, MAX_COURSE_BYTES + 1, [1; 32]).is_err());
        assert!(ContentId::new(ContentKind::Workout, 0, [1; 32]).is_err());
    }
    #[test]
    fn selection_revisions_and_membership_generations_gate_start() {
        let mut lobby = ActivityLobby::default();
        let host = lobby.join(RiderId::HOST).unwrap();
        let id = RiderId::new(1).unwrap();
        let old = lobby.join(id).unwrap();
        assert!(!lobby.can_start());
        lobby.select(selection(1)).unwrap();
        lobby.ready(host, 1, true).unwrap();
        lobby.ready(old, 1, true).unwrap();
        assert!(lobby.can_start());
        lobby.select(selection(2)).unwrap();
        assert!(!lobby.can_start());
        assert_eq!(lobby.ready(old, 1, true), Err(ActivityError::Revision));
        lobby.leave(old);
        let new = lobby.join(id).unwrap();
        assert_eq!(lobby.ready(old, 2, true), Err(ActivityError::Membership));
        lobby.leave(old);
        lobby.ready(new, 2, true).unwrap();
        lobby.ready(host, 2, true).unwrap();
        lobby.start().unwrap();
        assert_eq!(lobby.select(selection(3)), Err(ActivityError::Started));
        assert_eq!(lobby.start(), Err(ActivityError::Started));
        let late = lobby.join(RiderId::new(2).unwrap()).unwrap();
        lobby.ready(late, 2, true).unwrap();
    }
}
