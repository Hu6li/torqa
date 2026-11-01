//! Bounded voice datagrams and per-speaker jitter buffers, independent of audio hardware.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::link::Connection;
use crate::state::{MAX_RIDERS, Membership, RiderId};

/// One mono speech frame, at fifty packets per second.
pub const FRAME_TIME: Duration = Duration::from_millis(20);
/// Maximum compressed bytes in one speech frame; larger packets never reach a decoder.
pub const MAX_AUDIO_BYTES: usize = 256;
/// Fixed reorder capacity per speaker: at most 160 ms, never an accumulating audio queue.
pub const JITTER_FRAMES: usize = 8;
const PREBUFFER: Duration = Duration::from_millis(60);
const IDLE: Duration = Duration::from_millis(250);
const BURST: u8 = 10;
const HEADER: usize = 19;
pub(crate) const TAG: u8 = 0x81;

/// Invalid voice traffic. Errors never include audio, credentials or peer-controlled text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum VoiceError {
    /// Incorrect format, size, spurt, ordering or authenticated speaker.
    #[error("The voice packet is invalid.")]
    Invalid,
    /// More than fifty frames per second beyond the bounded jitter burst.
    #[error("The peer is sending too many voice packets.")]
    Flood,
    /// The voice owner is closed, disabled or microphone transmission is muted.
    #[error("Voice is disabled, muted or disconnected.")]
    Disabled,
}

/// One bounded Opus packet. The transport supplies the authenticated speaker separately.
#[derive(Clone, Copy)]
pub struct VoiceFrame {
    speaker: RiderId,
    spurt: u32,
    sequence: u32,
    epoch: u64,
    len: u16,
    bytes: [u8; MAX_AUDIO_BYTES],
}

impl std::fmt::Debug for VoiceFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoiceFrame")
            .field("speaker", &self.speaker)
            .field("sequence", &self.sequence)
            .field("audio", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl VoiceFrame {
    /// Wraps a single encoded 20 ms mono speech frame without keeping an unbounded buffer.
    /// Spurt is a monotonically increasing microphone activation number, starting at one.
    ///
    /// # Errors
    /// Returns [`VoiceError::Invalid`] for an empty/oversized payload or zero spurt.
    pub fn new(
        speaker: RiderId,
        spurt: u32,
        sequence: u32,
        payload: &[u8],
    ) -> Result<Self, VoiceError> {
        if spurt == 0 || payload.is_empty() || payload.len() > MAX_AUDIO_BYTES {
            return Err(VoiceError::Invalid);
        }
        let mut frame = Self {
            speaker,
            spurt,
            sequence,
            epoch: 0,
            len: u16::try_from(payload.len()).map_err(|_| VoiceError::Invalid)?,
            bytes: [0; MAX_AUDIO_BYTES],
        };
        frame.bytes[..payload.len()].copy_from_slice(payload);
        Ok(frame)
    }
    /// This authenticated speaker's host-assigned roster number.
    #[must_use]
    pub const fn speaker(self) -> RiderId {
        self.speaker
    }
    /// Current microphone activation; decoders reset when this changes.
    #[must_use]
    pub const fn spurt(self) -> u32 {
        self.spurt
    }
    /// The compressed payload, without any transport framing.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
    /// Encodes into a fixed caller-owned scratch buffer.
    #[must_use]
    pub fn datagram(&self) -> ([u8; HEADER + MAX_AUDIO_BYTES], usize) {
        let mut bytes = [0; HEADER + MAX_AUDIO_BYTES];
        bytes[..3].copy_from_slice(&[TAG, 1, self.speaker.value()]);
        bytes[3..7].copy_from_slice(&self.spurt.to_be_bytes());
        bytes[7..11].copy_from_slice(&self.sequence.to_be_bytes());
        bytes[11..19].copy_from_slice(&self.epoch.to_be_bytes());
        bytes[HEADER..HEADER + usize::from(self.len)].copy_from_slice(self.payload());
        (bytes, HEADER + usize::from(self.len))
    }
    pub(crate) fn parse(bytes: &[u8], speaker: RiderId) -> Result<Self, VoiceError> {
        if bytes.len() <= HEADER
            || bytes.len() > HEADER + MAX_AUDIO_BYTES
            || bytes[..3] != [TAG, 1, speaker.value()]
        {
            return Err(VoiceError::Invalid);
        }
        let spurt = u32::from_be_bytes(bytes[3..7].try_into().map_err(|_| VoiceError::Invalid)?);
        let sequence =
            u32::from_be_bytes(bytes[7..11].try_into().map_err(|_| VoiceError::Invalid)?);
        let mut frame = Self::new(speaker, spurt, sequence, &bytes[HEADER..])?;
        frame.epoch =
            u64::from_be_bytes(bytes[11..19].try_into().map_err(|_| VoiceError::Invalid)?);
        Ok(frame)
    }
    fn scoped(mut self, epoch: u64) -> Self {
        self.epoch = epoch;
        self
    }
}

/// One due playback action. Missing frames get at most sixty milliseconds of concealment.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // fixed stack-owned frames avoid real-time heap allocations
pub enum Playout {
    /// Decode this one compressed frame.
    Frame(VoiceFrame),
    /// Request one 20 ms packet-loss-concealment frame from the existing decoder.
    Conceal,
    /// Produce silence, without decoding or replaying old speech.
    Silence,
}

/// One playback action bound to a particular admitted rider's roster lifetime.
pub struct VoicePlayback {
    /// Host-assigned membership epoch; decoder state must reset when it changes.
    pub epoch: u64,
    /// The current frame, concealment request or silence.
    pub action: Playout,
}

/// A fixed-size, rate-limited jitter buffer for one authenticated speaker.
pub struct VoiceInbox {
    speaker: RiderId,
    frames: [Option<VoiceFrame>; JITTER_FRAMES],
    spurt: u32,
    latest: Option<(u32, Instant)>,
    next: u32,
    play_at: Option<Instant>,
    missing: u8,
    tokens: u8,
    refilled: Instant,
}

impl VoiceInbox {
    /// Reserves one bounded speaker buffer; no packet allocates a queue entry or task.
    #[must_use]
    pub fn new(speaker: RiderId, now: Instant) -> Self {
        Self {
            speaker,
            frames: [None; JITTER_FRAMES],
            spurt: 0,
            latest: None,
            next: 0,
            play_at: None,
            missing: 0,
            tokens: BURST,
            refilled: now,
        }
    }
    /// Charges ingress before parsing and retains only a bounded, plausible reorder window.
    /// Old/duplicate frames are discarded. Excessive future sequence jumps never alter ordering.
    ///
    /// # Errors
    /// Returns [`VoiceError`] for a forged/malformed packet or sustained flood.
    pub fn ingest(&mut self, bytes: &[u8], now: Instant) -> Result<bool, VoiceError> {
        let elapsed = now.saturating_duration_since(self.refilled);
        let added = u8::try_from(elapsed.as_nanos() / FRAME_TIME.as_nanos()).unwrap_or(BURST);
        if added > 0 {
            self.tokens = self.tokens.saturating_add(added).min(BURST);
            #[allow(clippy::cast_possible_truncation)] // remainder is below 20 million ns
            let remainder =
                Duration::from_nanos((elapsed.as_nanos() % FRAME_TIME.as_nanos()) as u64);
            self.refilled = now.checked_sub(remainder).unwrap_or(now);
        }
        if self.tokens == 0 {
            return Err(VoiceError::Flood);
        }
        self.tokens -= 1;
        let frame = VoiceFrame::parse(bytes, self.speaker)?;
        if frame.spurt < self.spurt {
            return Ok(false);
        }
        let new_spurt = frame.spurt > self.spurt;
        if !new_spurt && let Some((latest, at)) = self.latest {
            let allowed =
                u32::try_from(now.saturating_duration_since(at).as_nanos() / FRAME_TIME.as_nanos())
                    .unwrap_or(u32::MAX)
                    .saturating_add(u32::from(BURST));
            if frame.sequence > latest.saturating_add(allowed) {
                return Err(VoiceError::Invalid);
            }
        }
        let idle = self
            .latest
            .is_none_or(|(_, at)| now.saturating_duration_since(at) >= IDLE);
        if new_spurt || idle || self.play_at.is_none() {
            if !new_spurt
                && self
                    .latest
                    .is_some_and(|(latest, _)| frame.sequence <= latest)
            {
                return Ok(false);
            }
            self.frames.fill(None);
            self.latest = None;
            self.next = frame.sequence;
            self.play_at = Some(now + PREBUFFER);
            self.spurt = frame.spurt;
            self.missing = 0;
        }
        if frame.sequence < self.next {
            return Ok(false);
        }
        let index = usize::try_from(frame.sequence % u32::try_from(JITTER_FRAMES).unwrap_or(8))
            .unwrap_or(0);
        if self.frames[index].is_some_and(|old| old.sequence == frame.sequence) {
            return Ok(false);
        }
        if frame.sequence.saturating_sub(self.next) >= u32::try_from(JITTER_FRAMES).unwrap_or(8) {
            self.next = frame.sequence - u32::try_from(JITTER_FRAMES - 1).unwrap_or(7);
            for queued in &mut self.frames {
                if queued.is_some_and(|old| old.sequence < self.next) {
                    *queued = None;
                }
            }
        }
        if self
            .latest
            .is_none_or(|(sequence, _)| frame.sequence > sequence)
        {
            self.latest = Some((frame.sequence, now));
        }
        self.frames[index] = Some(frame);
        Ok(true)
    }
    /// Takes at most one due frame. Delayed consumers skip old speech instead of replaying it.
    #[must_use]
    pub fn playout(&mut self, now: Instant) -> Playout {
        let Some(at) = self.play_at else {
            return Playout::Silence;
        };
        if now < at {
            return Playout::Silence;
        }
        if self
            .latest
            .is_some_and(|(_, last)| now.saturating_duration_since(last) >= IDLE)
        {
            self.clear();
            return Playout::Silence;
        }
        let skipped =
            u32::try_from(now.saturating_duration_since(at).as_nanos() / FRAME_TIME.as_nanos())
                .unwrap_or(u32::MAX);
        self.next = self.next.saturating_add(skipped);
        let advance = FRAME_TIME.saturating_mul(skipped.saturating_add(1));
        self.play_at = Some(at.checked_add(advance).unwrap_or(now + FRAME_TIME));
        let wanted = self.next;
        self.next = self.next.saturating_add(1);
        for queued in &mut self.frames {
            if queued.is_some_and(|old| old.sequence < wanted) {
                *queued = None;
            }
        }
        let index =
            usize::try_from(wanted % u32::try_from(JITTER_FRAMES).unwrap_or(8)).unwrap_or(0);
        if let Some(frame) = self.frames[index]
            .take()
            .filter(|frame| frame.sequence == wanted)
        {
            self.missing = 0;
            return Playout::Frame(frame);
        }
        self.missing = self.missing.saturating_add(1);
        if self.missing <= 3 {
            Playout::Conceal
        } else {
            Playout::Silence
        }
    }
    /// Discards buffered speech when a rider is muted, leaves or the listener deafens.
    pub fn clear(&mut self) {
        self.frames.fill(None);
        self.play_at = None;
        self.missing = 0;
    }
}

struct Speaker {
    member: Option<Membership>,
    consent: bool,
    inbox: VoiceInbox,
    connection: Option<Connection>,
    epoch: u64,
}
#[allow(clippy::struct_excessive_bools)] // independent lifetime, consent, microphone and playback gates
struct Room {
    me: RiderId,
    organizer: RiderId,
    open: bool,
    enabled: bool,
    microphone: bool,
    deafened: bool,
    spurt: u32,
    muted: u8,
    speakers: [Speaker; MAX_RIDERS],
    uplink: Option<Connection>,
    next_epoch: u64,
}

pub(crate) struct IncomingBudget {
    tokens: u16,
    refilled: Instant,
    burst: u16,
    period: Duration,
}
impl IncomingBudget {
    pub fn new() -> Self {
        Self {
            tokens: 80,
            refilled: Instant::now(),
            burst: 80,
            period: Duration::from_micros(2500),
        }
    }
    pub fn controls() -> Self {
        Self {
            tokens: 8,
            refilled: Instant::now(),
            burst: 8,
            period: Duration::from_secs(1),
        }
    }
    pub fn charge(&mut self) -> Result<(), VoiceError> {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.refilled);
        let period = self.period;
        let added = u16::try_from(elapsed.as_nanos() / period.as_nanos()).unwrap_or(self.burst);
        if added > 0 {
            self.tokens = self.tokens.saturating_add(added).min(self.burst);
            #[allow(clippy::cast_possible_truncation)] // remainder is less than one second
            let remainder = Duration::from_nanos((elapsed.as_nanos() % period.as_nanos()) as u64);
            self.refilled = now.checked_sub(remainder).unwrap_or(now);
        }
        if self.tokens == 0 {
            return Err(VoiceError::Flood);
        }
        self.tokens -= 1;
        Ok(())
    }
}
fn lock(room: &Arc<Mutex<Room>>) -> MutexGuard<'_, Room> {
    room.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A local audio adapter's bounded interface to an existing group connection.
/// Opening this port never opens a microphone. Voice is off and transmission muted initially.
#[derive(Clone)]
pub struct VoicePort {
    room: Arc<Mutex<Room>>,
    consent: tokio::sync::watch::Sender<bool>,
}
pub(crate) struct VoiceOwner(pub VoiceHandle);
#[derive(Clone)]
pub(crate) struct VoiceHandle(Arc<Mutex<Room>>);

#[cfg(test)]
pub(crate) fn channel(me: RiderId) -> (VoicePort, VoiceOwner, tokio::sync::watch::Receiver<bool>) {
    channel_for(me, RiderId::HOST)
}

pub(crate) fn channel_for(
    me: RiderId,
    organizer: RiderId,
) -> (VoicePort, VoiceOwner, tokio::sync::watch::Receiver<bool>) {
    let room = Arc::new(Mutex::new(Room {
        me,
        organizer,
        open: true,
        enabled: false,
        microphone: false,
        deafened: false,
        spurt: 0,
        muted: 0,
        uplink: None,
        next_epoch: 0,
        speakers: std::array::from_fn(|index| Speaker {
            member: None,
            consent: false,
            inbox: VoiceInbox::new(
                RiderId::new(u8::try_from(index).unwrap_or(0)).unwrap_or(RiderId::HOST),
                Instant::now(),
            ),
            connection: None,
            epoch: 0,
        }),
    }));
    let (consent, changes) = tokio::sync::watch::channel(false);
    (
        VoicePort {
            room: room.clone(),
            consent,
        },
        VoiceOwner(VoiceHandle(room)),
        changes,
    )
}

impl VoicePort {
    /// This audio sender's authenticated roster number.
    #[must_use]
    pub fn me(&self) -> RiderId {
        lock(&self.room).me
    }
    /// Whether the group owner still exists. Audio adapters must stop capture on `false`.
    #[must_use]
    pub fn alive(&self) -> bool {
        lock(&self.room).open
    }
    /// Opts in to receiving speech. Disabling also mutes transmission and drops queued audio.
    pub fn enable(&self, enabled: bool) {
        let mut room = lock(&self.room);
        let enabled = enabled && room.open;
        let changed = room.enabled != enabled;
        room.enabled = enabled;
        let me = usize::from(room.me.value());
        room.speakers[me].consent = room.enabled;
        if !room.enabled {
            room.microphone = false;
        }
        for speaker in &mut room.speakers {
            speaker.inbox.clear();
        }
        if changed {
            self.consent.send_replace(room.enabled);
        }
    }
    /// Controls microphone transmission immediately; enabling starts a fresh decoder spurt.
    ///
    /// # Errors
    /// Returns [`VoiceError::Disabled`] if closed, deafened, not opted in or counter exhausted.
    pub fn microphone(&self, enabled: bool) -> Result<u32, VoiceError> {
        let mut room = lock(&self.room);
        if enabled && (!room.open || !room.enabled || room.deafened) {
            return Err(VoiceError::Disabled);
        }
        if enabled && !room.microphone {
            room.spurt = room.spurt.checked_add(1).ok_or(VoiceError::Disabled)?;
        }
        room.microphone = enabled;
        Ok(room.spurt)
    }
    /// Current microphone activation, or `None` while any transmission gate is closed.
    #[must_use]
    pub fn transmitting(&self) -> Option<u32> {
        let room = lock(&self.room);
        (room.open && room.enabled && room.microphone && !room.deafened).then_some(room.spurt)
    }
    /// Silences all received audio, clears pending speech and mutes the microphone.
    /// Undeafening leaves the microphone muted until explicitly enabled again.
    pub fn deafen(&self, deafened: bool) {
        let mut room = lock(&self.room);
        room.deafened = deafened;
        if deafened {
            room.microphone = false;
        }
        for speaker in &mut room.speakers {
            speaker.inbox.clear();
        }
    }
    /// Whether local playback is deafened.
    #[must_use]
    pub fn deafened(&self) -> bool {
        lock(&self.room).deafened
    }
    /// Silences one speaker locally without affecting anyone else's conversation.
    pub fn mute_peer(&self, peer: RiderId, muted: bool) {
        let mut room = lock(&self.room);
        let bit = 1 << peer.value();
        if muted {
            room.muted |= bit;
        } else {
            room.muted &= !bit;
        }
        room.speakers[usize::from(peer.value())].inbox.clear();
    }
    /// Sends one current, correctly paced encoded frame. An old microphone activation cannot
    /// send buffered speech after mute/unmute, and transport congestion drops disposable audio.
    ///
    /// # Errors
    /// Returns [`VoiceError`] for invalid state, closed/muted gates or a local frame flood.
    pub fn send(&self, frame: VoiceFrame) -> Result<(), VoiceError> {
        let mut room = lock(&self.room);
        if !room.open
            || !room.enabled
            || !room.microphone
            || room.deafened
            || frame.spurt != room.spurt
        {
            return Err(VoiceError::Disabled);
        }
        if frame.speaker != room.me {
            return Err(VoiceError::Invalid);
        }
        let (bytes, len) = frame.datagram();
        let slot = usize::from(room.me.value());
        if !room.speakers[slot]
            .inbox
            .ingest(&bytes[..len], Instant::now())?
        {
            return Ok(());
        }
        if room.me == room.organizer {
            let (scoped, size) = frame.scoped(room.speakers[slot].epoch).datagram();
            forward(&room, room.me, &scoped[..size]);
        } else if let Some(connection) = &room.uplink {
            let _ = connection.send_voice(&bytes[..len]);
        }
        Ok(())
    }
    /// Takes one due frame per remote speaker without holding a lock during audio decoding.
    #[must_use]
    pub fn playout(&self, now: Instant) -> [VoicePlayback; MAX_RIDERS] {
        let mut room = lock(&self.room);
        let enabled = room.open && room.enabled && !room.deafened;
        let me = room.me;
        let muted = room.muted;
        std::array::from_fn(|index| {
            let speaker = &mut room.speakers[index];
            let action = if !enabled
                || index == usize::from(me.value())
                || muted & (1 << index) != 0
                || speaker.member.is_none()
            {
                speaker.inbox.clear();
                Playout::Silence
            } else {
                speaker.inbox.playout(now)
            };
            VoicePlayback {
                epoch: speaker.epoch,
                action,
            }
        })
    }
    /// Closes local voice participation without affecting trainer control or group state.
    pub fn disable(&self) {
        self.enable(false);
    }
}

fn forward(room: &Room, source: RiderId, bytes: &[u8]) {
    for (index, speaker) in room.speakers.iter().enumerate() {
        if index != usize::from(source.value())
            && speaker.consent
            && let Some(connection) = &speaker.connection
        {
            let _ = connection.send_voice(bytes);
        }
    }
}
impl VoiceHandle {
    pub(crate) fn suspend(&self) {
        let mut room = lock(&self.0);
        room.open = false;
        room.microphone = false;
        room.enabled = false;
    }
    pub(crate) fn activate(&self) {
        lock(&self.0).open = true;
    }

    pub fn join(&self, member: Membership, connection: Option<Connection>) {
        let mut room = lock(&self.0);
        let id = member.rider();
        room.muted &= !(1 << id.value());
        let epoch = if room.me == room.organizer {
            room.next_epoch = room.next_epoch.saturating_add(1);
            room.next_epoch
        } else {
            0
        };
        room.speakers[usize::from(id.value())] = Speaker {
            member: Some(member),
            consent: false,
            inbox: VoiceInbox::new(id, Instant::now()),
            connection,
            epoch,
        };
    }
    pub fn leave(&self, member: Membership) {
        let mut room = lock(&self.0);
        let speaker = &mut room.speakers[usize::from(member.rider().value())];
        if speaker.member == Some(member) {
            speaker.member = None;
            speaker.consent = false;
            speaker.connection = None;
            speaker.inbox.clear();
            speaker.epoch = 0;
        }
    }
    pub fn uplink(&self, connection: Connection) {
        lock(&self.0).uplink = Some(connection);
    }
    pub fn epochs(&self) -> [u64; MAX_RIDERS] {
        let room = lock(&self.0);
        std::array::from_fn(|index| room.speakers[index].epoch)
    }
    pub fn install_epochs(&self, epochs: [u64; MAX_RIDERS]) -> Result<(), VoiceError> {
        let mut room = lock(&self.0);
        let me = room.me;
        if epochs[0] == 0 || epochs[usize::from(me.value())] == 0 {
            return Err(VoiceError::Invalid);
        }
        for (index, epoch) in epochs.into_iter().enumerate() {
            let speaker = &mut room.speakers[index];
            if (speaker.member.is_some() || index == usize::from(me.value())) != (epoch != 0) {
                return Err(VoiceError::Invalid);
            }
            if speaker.epoch != epoch {
                speaker.inbox = VoiceInbox::new(
                    RiderId::new(u8::try_from(index).unwrap_or(0)).unwrap_or(RiderId::HOST),
                    Instant::now(),
                );
                speaker.epoch = epoch;
            }
        }
        Ok(())
    }
    pub fn consent(&self, member: Membership, enabled: bool) -> Result<(), VoiceError> {
        let mut room = lock(&self.0);
        let speaker = &mut room.speakers[usize::from(member.rider().value())];
        if speaker.member != Some(member) {
            return Err(VoiceError::Invalid);
        }
        speaker.consent = enabled;
        if !enabled {
            speaker.inbox.clear();
        }
        Ok(())
    }
    pub fn ingest(&self, member: Membership, bytes: &[u8]) -> Result<(), VoiceError> {
        let mut room = lock(&self.0);
        let id = member.rider();
        let index = usize::from(id.value());
        let speaker = &mut room.speakers[index];
        if speaker.member != Some(member) {
            return Err(VoiceError::Invalid);
        }
        if room.me != room.organizer {
            let frame = VoiceFrame::parse(bytes, id)?;
            if frame.epoch == 0 || frame.epoch != room.speakers[index].epoch {
                return Ok(());
            }
        }
        let fresh = room.speakers[index].inbox.ingest(bytes, Instant::now())?;
        if room.me == room.organizer && !room.speakers[index].consent {
            room.speakers[index].inbox.clear();
            return Ok(());
        }
        if room.me == room.organizer && fresh && room.speakers[index].consent {
            let frame = VoiceFrame::parse(bytes, id)?;
            if frame.epoch != 0 {
                return Err(VoiceError::Invalid);
            }
            let (scoped, len) = frame.scoped(room.speakers[index].epoch).datagram();
            forward(&room, id, &scoped[..len]);
        }
        Ok(())
    }
}
impl Drop for VoiceOwner {
    fn drop(&mut self) {
        let mut room = lock(&self.0.0);
        room.open = false;
        room.microphone = false;
        for speaker in &mut room.speakers {
            speaker.inbox.clear();
            speaker.connection = None;
        }
        if let Some(connection) = room.uplink.take() {
            connection.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(speaker: RiderId, sequence: u32) -> Vec<u8> {
        let frame = VoiceFrame::new(speaker, 1, sequence, &[1, 2, 3]).unwrap();
        let (bytes, len) = frame.datagram();
        bytes[..len].to_vec()
    }
    #[test]
    fn forged_oversized_empty_and_malformed_packets_are_rejected_before_decode() {
        let now = Instant::now();
        let id = RiderId::new(1).unwrap();
        for bytes in [
            vec![],
            vec![0; 1024],
            bytes(RiderId::HOST, 0),
            vec![TAG, 1, 1],
        ] {
            assert_eq!(
                VoiceInbox::new(id, now).ingest(&bytes, now),
                Err(VoiceError::Invalid)
            );
        }
        assert!(VoiceFrame::new(id, 1, 0, &[]).is_err());
        assert!(VoiceFrame::new(id, 1, 0, &[0; MAX_AUDIO_BYTES + 1]).is_err());
    }
    #[test]
    fn malformed_and_duplicate_packets_consume_the_same_ingress_budget() {
        let now = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, now);
        for _ in 0..BURST {
            assert_eq!(inbox.ingest(&[], now), Err(VoiceError::Invalid));
        }
        assert_eq!(
            inbox.ingest(&bytes(RiderId::HOST, 0), now),
            Err(VoiceError::Flood)
        );
        assert_eq!(
            inbox.ingest(&bytes(RiderId::HOST, 0), now + FRAME_TIME),
            Ok(true)
        );
    }
    #[test]
    fn jitter_reorders_but_slow_consumers_skip_history_and_mute_discards_audio() {
        let now = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, now);
        for sequence in [0, 2, 1] {
            inbox.ingest(&bytes(RiderId::HOST, sequence), now).unwrap();
        }
        assert!(matches!(inbox.playout(now), Playout::Silence));
        for sequence in 0..3 {
            let Playout::Frame(frame) = inbox.playout(now + PREBUFFER + FRAME_TIME * sequence)
            else {
                panic!("frame");
            };
            assert_eq!(frame.sequence, sequence);
        }
        assert!(matches!(
            inbox.playout(now + Duration::from_secs(1)),
            Playout::Silence
        ));
        inbox.clear();
        assert!(matches!(
            inbox.playout(now + Duration::from_secs(2)),
            Playout::Silence
        ));
    }
    #[test]
    fn future_sequences_cannot_poison_ordering_and_correctly_paced_jitter_is_allowed() {
        let now = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, now);
        inbox.ingest(&bytes(RiderId::HOST, 0), now).unwrap();
        assert_eq!(
            inbox.ingest(&bytes(RiderId::HOST, u32::MAX), now),
            Err(VoiceError::Invalid)
        );
        for sequence in 1..1000 {
            let arrival =
                now + FRAME_TIME * sequence + Duration::from_millis(u64::from(sequence % 2));
            assert!(
                inbox
                    .ingest(&bytes(RiderId::HOST, sequence), arrival)
                    .unwrap()
            );
        }
    }

    #[test]
    fn delayed_audio_cannot_enter_a_reused_rider_slot_or_poison_its_sequence() {
        let now = Instant::now();
        let me = RiderId::new(2).unwrap();
        let peer = RiderId::new(1).unwrap();
        let (port, owner, _changes) = channel(me);
        port.enable(true);
        let mut memberships = crate::state::StateInbox::new(now);
        let host = memberships.join(RiderId::HOST, now).unwrap();
        let old = memberships.join(peer, now).unwrap();
        owner.0.join(host, None);
        owner.0.join(old, None);
        let mut epochs = [0; MAX_RIDERS];
        epochs[0] = 1;
        epochs[1] = 2;
        epochs[2] = 3;
        owner.0.install_epochs(epochs).unwrap();
        let stale = VoiceFrame::new(peer, 1, 100, &[1, 2, 3])
            .unwrap()
            .scoped(2)
            .datagram();
        owner.0.leave(old);
        memberships.remove(old);
        let fresh = memberships.join(peer, now).unwrap();
        owner.0.join(fresh, None);
        epochs[1] = 4;
        owner.0.install_epochs(epochs).unwrap();
        owner.0.ingest(fresh, &stale.0[..stale.1]).unwrap();
        assert_eq!(
            owner.0.ingest(old, &stale.0[..stale.1]),
            Err(VoiceError::Invalid)
        );
        let current = VoiceFrame::new(peer, 1, 0, &[1, 2, 3])
            .unwrap()
            .scoped(4)
            .datagram();
        owner.0.ingest(fresh, &current.0[..current.1]).unwrap();
        let playback = port.playout(Instant::now() + PREBUFFER);
        assert_eq!(playback[1].epoch, 4);
        let Playout::Frame(frame) = &playback[1].action else {
            panic!("new speaker frame");
        };
        assert_eq!(frame.sequence, 0);
    }

    #[test]
    fn microphone_gates_default_off_and_old_activations_cannot_send_after_unmute() {
        let (port, _owner, _changes) = channel(RiderId::HOST);
        assert!(port.transmitting().is_none());
        assert!(port.microphone(true).is_err());
        port.enable(true);
        let first = port.microphone(true).unwrap();
        port.microphone(false).unwrap();
        let second = port.microphone(true).unwrap();
        assert!(second > first);
        assert_eq!(
            port.send(VoiceFrame::new(RiderId::HOST, first, 0, &[1]).unwrap()),
            Err(VoiceError::Disabled)
        );
        port.deafen(true);
        assert!(port.transmitting().is_none());
        port.deafen(false);
        assert!(port.transmitting().is_none());
    }

    #[test]
    fn a_new_microphone_activation_can_restart_sequence_but_old_spurts_cannot_return() {
        let now = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, now);
        let packet = |spurt, sequence| {
            let frame = VoiceFrame::new(RiderId::HOST, spurt, sequence, &[1]).unwrap();
            let (bytes, len) = frame.datagram();
            bytes[..len].to_vec()
        };
        assert_eq!(inbox.ingest(&packet(1, 50), now), Ok(true));
        assert_eq!(inbox.ingest(&packet(2, 0), now), Ok(true));
        assert_eq!(inbox.ingest(&packet(1, 51), now), Ok(false));
        assert_eq!(
            inbox.ingest(&packet(2, u32::MAX), now),
            Err(VoiceError::Invalid)
        );
        assert_eq!(inbox.ingest(&packet(2, 1), now), Ok(true));
    }

    #[test]
    fn small_playback_clock_jitter_keeps_continuous_speech_without_decoder_resets() {
        let origin = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, origin);
        for sequence in 0..100_u32 {
            let arrival = origin + FRAME_TIME * sequence;
            inbox
                .ingest(&bytes(RiderId::HOST, sequence), arrival)
                .unwrap();
            if sequence >= 3 {
                let playback = arrival + Duration::from_millis(13 + u64::from(sequence % 2));
                let Playout::Frame(frame) = inbox.playout(playback) else {
                    panic!("continuous playout at {sequence}");
                };
                assert_eq!(frame.sequence, sequence - 3);
            }
        }
    }
}
