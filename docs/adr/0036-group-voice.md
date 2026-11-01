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
  once; normal compressed packets are 60 bytes. The 19-byte voice header includes version,
  authenticated rider number, microphone activation, sequence and host-assigned membership epoch.
- The host stamps membership epochs from its current admitted roster. Reliable voice-roster
  controls follow joined/left controls in the same stream; receivers reject other epochs before
  changing packet ordering. Decoder state resets across epochs. An old connection cannot submit
  for or remove a reused slot. Voice can therefore operate safely before the countdown too.
- At most 256 compressed bytes per frame; per-speaker ingress burst ten, sustained 50 Hz,
  including malformed/duplicate packets. Forwarded ingress additionally has an aggregate
  80-packet burst and 400 Hz allowance, including unknown/departed speakers. Consent controls
  have a burst of eight and sustained one per second. Invalid/flooding peers are disconnected;
  invalid compressed audio silences only that speaker locally, preserving other conversation.
- Each speaker has eight fixed jitter slots, a 60 ms prebuffer, at most three concealment
  frames and no playback of speech idle for 250 ms. Delayed consumers skip history. Source
  sequences cannot jump arbitrarily ahead and poison ordering. Decode accepts only mono and
  exactly 960 samples per packet into a fixed output slice; malformed durations and stereo
  are rejected before decode. Native hardening remains enabled.
- Capture and output callback queues have four frames each; callbacks never perform network
  work or wait for it. Captures older than 60 ms and output older than 100 ms are discarded.
  At most 640 of each connection's 4,096 datagram-send bytes are available for speech,
  leaving space for disposable ride state. Congestion drops speech instead of blocking.
- Each speaker has a separate decoder. Mixing normalizes by speaker count, accepts finite
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

Sources: [CPAL](https://github.com/RustAudio/cpal),
[Opus 1.6.1](https://opus-codec.org/downloads/),
[Opus packet format](https://www.rfc-editor.org/rfc/rfc6716.html).
