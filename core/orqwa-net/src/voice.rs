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
/// Frames buffered before a speaker starts playing: the one playing and one spare.
const START_DEPTH: u32 = 2;
/// Output frames a speaker waits for that depth before a very short word plays anyway.
const START_WAIT: u8 = 3;
/// Consecutive output frames concealed while a speaker's packets are late.
const MAX_CONCEAL: u8 = 3;
/// Output frames over which a speaker's smallest spare depth is measured before trimming.
const TRIM_WINDOW: u8 = 50;
const IDLE: Duration = Duration::from_millis(250);
const BURST: u8 = 10;
// Sender and receiver monotonic clocks are independent; do not accumulate tiny
// clock differences into a false sequence violation over a long conversation.
const CLOCK_SKEW_DIVISOR: u32 = 100;
const HEADER: usize = 19;
pub(crate) const TAG: u8 = 0x81;

/// Invalid voice traffic. Errors never include audio, credentials or peer-controlled text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum VoiceError {
    /// Incorrect format, size, spurt or authenticated speaker.
    #[error("The voice packet is invalid.")]
    Invalid,
    /// Sequence progress exceeds the microphone activation's elapsed time and jitter allowance.
    #[error("The voice sequence advanced too quickly (received {received}, limit {limit}).")]
    Sequence {
        /// Received counter, without any audio payload.
        received: u32,
        /// Highest plausible counter for the current activation.
        limit: u32,
    },
    /// A hard packet budget has been exhausted.
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

/// One playback action for the next 20 ms of output. Missing frames get at most sixty
/// milliseconds of concealment.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // fixed stack-owned frames avoid real-time heap allocations
pub enum Playout {
    /// Decode this one compressed frame.
    Frame(VoiceFrame),
    /// Rebuild the lost frame before this one from the recovery copy it carries. The frame
    /// itself stays queued and plays next.
    Recover(VoiceFrame),
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
///
/// Playback pulls one frame per 20 ms of output, timed by the listener's speaker rather than
/// a wall clock. Speech that runs dry is stretched by concealment, so the buffer grows to fit
/// the network's jitter; a buffer that stays deeper than needed for a second plays closer to
/// live again. The speaker's and listener's sound cards never run at exactly the same rate,
/// and this keeps their difference from becoming skipped words or growing delay.
pub struct VoiceInbox {
    speaker: RiderId,
    frames: [Option<VoiceFrame>; JITTER_FRAMES],
    spurt: u32,
    latest: Option<(u32, Instant)>,
    origin: Option<(u32, Instant)>,
    admitted_at: Instant,
    next: u32,
    playing: bool,
    waited: u8,
    missing: u8,
    spare: u32,
    measured: u8,
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
            origin: None,
            admitted_at: now,
            next: 0,
            playing: false,
            waited: 0,
            missing: 0,
            spare: u32::MAX,
            measured: 0,
            tokens: BURST,
            refilled: now,
        }
    }
    /// Charges ingress before parsing and retains only a bounded, plausible reorder window.
    /// Old/duplicate frames are discarded. Excessive future sequence jumps never alter ordering.
    ///
    /// # Errors
    /// Returns [`VoiceError`] for a forged/malformed packet or invalid sequence.
    /// Brief ingress excess is discarded before parsing, without ending the group.
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
            return Ok(false);
        }
        self.tokens -= 1;
        let frame = VoiceFrame::parse(bytes, self.speaker)?;
        if frame.spurt < self.spurt {
            return Ok(false);
        }
        let new_spurt = frame.spurt > self.spurt;
        // A late arrival must not restart the microphone's sequence clock: a fresh
        // packet can follow it immediately after loss or a change in network delay.
        if !new_spurt && let Some((sequence, at)) = self.origin {
            let frames =
                u32::try_from(now.saturating_duration_since(at).as_nanos() / FRAME_TIME.as_nanos())
                    .unwrap_or(u32::MAX);
            let allowed = frames
                .saturating_add(frames / CLOCK_SKEW_DIVISOR)
                .saturating_add(u32::from(BURST));
            let limit = sequence.saturating_add(allowed);
            if frame.sequence > limit {
                return Err(VoiceError::Sequence {
                    received: frame.sequence,
                    limit,
                });
            }
        }
        if new_spurt {
            // The first arrival may itself be delayed. Buffer admission is an
            // earlier bound; using receipt time can reject live speech forever
            // after that initial delay disappears. Packet budgets bound actual work.
            self.origin = Some((frame.sequence, self.admitted_at));
        }
        let idle = self
            .latest
            .is_none_or(|(_, at)| now.saturating_duration_since(at) >= IDLE);
        let drained = !self.playing && self.frames.iter().all(Option::is_none);
        if new_spurt || idle || drained {
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
            self.spurt = frame.spurt;
            self.restart();
        }
        // Playback only ever moves past frames it played, rebuilt or trimmed.
        if frame.sequence < self.next {
            return Ok(false);
        }
        let index = slot(frame.sequence);
        if self.frames[index].is_some_and(|old| old.sequence == frame.sequence) {
            return Ok(false);
        }
        if frame.sequence.saturating_sub(self.next) >= window() {
            // A listener that stopped pulling keeps only the newest window, never a backlog.
            self.next = frame.sequence - (window() - 1);
            self.evict();
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
    /// Takes the next 20 ms of this speaker's speech; call once per output frame.
    #[must_use]
    pub fn playout(&mut self) -> Playout {
        if !self.playing {
            let Some(first) = self.earliest() else {
                return Playout::Silence;
            };
            self.waited = self.waited.saturating_add(1);
            // Start with one frame in hand for network jitter; a single short word that
            // never fills it still plays after a few frames.
            let depth = self.newest().saturating_sub(first).saturating_add(1);
            if depth < START_DEPTH && self.waited < START_WAIT {
                return Playout::Silence;
            }
            self.next = first;
            self.playing = true;
            self.missing = 0;
        }
        self.trim();
        if let Some(frame) = self.frames[slot(self.next)]
            .take()
            .filter(|frame| frame.sequence == self.next)
        {
            self.next = self.next.saturating_add(1);
            self.missing = 0;
            self.measure();
            return Playout::Frame(frame);
        }
        if let Some(later) = self.earliest() {
            // Lost on the way: rebuild one frame, then carry on with the speech that came.
            let action = if later == self.next.saturating_add(1) {
                self.frames[slot(later)].map_or(Playout::Conceal, Playout::Recover)
            } else {
                Playout::Conceal
            };
            self.next = later;
            self.missing = 0;
            return action;
        }
        // Nothing has arrived yet: stretch rather than skip, so late speech still plays.
        self.missing = self.missing.saturating_add(1);
        if self.missing <= MAX_CONCEAL {
            self.spare = 0;
            return Playout::Conceal;
        }
        self.restart();
        Playout::Silence
    }
    /// Discards buffered speech when a rider is muted, leaves or the listener deafens.
    pub fn clear(&mut self) {
        self.frames.fill(None);
        self.restart();
    }
    fn restart(&mut self) {
        self.playing = false;
        self.waited = 0;
        self.missing = 0;
        self.spare = u32::MAX;
        self.measured = 0;
    }
    fn newest(&self) -> u32 {
        self.latest.map_or(self.next, |(sequence, _)| sequence)
    }
    fn earliest(&self) -> Option<u32> {
        self.frames
            .iter()
            .flatten()
            .map(|frame| frame.sequence)
            .filter(|sequence| *sequence >= self.next)
            .min()
    }
    fn evict(&mut self) {
        for queued in &mut self.frames {
            if queued.is_some_and(|old| old.sequence < self.next) {
                *queued = None;
            }
        }
    }
    fn measure(&mut self) {
        let spare = self.newest().saturating_add(1).saturating_sub(self.next);
        self.spare = self.spare.min(spare);
        self.measured = self.measured.saturating_add(1);
    }
    fn trim(&mut self) {
        if self.measured < TRIM_WINDOW {
            return;
        }
        // Speech queued beyond one spare frame for a whole second only adds delay.
        if self.spare != u32::MAX && self.spare > 1 {
            self.next = self.next.saturating_add(self.spare - 1);
            self.evict();
        }
        self.spare = u32::MAX;
        self.measured = 0;
    }
}

fn window() -> u32 {
    u32::try_from(JITTER_FRAMES).unwrap_or(8)
}
fn slot(sequence: u32) -> usize {
    usize::try_from(sequence % window()).unwrap_or(0)
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
    audio_owner: u64,
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
        self.accept().then_some(()).ok_or(VoiceError::Flood)
    }
    pub fn accept(&mut self) -> bool {
        self.accept_at(Instant::now())
    }
    fn accept_at(&mut self, now: Instant) -> bool {
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
            return false;
        }
        self.tokens -= 1;
        true
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
    audio_owner: Option<u64>,
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
        audio_owner: 0,
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
            audio_owner: None,
        },
        VoiceOwner(VoiceHandle(room)),
        changes,
    )
}

impl VoicePort {
    fn current(&self, room: &Room) -> bool {
        self.audio_owner
            .is_none_or(|owner| owner == room.audio_owner)
    }
    /// Claims a fresh local audio lifetime, starting muted and without listening consent.
    /// Retired workers cannot send, play or revoke a replacement's consent during cleanup.
    /// The original port remains usable for immediate UI mute/leave controls.
    ///
    /// # Errors
    /// Returns [`VoiceError::Disabled`] if closed, superseded or the owner counter is exhausted.
    pub fn claim(&self) -> Result<Self, VoiceError> {
        let mut room = lock(&self.room);
        if !room.open || !self.current(&room) {
            return Err(VoiceError::Disabled);
        }
        room.audio_owner = room
            .audio_owner
            .checked_add(1)
            .ok_or(VoiceError::Disabled)?;
        room.microphone = false;
        room.enabled = false;
        let me = usize::from(room.me.value());
        room.speakers[me].consent = false;
        for speaker in &mut room.speakers {
            speaker.inbox.clear();
        }
        self.consent.send_if_modified(|enabled| {
            let changed = *enabled;
            *enabled = false;
            changed
        });
        Ok(Self {
            room: self.room.clone(),
            consent: self.consent.clone(),
            audio_owner: Some(room.audio_owner),
        })
    }
    /// This audio sender's authenticated roster number.
    #[must_use]
    pub fn me(&self) -> RiderId {
        lock(&self.room).me
    }
    /// Whether the group owner still exists. Audio adapters must stop capture on `false`.
    #[must_use]
    pub fn alive(&self) -> bool {
        let room = lock(&self.room);
        room.open && self.current(&room)
    }
    /// Opts in to receiving speech. Disabling also mutes transmission and drops queued audio.
    pub fn enable(&self, enabled: bool) {
        let mut room = lock(&self.room);
        if !self.current(&room) {
            return;
        }
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
        if !self.current(&room) {
            return Err(VoiceError::Disabled);
        }
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
        (self.current(&room) && room.open && room.enabled && room.microphone && !room.deafened)
            .then_some(room.spurt)
    }
    /// Silences all received audio, clears pending speech and mutes the microphone.
    /// Undeafening leaves the microphone muted until explicitly enabled again.
    pub fn deafen(&self, deafened: bool) {
        let mut room = lock(&self.room);
        if !self.current(&room) {
            return;
        }
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
        let room = lock(&self.room);
        !self.current(&room) || room.deafened
    }
    /// Silences one speaker locally without affecting anyone else's conversation.
    pub fn mute_peer(&self, peer: RiderId, muted: bool) {
        let mut room = lock(&self.room);
        if !self.current(&room) {
            return;
        }
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
    /// Returns [`VoiceError`] for invalid state, closed/muted gates or an invalid sequence.
    /// Local media-budget exhaustion drops the frame without closing the voice session.
    pub fn send(&self, frame: VoiceFrame) -> Result<(), VoiceError> {
        let mut room = lock(&self.room);
        if !self.current(&room)
            || !room.open
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
    /// Takes the next 20 ms per remote speaker; call once per output frame, paced by the
    /// speaker device. No lock is held during audio decoding.
    #[must_use]
    pub fn playout(&self) -> [VoicePlayback; MAX_RIDERS] {
        let mut room = lock(&self.room);
        let current = self.current(&room);
        let enabled = current && room.open && room.enabled && !room.deafened;
        let me = room.me;
        let muted = room.muted;
        std::array::from_fn(|index| {
            let speaker = &mut room.speakers[index];
            let action = if !current {
                Playout::Silence
            } else if !enabled
                || index == usize::from(me.value())
                || muted & (1 << index) != 0
                || speaker.member.is_none()
            {
                speaker.inbox.clear();
                Playout::Silence
            } else {
                speaker.inbox.playout()
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
        assert_eq!(inbox.ingest(&bytes(RiderId::HOST, 0), now), Ok(false));
        assert_eq!(
            inbox.ingest(&bytes(RiderId::HOST, 0), now + FRAME_TIME),
            Ok(true)
        );
    }

    #[test]
    fn a_delayed_speech_batch_drops_excess_and_resumes_with_fresh_audio() {
        let now = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, now);
        inbox.ingest(&bytes(RiderId::HOST, 0), now).unwrap();
        let delayed = now + Duration::from_millis(500);
        for sequence in 1..=25 {
            assert_eq!(
                inbox.ingest(&bytes(RiderId::HOST, sequence), delayed),
                Ok(sequence <= u32::from(BURST))
            );
        }
        let fresh = delayed + FRAME_TIME;
        assert_eq!(inbox.ingest(&bytes(RiderId::HOST, 26), fresh), Ok(true));
        // Recovery stays within the bounded playout window, never a growing backlog.
        assert!((0..=8).any(|_| matches!(
            inbox.playout(),
            Playout::Frame(frame) if frame.sequence == 26
        )));
    }

    #[test]
    fn aggregate_media_budget_recovers_without_borrowing_from_future_time() {
        let mut budget = IncomingBudget::new();
        let now = budget.refilled;
        for _ in 0..80 {
            assert!(budget.accept_at(now));
        }
        for _ in 0..1000 {
            assert!(!budget.accept_at(now));
        }
        assert!(!budget.accept_at(now + Duration::from_micros(2499)));
        assert!(budget.accept_at(now + Duration::from_micros(2500)));
        assert!(!budget.accept_at(now + Duration::from_micros(2500)));
        for _ in 0..80 {
            assert!(budget.accept_at(now + Duration::from_secs(1)));
        }
        assert!(!budget.accept_at(now + Duration::from_secs(1)));
    }
    #[test]
    fn reordered_speech_plays_in_order_and_mute_discards_what_is_queued() {
        let now = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, now);
        for sequence in [0, 2, 1] {
            inbox.ingest(&bytes(RiderId::HOST, sequence), now).unwrap();
        }
        for sequence in 0..3 {
            let Playout::Frame(frame) = inbox.playout() else {
                panic!("frame {sequence}");
            };
            assert_eq!(frame.sequence, sequence);
        }
        // Late speech is stretched for at most sixty milliseconds, then the speaker is quiet.
        for _ in 0..MAX_CONCEAL {
            assert!(matches!(inbox.playout(), Playout::Conceal));
        }
        assert!(matches!(inbox.playout(), Playout::Silence));
        for sequence in 3..6 {
            inbox
                .ingest(&bytes(RiderId::HOST, sequence), now + FRAME_TIME * sequence)
                .unwrap();
        }
        inbox.clear();
        for _ in 0..=START_WAIT {
            assert!(matches!(inbox.playout(), Playout::Silence));
        }
    }

    /// What one listener plays, pulling once per 20 ms of output, while packets arrive at
    /// `arrivals[k]` milliseconds for sequence `k` (`None` = lost on the way).
    fn listen(arrivals: &[Option<f64>], pulls: usize) -> Vec<Playout> {
        let origin = Instant::now();
        let at = |ms: f64| origin + Duration::from_secs_f64(ms / 1000.0);
        let mut inbox = VoiceInbox::new(RiderId::HOST, origin);
        let mut pending: Vec<(f64, u32)> = arrivals
            .iter()
            .enumerate()
            .filter_map(|(sequence, ms)| Some(((*ms)?, u32::try_from(sequence).ok()?)))
            .collect();
        pending.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut pending = pending.into_iter().peekable();
        (0..pulls)
            .map(|pull| {
                #[allow(clippy::cast_precision_loss)]
                let now = 5.0 + 20.0 * pull as f64;
                while let Some((ms, sequence)) = pending.next_if(|(ms, _)| *ms <= now) {
                    inbox
                        .ingest(&bytes(RiderId::HOST, sequence), at(ms))
                        .unwrap();
                }
                inbox.playout()
            })
            .collect()
    }
    fn played(playback: &[Playout]) -> Vec<u32> {
        playback
            .iter()
            .filter_map(|action| match action {
                Playout::Frame(frame) => Some(frame.sequence),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_lost_packet_is_rebuilt_from_the_next_one_and_speech_carries_on() {
        let lost = [2, 5, 6];
        let arrivals: Vec<_> = (0..10_u32)
            .map(|sequence| (!lost.contains(&sequence)).then_some(f64::from(sequence) * 20.0))
            .collect();
        let playback = listen(&arrivals, 10);
        let actions: Vec<String> = playback
            .iter()
            .map(|action| match action {
                Playout::Frame(frame) => format!("play {}", frame.sequence),
                Playout::Recover(frame) => format!("rebuild before {}", frame.sequence),
                Playout::Conceal => "conceal".into(),
                Playout::Silence => "silence".into(),
            })
            .collect();
        assert_eq!(
            actions,
            [
                "silence",
                "play 0",
                "play 1",
                "rebuild before 3",
                "play 3",
                "play 4",
                "conceal",
                "conceal",
                "play 7",
                "play 8"
            ]
        );
    }

    #[test]
    fn future_sequences_cannot_poison_ordering_and_correctly_paced_jitter_is_allowed() {
        let now = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, now);
        inbox.ingest(&bytes(RiderId::HOST, 0), now).unwrap();
        assert_eq!(
            inbox.ingest(&bytes(RiderId::HOST, u32::MAX), now),
            Err(VoiceError::Sequence {
                received: u32::MAX,
                limit: u32::from(BURST)
            })
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
    fn delayed_microphone_packets_cannot_move_the_sequence_clock_forward() {
        let now = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, now);
        inbox.ingest(&bytes(RiderId::HOST, 0), now).unwrap();
        // Frame 50 was emitted at 1 s but delayed by 500 ms; frame 75 arrives
        // promptly after it. Both respect the microphone's 50 Hz source rate.
        inbox
            .ingest(&bytes(RiderId::HOST, 50), now + Duration::from_millis(1500))
            .unwrap();
        assert_eq!(
            inbox.ingest(&bytes(RiderId::HOST, 75), now + Duration::from_millis(1501)),
            Ok(true)
        );
        assert_eq!(
            inbox.ingest(&bytes(RiderId::HOST, 76), now + Duration::from_millis(1520)),
            Ok(true)
        );
    }

    #[test]
    fn a_delayed_first_packet_cannot_make_all_following_live_speech_look_too_fast() {
        let now = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, now);
        // A stall starts before any speech arrives. The first old frame must not
        // establish a clock that permanently rejects the source's fresh frames.
        let resumed = now + Duration::from_secs(1);
        assert_eq!(inbox.ingest(&bytes(RiderId::HOST, 0), resumed), Ok(true));
        for sequence in 50..100 {
            let at = resumed + FRAME_TIME * (sequence - 50) + Duration::from_millis(1);
            assert_eq!(inbox.ingest(&bytes(RiderId::HOST, sequence), at), Ok(true));
        }
    }

    #[test]
    fn successive_small_future_jumps_cannot_accumulate_sequence_allowance() {
        let now = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, now);
        inbox.ingest(&bytes(RiderId::HOST, 0), now).unwrap();
        inbox
            .ingest(&bytes(RiderId::HOST, 11), now + FRAME_TIME)
            .unwrap();
        assert_eq!(
            inbox.ingest(&bytes(RiderId::HOST, 22), now + FRAME_TIME * 2),
            Err(VoiceError::Sequence {
                received: 22,
                limit: 12
            })
        );
        assert_eq!(
            inbox.ingest(&bytes(RiderId::HOST, 12), now + FRAME_TIME * 3),
            Ok(true)
        );
    }

    #[test]
    fn independent_clocks_do_not_accumulate_false_sequence_errors_during_long_activations() {
        let now = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, now);
        inbox.ingest(&bytes(RiderId::HOST, 0), now).unwrap();
        let later = now + Duration::from_mins(30);
        assert_eq!(inbox.ingest(&bytes(RiderId::HOST, 90_045), later), Ok(true));
        assert_eq!(
            inbox.ingest(&bytes(RiderId::HOST, 90_911), later),
            Err(VoiceError::Sequence {
                received: 90_911,
                limit: 90_910
            })
        );
        assert_eq!(
            inbox.ingest(&bytes(RiderId::HOST, 90_046), later + FRAME_TIME),
            Ok(true)
        );
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
        let playback = (0..START_WAIT)
            .map(|_| port.playout())
            .find(|playback| !matches!(playback[1].action, Playout::Silence))
            .expect("the new speaker is heard");
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
            Err(VoiceError::Sequence {
                received: u32::MAX,
                limit: u32::from(BURST)
            })
        );
        assert_eq!(inbox.ingest(&packet(2, 1), now), Ok(true));
    }

    #[test]
    fn network_jitter_keeps_continuous_speech_without_concealment() {
        // Up to 16 ms between the earliest and latest arrival of evenly sent frames.
        let arrivals: Vec<_> = (0..500_u32)
            .map(|sequence| Some(f64::from(sequence) * 20.0 + f64::from((sequence * 7) % 17)))
            .collect();
        let playback = listen(&arrivals, 500);
        let heard = played(&playback);
        assert!(heard.windows(2).all(|pair| pair[1] == pair[0] + 1));
        assert!(heard.len() > 490, "heard {}", heard.len());
        assert!(
            !playback
                .iter()
                .skip_while(|action| matches!(action, Playout::Silence))
                .any(|action| matches!(action, Playout::Conceal | Playout::Silence))
        );
    }

    #[test]
    fn sound_cards_running_at_different_rates_neither_skip_words_nor_drift_behind() {
        // Real devices differ by up to a few hundred parts per million; exaggerate it.
        for period in [19.9, 20.1] {
            let arrivals: Vec<_> = (0..3000_u32)
                .map(|sequence| Some(f64::from(sequence) * period))
                .collect();
            let playback = listen(&arrivals, 2950);
            let heard = played(&playback);
            let concealed = playback
                .iter()
                .filter(|action| matches!(action, Playout::Conceal))
                .count();
            let skipped: u32 = heard.windows(2).map(|pair| pair[1] - pair[0] - 1).sum();
            // A 0.5 % difference is 15 frames in 3000; each is absorbed once, not repeatedly.
            assert!(concealed <= 20, "{period} ms: concealed {concealed}");
            assert!(skipped <= 20, "{period} ms: skipped {skipped}");
            // Delay stays small: the last frame played is close behind the last one sent.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let sent = ((5.0 + 20.0 * 2949.0) / period) as u32;
            let behind = sent - heard.last().copied().unwrap_or(0);
            assert!(behind <= 4, "{period} ms: {behind} frames behind");
        }
    }

    #[test]
    fn a_slow_speaker_is_stretched_rather_than_cut_into_pieces() {
        // A sender at half speed: every frame it does send is still played.
        let arrivals: Vec<_> = (0..250_u32)
            .map(|sequence| Some(f64::from(sequence) * 40.0))
            .collect();
        let heard = played(&listen(&arrivals, 500));
        assert!(heard.len() > 240, "heard {} of 250 frames", heard.len());
    }

    #[test]
    fn a_stalled_listener_skips_history_beyond_the_window_and_then_catches_up_with_live() {
        let now = Instant::now();
        let mut inbox = VoiceInbox::new(RiderId::HOST, now);
        for sequence in 0..=50 {
            inbox
                .ingest(&bytes(RiderId::HOST, sequence), now + FRAME_TIME * sequence)
                .unwrap();
        }
        let Playout::Frame(frame) = inbox.playout() else {
            panic!("a live producer must still be audible after listener scheduling stalls");
        };
        assert!(
            frame.sequence > 50 - window(),
            "old speech beyond the jitter window must be skipped, not replayed"
        );
        // Live speech keeps coming; within two seconds playback is close behind it again.
        let mut last = frame.sequence;
        for sequence in 51..=150 {
            inbox
                .ingest(&bytes(RiderId::HOST, sequence), now + FRAME_TIME * sequence)
                .unwrap();
            if let Playout::Frame(frame) = inbox.playout() {
                last = frame.sequence;
            }
        }
        assert!(150 - last <= 2, "{} frames behind", 150 - last);
    }
}
