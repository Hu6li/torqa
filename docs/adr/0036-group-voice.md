# ADR 0036 — Optional bounded voice over the group host

- Status: proposed
- Date: 2026-10-28
- Extends ADR 0035 and requirements R162–R163

## Decision

Add voice to the existing CLI group ride. The owner requested voice and chose reliability
and low overhead over a particular topology. Use the ride's existing host connections:
seven QUIC connections for eight riders, rather than a 28-connection voice mesh. No extra
NAT paths, address exchange, STUN calls, router configuration or external voice relay.
The host forwards compressed packets without re-encoding; each listener mixes locally.
Host loss ends voice while each local ride continues. TLS terminates at the host, which can
access the speech. Any admitted rider who opts into voice can listen; this is no whisper
or end-to-end-encrypted channel between joiners.

`VoiceAudio` in the domain crate defines local capture/playback. `orqwa-voice` owns the
codec, audio thread and CPAL backend; `orqwa-net` owns consent, authenticated sender binding,
fixed speaker buffers and bounded forwarding. Network and audio code have no path to local
trainer control, physics or activity recording. Audio is not persisted, transcribed or uploaded.

## Consent and lifecycle

`--voice` opts into listening, with microphone transmission initially muted. Native input
streams open only after `mic on`; `mic off` invalidates pending speech immediately and the
audio worker closes input. Every activation has a new number, checked before sending, so a
late callback or a queued frame from an older activation cannot resume transmission.
Deafen also mutes the mic; undeafening leaves it muted. Local speaker mute and volume are
independent of trainer/ride settings. Use headphones; acoustic echo cancellation and automatic
noise suppression are not included in this first implementation.

Audio startup runs outside the trainer loop. Audio failures preserve group state and local
riding. The group actor's voice lifetime guard invalidates surviving audio handles and
releases connection references on cancellation/drop. The audio owner stops capture, clears
playback and disables transmission on close/drop. Explicit close waits up to one second for
normal thread cleanup. A native driver stalled inside an OS operation cannot be forcibly
killed safely: transmission/callback gates stay off, and its streams are released when it
returns. Real hardware/permissions/hotplug acceptance remains a separate manual check.

## Protocol and resource limits

- Invite/control version 2 and ALPN `orqwa-group/2` require matching builds and fresh codes.
  Existing address policy, per-ride certificate pinning, one-use admission and deadline remain.
- Mono 48 kHz Opus, 20 ms frames, 24 kbit/s CBR, no DTX, encoder complexity five. Encode
  once; normal compressed packets are 60 bytes. *(Now 32 kbit/s with in-band recovery data:
  see the device-clock amendment below.)* The 19-byte voice header includes version,
  authenticated rider number, microphone activation, sequence and host-assigned membership epoch.
- The host stamps membership epochs from its current admitted roster. Reliable voice-roster
  controls follow joined/left controls in the same stream; receivers reject other epochs before
  changing packet ordering. Decoder state resets across epochs. An old connection cannot submit
  for or remove a reused slot. Voice can therefore operate safely before the countdown too.
- At most 256 compressed bytes per frame; per-speaker ingress burst ten, sustained 50 Hz,
  including malformed/duplicate packets. Forwarded ingress additionally has an aggregate
  80-packet burst and 400 Hz allowance, including unknown/departed speakers. Exhaustion drops
  disposable media before parsing; it does not end the group. A separate connection guard counts
  every raw datagram, allows 880 per one-second window and drops excess, closing only on the third
  consecutive overloaded window. Consent controls have a burst of eight and sustained one per
  second. Malformed packets and sustained raw abuse still disconnect the offending connection;
  invalid compressed audio silences only that speaker locally, preserving other conversation.
- Each speaker has eight fixed jitter slots, a 60 ms prebuffer, at most three concealment
  frames and no playback of speech idle for 250 ms. *(Playout is now pulled by the speaker:
  see the device-clock amendment.)* Delayed consumers skip history. Source
  sequences cannot jump arbitrarily ahead and poison ordering. Decode accepts only mono and
  exactly 960 samples per packet into a fixed output slice; malformed durations and stereo
  are rejected before decode. Native hardening remains enabled.
- Capture and output callback queues have four frames each; callbacks never perform network
  work or wait for it. Captures older than 60 ms and output older than 100 ms are discarded.
  Each live 20 ms tick drains at most four captures and encodes only the freshest valid one;
  late/control wakeups never emit catch-up speech. *(Superseded: every fresh capture is now
  sent; see the device-clock amendment.)*
  At most 640 (now 800) of each connection's 4,096 datagram-send bytes are available for speech,
  leaving space for disposable ride state. Congestion drops speech instead of blocking.
- Each speaker has a separate decoder. Mixing normalizes by speaker count *(now a soft
  limiter instead: device-clock amendment)*, accepts finite
  samples and limits output amplitude; gain is finite and 0–1. Samples and credentials never
  appear in status/errors or Debug. Audio queues and decoder work remain bounded when muted.

## Dependencies and validation

The owner approved proficient audio dependencies on 2026-10-28, subject to clean checks.
Current stable releases were verified against crates.io: CPAL 0.18.2 (Apache-2.0), opusic-c
1.6.1 and opusic-sys 0.7.6 (BSD-3-Clause), bundling upstream Opus 1.6.1. Default hardening,
stack protection and fortification remain enabled; experimental DRED/OSCE are off. The wrapper
is used through fixed-slice encoder/decoder calls and packet-duration inspection; its vector
and soft-clipping helpers are not used. Cargo-deny checks advisories, licences, bans and sources.
ALSA headers and CMake are build dependencies in the container/Linux CI; macOS CI uses CMake.
CLI archives carry generated dependency licences plus the bundled Opus redistribution notice.

Automated tests use local encrypted QUIC and simulated audio devices with the real codec.
They cover default silence/no capture, conversation without self-echo, mute/unmute and deafen,
local speaker mute, malformed speech isolation, roster reuse, microphone generations, stream
cleanup, eight-rider voice with ride state, sample-rate conversion and unchanged FIT isolation.
No tests capture a real microphone, contact public STUN or assert home-router compatibility.
App/GUI voice controls and native microphone/headphone acceptance remain follow-up work.

## Amendment — retiring local audio workers (2026-11-01, #428, #441)

Every audio worker claims a local ownership generation before opening devices. A replacement
starts microphone-muted and invalidates its predecessor's port. Late startup, capture, playback
and cleanup from a retired worker cannot alter the replacement's consent, microphone or queued
speech. The unscoped application port still closes transmission synchronously for mute/leave.
This generation is local only: no protocol, roster, connection or rider identity changes.

Device replacements must also wait for the previous worker's bounded close before opening
exclusive native devices. A stalled OS driver may outlive that wait, but its stale port stays
inert. The regression test holds an old backend inside playback until a replacement is speaking,
then releases it and checks that new microphone consent and group state survive its cleanup.

## Amendment — device sample-rate recovery (2026-11-01, #413)

CPAL classifies a CoreAudio sample-rate change as `StreamInvalidated`: its stream must be
rebuilt, while `DeviceChanged` means a default-device stream already followed the new route.
Rebuild invalidated streams on the audio owner, requerying the selected device's native rate
and channel configuration. Keep an unaffected microphone open: closing a Bluetooth microphone
can switch its profile and invalidate the output again. Actual device choices still reconnect
microphone-off as R169 specifies; a rate change preserves the rider's existing mic consent.
Mute/deafen and stop continue to gate transmission immediately while recovery is in progress.

Discard queued playback and decoder history at recovery. Permit three recoveries per five
seconds, with a 100 ms settling delay before reopening, then report the original device error
through the usual voice diagnostic. Disconnects and permission failures remain explicit errors;
underrun, automatic reroute and denied realtime priority notifications do not end conversation.
Selected devices must support the requested direction, so an output-only device cannot become
a system-audio loopback capture by accident. No package or protocol version changes.

## Amendment — local microphone test and speaker echo gate (2026-11-01, R177)

The microphone test owns a separate capture-only backend with no voice port, codec, recording
or playback. Only a finite peak level enters the app snapshot. It requires explicit microphone
permission, runs for ten seconds, and closes when stopped or its dialog closes. Device changes,
group teardown and handover cancel its owners. Native cleanup is bounded as for voice.
Testing pauses conversation and resumes prior listening microphone-off, preserving device
choices, volume, deafen and local mutes. UI code presents the Rust state and requests actions.

The existing transport excludes the local speaker from forwarding and playout. That does not
prevent another rider's speakers being recaptured by their microphone (#429). Speaker mode
therefore defaults to a half-duplex gate: while mixed remote playback is audible *(now: while a
friend's voiced speech is, see the device-clock amendment)*, discard local capture, including a 300 ms tail covering the output queue and short room echo. Keep capture
open to avoid Bluetooth profile switches and preserve the explicit microphone consent.
Disable **Prevent speaker echo** for full-duplex headphones. This is no adaptive acoustic echo
canceller or noise suppression; competing speakers may need to take turns. No extra dependency,
network service, protocol field or recorded speech is introduced.

## Amendment — visible speech activity and speaker checks (2026-11-01, R178)

The microphone worker estimates speech activity from finite, band-limited frame energy and
zero crossings, with a short onset threshold and release hold between syllables. It rejects
silence, DC, low mains hum and isolated transients; other speech-like sound can trigger it.
This is local activity estimation, not speech recognition or speaker identification. The UI
shows listening/detected states and keeps whether any voice was detected after completion.
Changing the selected input clears its previous result. Samples are never stored or sent.

A shared bounded audio-check owner also supports output-only speaker testing: two faded
tones at voice volume, for at most two seconds. It never calls capture or microphone setup.
The tests are mutually exclusive, close on the existing lifecycle boundaries and restore
prior listening microphone-off. A scrollable settings body keeps every control reachable at
the minimum window size. No dependency, audio asset, network service or protocol change.

## Amendment — Linux default routing (2026-11-01, #456)

CPAL 0.18.2 deliberately reports an unknown direction for ALSA's virtual default device;
its metadata-only `supports_input/output` checks return false for that default. Rejecting
it before asking for the requested configuration broke default Linux voice startup.
Allow unknown ALSA routes to reach the requested input/output configuration check. Known
direction mismatches and the non-ALSA output-loopback protection remain in place. Enumeration
uses metadata only; probing every ALSA plugin can leak native handles on failed opens.

For an unspecified Linux ALSA device, try the PipeWire then PulseAudio bridge when listed,
using the first available requested configuration, otherwise the ALSA default. Permission
and busy errors stop selection. Explicit device IDs never fall back to another device.
Reuse the selected configuration rather than opening it again, and keep microphone setup
behind the existing explicit consent. Put bridge routes first in the picker and omit ALSA's
silent `null` route. Named Bluetooth endpoints remain managed through the desktop sound
settings with this ALSA backend; no native PipeWire/PulseAudio dependency is added.

Regression fixtures execute real CPAL/ALSA configuration queries against a private null-PCM
configuration in child processes. They never access audio hardware or change host settings.
The first fixture reproduces the reported direction error without the fix; the others cover
bridge preference, unavailable-server fallback, explicit choices and known output-only rejection.
The separate “no Bluetooth adapter found” report still requires OS/BlueZ diagnostics; audio
selection cannot create a controller that BlueZ does not expose.

References: [CPAL ALSA implementation](https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/alsa/mod.rs),
[ALSA and sound-server routing](https://github.com/RustAudio/cpal#alsa-pipewire-and-pulseaudio),
[WirePlumber Bluetooth monitoring](https://pipewire.pages.freedesktop.org/wireplumber/daemon/configuration/bluetooth.html).

## Amendment — macOS application microphone authorization (2026-11-01)

The exported application enables the audio-input signing entitlement and uses Godot's dedicated
microphone usage-description field, including local microphone checks in the purpose string.
CoreAudio stream creation alone was not a reliable permission prompt. Godot 4.7.2's macOS
`OS.request_permission` handles screen recording only. A small Objective-C bridge in the
GDExtension calls AVFoundation's authorization API on explicit **Test microphone** or
**Microphone on**, without constructing capture or changing consent. Apple's Clang compiles
the bridge and its completion block; no new dependency is added.

The UI displays pending, denied, restricted and missing-bundle-description guidance. It polls
the result without requesting again. Approval requires a fresh microphone action, so a late
response cannot unmute or start a test after the rider closes settings or leaves. Joining voice,
device enumeration and speaker tests do not request microphone access. Android keeps its
existing permission flow; the CLI keeps its existing native-stream behavior.

CI syntax-checks the Apple bridge before FFmpeg builds and inspects the exported app's actual
Info.plist and signed audio-input entitlement before uploading it. Simulated UI tests exercise
pending/denied/restricted results and late approval without microphone hardware. Real macOS
first-run approval and System Settings registration remain a native acceptance check.

References: [Apple microphone authorization](https://developer.apple.com/documentation/avfoundation/avcapturedevice/requestaccess(for:completionhandler:)),
[Apple audio-input entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.device.audio-input),
[Godot 4.7.2 macOS permission implementation](https://github.com/godotengine/godot/blob/4.7.2-stable/platform/macos/os_macos.mm).

## Amendment — observable peer-listening disconnects (2026-11-01, #428)

The earlier ownership fix does not establish that #428 is resolved. The reported reproduction
is a macOS organizer and a Windows or macOS joiner: **Join voice chat** disconnects the joiner
after roughly 2–5 seconds. The affected case has the joiner's microphone on: the organizer accepts
listening consent, then reports invalid voice traffic. Silent listening regressions alone do not
exercise this path. The sequence-timing correction below covers a reproduced cause of this error.

Lobby actors previously discarded their results and replaced a failed reader's original cause
with a generic protocol/lost error. Preserve reader results, voice validation errors, control
rate-limit errors and the two-second partial-frame/write deadlines. Emit locally constructed
operation/rider/cause diagnostics to tracing and the existing application Technical details
path, with a bounded failure event before membership departure or connection end. Ordinary
remote closes remain informational. Transport categories never copy peer-supplied close text.
After group end, remove remote roster entries and readiness while retaining the local rider.
With reconnection (#434, merged into this stack later), a stalled write closes the link for a
reconnection instead of failing with a write timeout. A network loss the group reconnects from
is logged with its transport cause, not reported, because the rider is shown as reconnecting.

Linux loopback tests cover normal listening for longer than the reported delay, reader-error
propagation, two-second partial-control timeout, offending-peer isolation and close-text privacy.
The macOS/Windows jobs also run the lobby protocol regressions. These tests do not substitute
for acceptance on the affected native devices; retain both sides' diagnostics if a failure recurs.

## Amendment — stable microphone sequence clock (2026-11-01, #428)

The future-sequence guard previously measured from the most recently accepted packet's arrival.
After loss or a change in delay, a late packet could shift that reference forward and make the
next fresh packet look impossibly early. A deterministic reproduction receives frame 0 at time 0,
frame 50 at 1.5 s after 500 ms of delay, then frame 75 at 1.501 s. This respects a 50 Hz source,
but the old guard rejected frame 75 as invalid and disconnected the sender.

Keep one sequence/time anchor per microphone activation, independently of latest arrivals and
playback flushes. The ten-frame jitter allowance and nominal 50 Hz sequence clock now cover the
whole activation, with 1% allowance for independent machine clocks over long conversations.
The separate ingress budget still enforces its original 50 Hz rate. A delayed packet cannot
restart the clock. Impossible jumps still fail without
changing ordering; repeated small jumps can no longer accumulate a new allowance each time.
Ingress budgets, sender/membership binding, epochs, fixed jitter buffers and privacy stay intact.
An impossible jump now reports its counter and allowed limit, never audio. No protocol change.

The peer's stream-accept branch also treated any result, including transport closure, as an
unexpected organizer stream. Propagate the transport error; continue to reject an actual extra
stream. This avoids a misleading invalid-control diagnostic after the organizer closes a link.
Both regressions fail with their old guards restored. A fifteen-second encrypted Opus test keeps
the organizer listening and the joiner transmitting through simulated devices. macOS/Windows CI
also runs the deterministic voice-clock regressions; physical-device acceptance remains manual.

## Amendment — recover from delayed media instead of ending the group (2026-11-01, #428, R179)

The next native report reached the original fatal token-bucket overflow with only a few spoken
words. The codec sends 50 constant-size packets per second while unmuted, including silence:
these budgets measure arrivals, not words or loudness. Network and scheduler delays can release
several valid packets together. Treating the eleventh packet in a burst as a connection failure
confused congestion with abuse. The aggregate forwarded-voice and state buckets had the same flaw.

Preserve all media rates, queue sizes, authentication, membership epochs and decoding bounds.
Exhausted delivery buckets now drop packets before parsing/forwarding; they refill normally and
accept fresh audio and positions without membership or consent changes. Count all raw datagrams
separately before media parsing, including malformed, duplicate, unknown and departed senders.
At 880 packets per second (twice eight riders' 50 Hz audio plus 5 Hz state), an overloaded window
drops excess immediately. Only three consecutive overloaded windows end the connection; a quiet
window resets that history. Thus one delayed batch cannot close a lobby, but sustained excess
cannot grow decoding, fan-out or queues. Reliable controls retain their existing hard budgets.

The audio worker now schedules its next tick from the live tick, without accumulating missed
transmissions, and consumes only the freshest valid frame from the four-frame capture queue
*(superseded by the device-clock amendment below)*.
An overrun inside encoding or playback also defers the next tick, so completing stalled work
cannot leave an expired deadline immediately behind the just-sent frame.
Suppressed feedback, superseded words, stale captures and old activations never replay later.

A deeper timing regression also delays the very first speech packet. Anchoring the sequence clock
to that first arrival rejects every following live packet once the initial delay disappears.
Use the speaker buffer's admission time as the earlier timing bound, retaining the first sequence
number, fixed anchor and clock-skew allowance. Actual work remains limited by the independent
ingress budgets. Huge future jumps and repeated staircase advances still fail validation without
changing ordering. There is no wire, service or dependency change.

Playback also resynchronizes when a newer source frame arrives behind an underrun cursor;
old reordered frames remain discarded. Evicting history from the fixed jitter window advances
its matching deadline, capped to the existing prebuffer, so a delayed consumer cannot skip that
history twice. Deterministic clock regressions reproduce both defects: with capture at 25 Hz,
the old receiver plays only 200 of 500 fresh frames; after a one-second listener stall it skips
all current buffered speech. Both now recover without catch-up transmission or larger buffers.
When delayed playback also outruns a slower producer's latest observed sequence, play the newest
unplayed buffered frame and resume the live deadline. Do not conceal source ticks that capture
never emitted. A further deterministic regression with both workers at 10 Hz previously played
zero of 100 valid frames; it now keeps current speech audible while still discarding history.

The forwarded-burst and delayed-first-arrival regressions fail with their original guards. Real
loopback QUIC tests cover both receivers, multiple forwarded speakers, state recovery, reliable
countdown after overload and isolation of persistent raw floods. Real Opus with simulated devices
covers current silence superseding queued speech, subsequent fresh words and every speaking rider's
audio worker stalling/resuming while group state continues. macOS and Windows CI run these
simulated conversations as well as lobby/voice validation. Run independent native conversation
tests serially: each already exercises up to eight simultaneous riders with real audio threads,
and multiple unrelated rooms must not compete for its wall-clock deadlines on a shared runner.
Speaker checks assert both audible pitches and gain, rather than scheduler-dependent callback
throughput. Physical-device acceptance stays manual.

## Amendment — device-clocked, continuous speech (2026-11-01, R189)

Native macOS-to-macOS tests after the burst fix found voice "extremely metallic", and half the
time voices did not come through. Three independent clocks (microphone, the worker's 20 ms timer
and the speaker) were reconciled by dropping or inserting whole 20 ms frames. The worker encoded
only the freshest of its queued captures, the speaker played a frame of silence whenever the timer
ran late, and listeners concealed the resulting gaps; Opus concealment, many times a second, is
what sounds robotic. Simulated sound cards running 0.1–0.2 % apart reproduce it: 15 silent frames
in 250 with timer-paced playback.

- **Playback is pulled by the speaker.** The output callback counts every frame it takes; the
  worker keeps two frames queued ahead of it, more for a device that plays larger blocks per
  callback, and wakes every 5 ms to top up. The output queue holds eight frames and discards
  frames older than 250 ms. Each mixed frame pulls exactly one 20 ms step from every speaker's
  jitter buffer. A starved callback plays silence only until the next frame arrives, not for a
  whole frame. Backends without a device clock (test doubles) keep the worker's 20 ms pacing.
- **Every fresh capture is sent, in order,** at the microphone's own rate. Frames older than
  120 ms (after a worker stall) are still discarded, and their sequence numbers are spent so
  listeners conceal the gap instead of splicing speech together.
- **Jitter buffers no longer run on wall-clock deadlines.** Playback starts with one spare
  frame, or after three frames for a single short word. A missing frame with later speech
  queued is rebuilt from the next packet's in-band recovery data (or concealed once), then
  playback continues with the speech that arrived. When nothing has arrived, up to three frames
  are concealed without skipping, so late speech still plays and the buffer grows to the
  network's jitter. A buffer whose smallest spare depth over one second exceeds one frame drops
  the excess, so differing sound-card rates neither drift behind nor skip words. The eight
  fixed slots, ingress budgets, sequence-clock validation, epochs and 250 ms idle restart stay.
- **Opus** runs at 32 kbit/s CBR (80-byte packets), complexity ten, voice signal, with in-band
  recovery data planned for 10 % loss; Opus 1.6 codes none at all at 5 % or less. A frame
  rebuilt from the next packet is about six times closer to the original than concealment in
  the codec regression. Speech may use 800 of each connection's 4,096 datagram-send bytes,
  eight 99-byte packets. Receivers already accepted any valid 20 ms packet up to 256 bytes.
- **Resampling** between 48 kHz and device rates uses a windowed-sinc kernel (Blackman,
  24 input samples per side, widened when decimating, cutoff at 90 % of the slower rate's
  Nyquist frequency) instead of nearest-sample output and two-point capture interpolation.
  Equal rates pass through. Nearest-sample output folded Opus' upper band back into the
  audible range on 16/24 kHz Bluetooth headsets.
- **Mixing** no longer divides by the number of active speakers, which changed one voice's level
  whenever another microphone opened. Gain applies to the sum; a soft limiter (linear to 0.5,
  a tanh knee up to 0.9) rounds off loud overlaps instead of clipping them.
- **Speaker echo prevention holds the microphone only for a friend's speech:** two consecutive
  playback frames that are voiced (normalized autocorrelation at least 0.45 at a 60–400 Hz
  pitch, measured at 8 kHz over 40 ms) and stand 10 dB above that playback's background noise
  and above −46 dBFS. The background is the fourth-quietest frame of each block of fifteen,
  the minimum over five blocks, ignoring digital silence from a friend whose own gate holds.
  The held microphone fades to silence within 5 ms and keeps sending, so the stream stays
  continuous; after the 300 ms tail it fades back in over 20 ms, as it does when opened.
  Previously any playback sample above 0.005 held it: one rider's fan or room noise held the
  other's microphone, whose speech then held the first in turn. A regression with a fan at one
  end and speech at the other heard the speaker in none of 101 frames.

There is still no adaptive acoustic echo cancellation or noise suppression. The operating
systems' voice-processing units (macOS VoiceProcessingIO, Windows communications mode, Android
VoiceCommunication) remain a possible follow-up needing a native backend beside CPAL. There is
no protocol, wire format or dependency change.

Unit regressions cover the resampler (pace, tone fidelity, rejection of Bluetooth-rate aliases),
recovery from the next packet, the pull jitter buffer (loss recovery, jitter without
concealment, ±0.5 % clock differences, slow senders, stalled listeners) and the gate (speech
and tail, a loud fan never holds, a whine learned as background). Real QUIC and Opus with
simulated sound cards, whose microphones and speakers run at their own rates, check continuous
speech (at most three gaps or quiet frames in 250) and that a rider by a fan never silences a
speaking one. Each fails with the previous timer pacing or gate respectively. Physical-device
acceptance stays manual.

## Amendment — who is speaking (2026-11-02, R190)

Riders could not tell who was talking. Each computer now works it out from the audio it
already handles, so nothing is added to the protocol or the voice header and there is no new
dependency:

- **Friends, as they play here.** Each friend's decoded frames, before the local volume, go
  through the speech test the echo gate uses: voiced sound with a pitch, above −46 dBFS and
  about 10 dB above that friend's own background, for two frames in a row. A per-friend
  detector learns each background separately, so a fan on one open microphone neither shows
  that rider speaking nor hides a quieter speaker. A friend muted locally, every friend while
  deafened, and a friend whose stream has ended play silence here and are not shown.
- **This rider, as they are sent.** The same test runs on each captured frame after the echo
  gate, so the rider is shown speaking only while friends can hear them.
- A rider stays shown for 500 ms after their last speech frame, so the indicator does not
  flicker between syllables. The voice worker publishes one bit per rider slot after every
  wake; the app copies it into the group snapshot on its 200 ms tick, and the UI shows a small
  speaker beside the name without rebuilding the participant rows.

Alternatives considered: a speaking flag in each voice packet would cost a protocol version
and trust the sender's own detector; a plain level threshold on received audio would show
riders by a fan as speaking all the time, the fault R189 removed from the echo gate.

Unit tests cover words with pauses (shown throughout, gone after the hold), single clicks and
loud fans (never shown). Real QUIC and Opus with simulated sound cards check that a talking
rider is shown to a friend and to themselves, a rider by a fan to nobody, and that a local
mute or a closed microphone clears the indicator.

Sources: [CPAL](https://github.com/RustAudio/cpal),
[Opus 1.6.1](https://opus-codec.org/downloads/),
[Opus packet format](https://www.rfc-editor.org/rfc/rfc6716.html).

## Amendment — guided joining and the open microphone's level (2026-11-02, R194)

Riders found the voice setup wordy and left **Prevent speaker echo** on with headphones, so
speech was held more than needed. Joining now goes through a window that asks every time and
remembers nothing: **Headphones** or **Speakers** sets the echo gate before voice opens, and
the rider checks their sound first.

- **Consent.** The window's **Microphone** starts **Off**. Switching it **On** asks for
  microphone permission in context and runs the capture-only check, nobody hearing it; **Join
  voice** then opens voice with transmission on. Choosing On is the explicit consent that the
  muted start stood for, so the first activation may now carry `mic`; joining with Off still
  opens no capture and asks for no permission.
- **The check's bound.** The window keeps the microphone check running while it is shown, so
  its fixed ten seconds became a bound of two minutes (`MICROPHONE_CHECK`), still ended by Off,
  closing the window, joining, a device change or leaving.
- **Level in voice.** **Audio devices…** in voice must not pause conversation for a meter.
  The voice worker therefore publishes the open microphone's peak level per wake, mapped like
  the check's, before the echo gate holds it; it is zero while the microphone is closed. Only
  that number reaches the snapshot, as with the check.
- **After a check.** A sound check during voice now brings voice back with the microphone as it
  was, as every other automatic restart does (R188), instead of always off: otherwise playing
  the test sound in voice would silently mute the rider.

- **Following the output.** A separate Headphones | Speakers switch would be forgotten when
  riders change headphones, leaving the echo gate wrong for the new device. CPAL already
  reroutes a stream built on the default output when the system's default moves and reports
  `DeviceChanged`; the backend now takes that as the output having moved, reads the new
  default's name and tells it, and tells the name of whatever output it opens or rebuilds.
  The app remembers which output the rider last answered for (the next one voice opens after
  joining or choosing devices, else the current one when they answer) and asks again,
  `ask_listening`, while voice plays on another. Only names cross into the snapshot.

Tests open a check with a one-second limit to prove it closes itself, join with the
microphone on and see capture, level and speech at once, and restore voice after checks with
the microphone on when it was on and off when the rider had muted it. A backend that moves its
output is asked about again until the rider answers, but not after choosing devices right
after being asked; an output rerouted by CPAL marks the move without invalidating the stream.
