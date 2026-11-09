# ADR 0037 — Group activities, assistance and app integration

- Status: accepted; delivery is incremental, as listed below
- Date: 2026-10-28
- Extends ADR 0035/0036 and requirements R164–R168

## Decision

Stack the app integration above the secure networking and voice PRs (#398–#401, #403).
One group holds at most eight riders. The host creates a lobby before selecting a 3D
course, a structured workout, or a workout on a course. Video distribution, constant-power
and heart-rate modes and FTP tests are outside this delivery. There is no new account,
directory, rendezvous server or relay. Use the existing private invite/reply exchange.

Admission authenticates the lobby identity rather than the mutable activity. The app
integration will introduce a protocol version and update CLI callers together. Existing
CLI controls and their course-bound admission remain unchanged in the foundation step.

### Content identity and readiness

Each activity selection has a strictly increasing revision, printable name, optional
course/workout identities, and independent Drafting and Keep together settings. At least
one content identity is required. Each identity contains its kind, encoded length and
SHA-256 of the complete file, independent of the display name. Changing selection before
countdown clears every readiness acknowledgment. Membership generations prevent a departed
rider's preparation from readying a replacement in the same roster slot. Countdown freezes
the selection only when every admitted participant, including the host, is ready.

Course transfers carry `.oqc` inputs, not built worlds or video. Structured workout transfer
uses a bounded binary wire representation preserving FTP shares and absolute watt targets,
ramps, free steps, cadence and cues. It is independent of the rider's FTP; no watts-to-FTP
conversion is performed. The `.oqw` basename identifies the normalized wire artifact; app
integration must register its decoder in the workout library before offering these files
as library workouts. It must not reinterpret them as ZWO. Author descriptions and cues
stay intact within explicit text limits.

The foundation limits each selection payload to 256 bytes, course archives to 512 MiB and
workouts to 1 MiB. Workouts have at most 4,096 steps and cues each, last at most 48 hours,
and contain finite targets within the existing network physical ranges. Relative power is
limited to five times FTP. Targets retain their original kind and each rider applies their
own FTP in the session layer.

### Transfer and publication boundary

Use an existing pinned QUIC connection and reliable stream, separately from state and
speech datagrams. A content request names only an advertised kind/size/hash, never a path.
The serving app must restrict requests to the current selection, with one transfer task
per authenticated peer and ownership/cancellation in the lobby actor. No detached worker
or unbounded peer-driven task creation is allowed. The foundation provides stream helpers;
existing CLI lobby actors still refuse unexpected streams until this integration lands.

Transfers use a fixed 16 KiB chunk, one coalesced progress value, a 30-second idle deadline
and a 15-minute whole-transfer budget. Connection admission retains its separate one-minute
deadline. Receiving checks the selection identity before creating a temporary file, exact
length, clean FIN and the streaming SHA-256. Every temporary file has a random local name
and is removed when its owner is dropped. Application validation runs on a blocking worker
whose result owns cleanup even if its caller cancels.

Hash verification alone does not make a file safe to import. The unvalidated receipt cannot
publish: it must pass the course or workout validator first. Publication uses a hash-derived
basename and atomic non-overwriting hard link, rehashes after validation, and reuses an
existing file only after checking its complete content. Stage downloads on the library's
file system; cross-device publication fails rather than silently losing atomicity.

The course validator inspects the bounded classic ZIP trailer before the ZIP reader can
allocate its directory: at most 16,384 entries and a 16 MiB directory. It rejects multi-disk
and ZIP64 archives. Validate every entry's path, file type, uniqueness, decompressed length
and CRC without writing any shared cache data. Cap aggregate expansion at 2 GiB, ordinary
entries at 256 MiB, the manifest at 512 KiB, GPX at 32 MiB and map pictures at 8 MiB. Refuse
referenced video, unknown format versions and non-finite metadata. Route parsing and offline
world preparation still have to succeed before the app reports Ready; this archive scan
does not claim a GPX is rideable or that its bundled data is complete.

### Assistance and recording

Drafting and Keep together are independent host-owned toggles, both on by default and
locked at countdown. Their amounts are local calculations from validated member state,
never peer-supplied forces or trainer commands. Drafting reduces aerodynamic drag from nearby
group riders, with bounded non-stacking shelter and consistent simulation-mode resistance.
Ghosts do not supply drafting. Keep together smoothly assists riders behind the host and
restrains riders ahead while pedaling, without changing measured telemetry or ERG targets.
Stale/missing sources fade assistance out. No assistance runs during local pause or without
valid pedaling input. Assisted course/climb times set no personal speed records; genuine
measured power records and training metrics remain eligible. Record assistance in summaries.

This separates the two behaviors described in [ROUVY's group ride guide](https://support.rouvy.com/hc/en-us/articles/360018700677-ROUVY-Group-Rides)
and the organizer-chosen [Zwift Keep Everyone Together option](https://support.zwift.com/en_us/meetups-HJP7iUd4r).
Their proprietary algorithms are not an implementation specification. Host-owned settings
with individually calculated assistance are the chosen product behavior.

### Ride lifecycle and authority

Late joiners download and prepare independently, then explicitly enter near the host at the
current workout step. Keep course position, group workout time and personal elapsed time/
recorded distance separate: entering halfway creates no fictitious earlier riding or records.
The host coordinates workout pause/resume and skips through ordered authenticated controls;
intensity stays personal. Local safety pause is always available, with resume aligning to
the current group workout step.

Planned handover requires a connected, prepared successor's explicit acceptance. The live
host coordinates fresh rider-bound credentials and the successor's certificate pin; explain
the necessary address sharing. Establish replacement links before committing a new authority
epoch, preserve stable rider IDs and workout time, reject old authority commands, invalidate
old invitations, and transition the keep-together reference smoothly. Bound transient
connections and handover time. Preparation failure leaves the old host in charge. Unexpected
host loss ends group networking and voice, fades assistance and preserves each local ride;
automatic crash recovery is not part of this decision.

## Delivery and validation

### Guided connection consent — 2026-10-31

Connection options move from a separate settings button into create/join popups. The rider
chooses Separate homes, Same home Wi-Fi, or Some together, some elsewhere, with short
descriptions. Optional public address lookup stays available under More connection options.
Same-home mode always clears that server list; separate-home mode never enables LAN access.
These mappings live in Rust and retain the existing transport limits and address policy.

Joining inspects the bounded invitation offline first, showing only the validated organizer
name and whether home-network permission is needed. Inspection permits parsing LAN fields
without granting network access; the selected policy is checked again synchronously before
starting the existing group owner, and again by transport. No new service or protocol version
is introduced. Back preserves the invitation; cancellation starts no discovery, probes or
listeners. Invitations that include local destinations still require explicit home-network
permission rather than silently widening the rider's separate-home choice.
The popup uses the existing touch-scroll helper and native release actions for whole-card
selection, so finger swipes scroll without changing connection permissions. Gesture tests
exercise the popup's actual cards as well as tap selection.

Amended 2026-11-03 (R197, ADR 0035 "The organizer's connection choice"): only the organizer
sees the popup. The invitation carries the choice and joiners follow it without a popup;
the joining page names it instead.

The first step implements activity/readiness types, normalized workout wire encoding,
streaming content helpers and a course archive validator. Tests exercise the existing real
pinned loopback transport, content corruption/truncation/trailing data, atomic publication,
cancellation cleanup, stale revisions/memberships and differing FTPs. No UI, existing lobby
actor behavior, trainer control, network endpoints or rider workflow changes in this step.
Consequently it needs no usage-wiki changes or new screenshots.

Subsequent PRs integrate admission/content into the lobby actors and app, expose translated
controls/voice and peer rendering, implement assistance physics and recording eligibility,
then late joining/workout synchronization and planned handover. Each updates its affected
usage guides and screenshots. Those roadmap items stay unchecked until implemented and
validated, including fake-trainer/FIT isolation tests. Native audio, trainer feel, cross-home
connectivity and frame rate with seven remote avatars require separate manual acceptance.

## App-lobby integration — 2026-10-28

The next stacked step connects these primitives to the lobby actors, `orqwa-app` and Godot.
Protocol/invitation version 3 and ALPN `orqwa-group/3` require matching builds and fresh codes;
the CLI is rebuilt against the same protocol while preserving its course-bound workflow.
`RouteHash::LOBBY` reserves the zero value for an app lobby: the independently generated
ride ID and certificate still bind admission. Content hashes are checked after admission.

Selected content is held in a watch channel. Each peer may have one owned transfer worker,
and since 2026-11-02 a second one for the organizer's preview (see below), each
cancelled on changes to what it serves or departure. A completed worker is reaped before
accepting the next file request. Only the selected course/workout identity is served. Ready
controls and file requests have independent ingress budgets; file requests allow a burst
of eight and sustained one per second, charged before spawning a transfer task. Ready
controls bind to the authenticated membership and current revision; old acknowledgments
are ignored. App host controls require all readiness flags before countdown. App joiners
bound forwarded reliable controls to 64 per second; the CLI retains its 64-control lifetime
limit. Existing fixed queues and voice/state budgets remain in force.

The app exposes a Group rides tab and compact in-ride panel, byte progress/retry, device
readiness, shared starts and voice controls. The background task owns networking, download
jobs and voice startup. Leaving aborts it, closes its owners and cancels local preparation,
without ending the local ride. Received courses load offline, without replacing inputs with
locally installed map detail or refreshing old previews online. Decoded workouts are retained
in memory before countdown so ride startup does no workout file I/O. Normalized `.oqw`
files in `shared-workouts/` appear in the app's workout library; editing saves a new local
workout rather than overwriting the downloaded original. Inactive tabs do not stop the group.

Microphone off, deafen and voice off close the transmission gate synchronously in the app
command, before queueing device cleanup. A shared generation and short control mutex prevent
older queued unmute commands from reopening capture. Volume changes are separate and never
reactivate a microphone. Pending native audio startup has an explicit UI state and stale
startup results are discarded. Device failures stop local voice rather than group riding.

Initial group UI delivery keeps drafting and keep-together off and exposes no misleading
assistance controls. Peer avatars/map markers, assistance physics, shared workout control
after start, late ride admission and planned handover stay unchecked in the roadmap. The
current actor still seals admission at countdown and host loss ends networking/voice.
Native microphone permissions, audio device/hotplug acceptance and cross-home-network
connectivity remain manual checks; local integration tests do not claim those results.

### Late entry — 2026-10-29

App lobbies retain explicit admission after countdown. The CLI continues sealing admissions.
Protocol/invitation version 4 and ALPN `orqwa-group/4` add bounded Running, Enter and Entry
controls; builds must match and exchange fresh codes. The eight-rider cap, one-use admission,
fixed task/queue limits, selected-content serving and independent control/state/voice budgets
remain in force. Activity and assistance selection stay frozen after countdown.

New members receive the frozen selection and group clock without a local Start event. They
download and build independently, then explicitly request entry after preparation and trainer
connection. The host checks the current connection membership, selection revision, readiness
and a recent active organizer snapshot. An unavailable snapshot yields a retryable response;
it does not pause existing riders or end the connection. A successful connection enters once.
Late members cannot publish ride state before entry; existing clients accept bounded roster
changes during riding. Datagram/control reordering before clock receipt does not disconnect
a preparing app joiner. Original countdown participants still start normally.

The session keeps absolute course position separately from actual ridden distance and personal
elapsed time. Entry starts at rest, five metres behind the organizer, seeks only the structured
workout plan and creates no samples or previous telemetry. The HUD, FIT and summary count only
post-entry riding. A partial course and climbs whose starts were skipped produce no speed
records; later complete unassisted climbs and genuine power/training retain eligibility.
Milestone and climb cursors skip unridden sections. Workout targets use local FTP and intensity.
Shared pause/resume/skips and planned authority handover remain separate roadmap steps.

### Rider-facing setup refinement — 2026-10-28

The tab starts with two actions: create a ride or join one. Invitation exchange is guided
with copy/paste dialogs; connection policy and audio devices live in separate settings.
Course and workout choices preview immediately using the existing map, elevation and power
charts, and require explicit confirmation before sharing; since 2026-11-02 friends see that
preview too (see below). Readiness names the rider's next
action, including trainer connection. These are presentation changes over the existing
admission and transfer protocol.

Audio device selection remains available during voice. The app synchronously disables
transmission and advances its activation generation before queueing the switch. The owner
cancels pending startup, reconnects opted-in voice and retains playback gain, deafen and
per-speaker mutes. The microphone comes back as it was since 2026-11-01, see below.
Selected device IDs and playback gain in the snapshot keep lobby and in-ride controls consistent.
Device enumeration has its own single-job budget, independent of pending voice startup, so
opening audio settings cannot consume or suppress the rider's voice activation request.

### Remote motion and assistance delivery — 2026-10-28

The app now renders up to seven remote riders beside the local rider and ghosts, with
matching map/profile markers and HUD distance gaps. Eight fixed histories are bound to
local membership generations. Initially, positions interpolated 100 ms behind packet arrival
(superseded by Shared rider playback below), extrapolated for at most 300 ms, held briefly,
then disappeared after two seconds. Reordered,
invalid and departed state cannot extend a previous membership's motion. Heart rate is
absent from remote motion and never leaves the local activity.

Drafting and Keep together now default on. The organizer can change either before countdown;
changing rules advances selection revision and clears readiness, retaining verified content
and completed preparation. Countdown locks the rules in both the lobby authority and its
app command owner; a queued setting change after Start is rejected without stopping the ride.
Drafting uses the strongest shelter
from riders within 15 m ahead, quadratically tapering from 30% drag reduction; effects never
stack. Keep together uses a critically damped spring around the organizer, with a 5 m
position dead zone and a gradient offset bounded to ±2 percentage points. Relative speed
damps the catch-up before overshoot. The organizer receives no elastic band. Both effects
require local pedaling, stop during local pause and fade between 500 ms and two seconds
without fresh source state. Motion and simulation-mode resistance use the same modified
drag/gradient before personal difficulty/gearing; ERG targets and measurements are unchanged.

Rides started with either setting on retain an additive `assisted` metadata flag, even after
leaving the group. Summaries/history identify them, and course/climb times and personal-best
speed ghosts exclude them. Measured-power records, FIT power/cadence/heart rate and training
analysis retain genuine local readings. Older metadata defaults to unassisted. Loopback
tests cover rule replication, readiness reset, local/remote power separation and dropout;
session tests cover simulation/ERG behavior, and saved FIT tests cover recording isolation.
Seven-rider review images use presentation fixtures, separately from transport tests. Native
trainer feel, audio permissions, cross-home connectivity and GPU frame rate remain manual.

### Group-based Keep together — 2026-11-04 (#504, R211)

This supersedes the organizer-based reference above. Every client computes a reference
from live local motion and current validated peer motion, excluding its own accepted echo.
Organizer identity is never an input. A rider moving at more than 0.5 m/s participates,
including while coasting; stationary, paused and finished riders do not anchor the group.
Disconnected/away memberships and reconnecting clients supply no peer reference. Arrival
and source age retain the two-second cutoff; weight is one through 800 ms (R215), then
falls linearly to zero at two seconds. Local motion has weight one.

For distance and speed separately, find the centre `c` where
`sum(w * (x - c) / hypot(x - c, s)) = 0`, using scales `s` of 12.5 m and 0.5 m/s.
Each rider's influence is bounded by its freshness weight and varies continuously with
position, speed and weight. Two fresh moving riders share their midpoint. A distant sprinter
cannot pull two fresh coincident riders' reference more than `s / sqrt(3)` away from them,
retaining the existing modest outlier-assistance bound. The strictly decreasing balance has
one root between the minimum and maximum values; at most 64 bisections bound per-frame work.
At least one eligible peer is required. Multiply the resulting band by the freshest eligible
peer's weight. This prevents an isolated stale peer from retaining full assistance through
normalized weights. Stopped local riders may receive help when they resume pedaling, but
supply no stationary anchor.

This continuous calculation replaces the initial median-centred clamp (R214). At a 75 m
two-rider gap, reducing one weight just below one switched that median from the midpoint to
the local rider. The requested band jumped as source age crossed 500 ms, even without a new
packet or movement. The applied rate limit softened each jump but left repeated fluctuations.
The replacement preserves the equal-weight midpoint without that weight-boundary jump and
keeps stale/outlier influence bounded.

The existing 5 m position dead zone, relative-speed damping and ±2 percentage-point limit
remain. The applied band changes by at most one gradient percentage point per second,
including after outlier, membership and authority changes. With no eligible peers it returns
to zero within two seconds; a stale peer's target is already zero at its cutoff. Stopping
pedaling, pausing, finishing, disabling Keep together or leaving clears the band immediately.
Drafting retains its authority-transition blend while a source supplies shelter. R215 below
refines the drafting curve and filters applied effects. The band uses its own rate limit
across authority changes rather than changing reference with the organizer.
Feedback reads the final applied effect. Measurements, ERG targets and assisted-record
eligibility retain the rules above. No wire-format or protocol-version change is needed.

Tests cover role/slot permutations, a two-rider midpoint, an unaffected pack centre,
bounded sprint influence, stopped/coasting/stale riders, transitions and closed-loop motion.
Continuity regressions sweep sample age through 500 ms and the stale cutoff at steady
two-rider gaps/speeds, vary a four-rider split pack's weight and replay 5 Hz updates delayed
by 400 ms while buffered playback retains an exact 75 m gap.
Native trainer feel remains a manual acceptance check.

### Live assistance timing and drafting — 2026-11-04 (R215)

The 300 ms missing-data prediction was shared by buffered playback and live assistance.
That is shorter than ordinary publication, host fan-out and receiver coalescing together.
The live peer therefore stopped moving between healthy packets and jumped forward on the
next one. Fading from 500 ms also treated ordinary delivery as a disappearing rider. A ride
trace on the initial R214 implementation still varied applied support from 51% to 65% at
a steady 75 m gap, and drafting from zero to 93% at a steady 3 m gap.

Live prediction now covers the existing 800 ms delivery budget; older positions hold while
their influence fades to zero at two seconds. The same budget supplies the freshness grace.
The app calculates assistance before its next physics step, so it queries peers at the last
published live local sample's time when that sample still matches local position/speed.
Freshness uses the current clock independently, preventing an earlier query from reviving
old data. New/jumped positions fall back until their new sample is published. The local
accepted echo is still excluded. None of this changes buffered playback: its 800 ms delay,
300 ms missing-data prediction, interpolation, source histories and shared gap displays stay
as before.

Drafting formerly reached maximum shelter at exactly zero gap and disappeared immediately
when the peer moved infinitesimally behind. It now supplies no shelter through a 1 m forward
clearance. From 1 to 3 m it uses `smoothstep((gap - 1) / 2)`; from 3 to 15 m it uses
`((15 - gap) / 12)^2`. Both meet at full shelter, capped at 30% aerodynamic reduction; other
gaps supply none. The strongest eligible peer wins, without stacking.

The actual applied drag and nonzero band targets follow a first-order response with a 0.5 s
time constant, using `1 - exp(-dt / 0.5)` independently of frame rate. Keep together also
retains its one-percentage-point-per-second slew limit; a zero target follows that limit
directly so loss fades fully within two seconds. Drafting clears immediately without an
eligible source, including while abreast, behind, expired, or during handover. Local safety
and disabled rules clear immediately. Feedback reads these same applied effects; this is
not display-only smoothing. Telemetry, recording and ERG targets remain live and unmodified.

Regressions include wire-rounded ride traces at steady 0, 3 and 75 m gaps through 5 Hz
publication, 90–510 ms delivery jitter, reordered packets and isolated loss. They check zero
abreast drafting, effective shelter/support and less than two percentage points of applied
variation after settling. Slow-frame sample-time alignment and stale expiry are separate
checks. Existing two-screen motion/overtake tests continue to protect playback.

### Position-constrained assistance and real-time motion — 2026-11-08 (R233)

The original critically damped band could reverse its position effect: a fast rider still
behind the middle received braking, and the rider ahead support. R209 documented that
behavior and a regression explicitly expected it. R233 supersedes it: the group middle
determines the allowed direction, while relative speed may only reduce the restoring force
to zero. The robust reference, freshness weights, five-metre middle dead zone and ±2
gradient-percentage-point bound remain. The organizer has no special role.

Each request retains the reference's position constraint. Before filtering, cancel any old
band that now points away from the middle, including when a corrected position crosses it.
New effects still ramp under the existing response and slew limit. With no eligible peer,
retain the existing bounded fade. Direction cancellation is an explicit exception to the
ordinary slew limit; retaining wrong-direction assistance to meet that limit is forbidden.
The session invalidates its cached trainer command when a band clears or reverses, or
drafting clears, so its one-second throttle and 0.1-point gradient threshold cannot leave
an obsolete effect on the trainer. ERG targets remain unchanged.

Investigation also reproduced sustained front support and back braking while the back lost
ground with a slow-rendering client. Godot 4.7.2 caps process delta during slow frames, but
group state is timestamped with real monotonic elapsed time. Advancing physics with the
capped delta made reported virtual speed exceed actual progress per real second. The
extension now supplies real monotonic frame duration to the Rust app, matching the group
clock; explicit accelerated simulation remains separate. A five-fps Godot regression checks
that riding time follows real elapsed time, alongside the direction and command regressions.
No wire-format change is needed, and buffered presentation remains unchanged.

### Android publication and diagnostics repair — 2026-10-29

Android's shared-storage filesystem rejects hard links even in the app-owned media folder.
After complete validation and rehashing, a denied/unsupported link now falls back on Android
and Linux to atomic `renameat2(RENAME_NOREPLACE)`, which [Android's FUSE service accepts](https://android.googlesource.com/platform/packages/providers/MediaProvider/+/refs/heads/master/jni/FuseDaemon.cpp).
Existing files and symlinks are never
replaced; matching content is still reverified and reused, and cross-filesystem publication
still fails. API 29 uses the stable kernel syscall because Bionic's wrapper starts at API 30.
No copy into an exposed partial library file, overwrite rename or permission broadening occurs.

The Android plugin supplies a retained application context to CPAL's existing `ndk-context`
before Godot opens the extension. CPAL documents that embedded JNI hosts must initialize this
context [when embedded through JNI](https://github.com/RustAudio/cpal#compiling-for-android);
otherwise device setup can panic before readiness. Voice setup remains microphone-off.
Group errors use ADR 0017's common classified/translated failure history with original causes,
Technical details and Dismiss; independent activity retry state survives later voice failures.

### Shared workout timing — 2026-10-31

Protocol/invitation version 5 and ALPN `orqwa-group/5` add organizer-owned workout samples.
Each includes the frozen activity revision, increasing sequence, bounded workout position,
paused flag and sample position on the original group clock. Mapping that sample position
onto the already established local group start accounts for reliable delivery delays instead
of starting a stale clock at receipt. Values and future times are validated before use.

The host owns a monotonic timeline independent of app frame steps, local safety pauses and
personal recording. Pause/resume and forward skips enter its existing bounded command queue;
peer attempts to supply organizer time disconnect only that peer. Reliable snapshots run at
five Hz and coalesce into one latest app value, so a slow consumer accumulates no clock history.
Existing control ingress/frame limits and state/voice/content budgets remain intact. Late
entry uses current authoritative workout time rather than a previous local progress report.

Only structured-plan time is aligned in the session. Shared pause holds the step and asks
for zero resistance while physical simulation and personal recording can continue. FTP,
intensity, measured telemetry, course position and personal elapsed time are untouched.
Local safety pause still stops that rider and frees the trainer; resume aligns to the current
group step without manufacturing samples or distance. Shared step boundaries, skips and
pause changes promptly update trainer control while retaining normal control throttling.

All app skip inputs pass through organizer authority, and the engine refuses local skips
while externally timed. Frozen shared plans cannot be replaced in the app or ride settings.
Leaving or losing networking releases external timing once and continues the local workout
from its reached step, including when the group had paused. Automatic recovery after unexpected
host loss is still excluded.

### Planned organizer handover — 2026-10-31

Protocol/invitation version 6 and ALPN `orqwa-group/6` retain a logical root group identity,
stable rider slots and a committed authority epoch independently of fresh transport identities.
Welcome includes the actual organizer slot, root and epoch, also for later ordinary joiners.
Activity readiness and voice forwarding use this explicit organizer rather than assuming slot 0.

The current organizer offers duties to one prepared member. That rider explicitly accepts
before opening a fresh pinned listener under their existing discovery/address settings.
Authenticated old links distribute a bounded public plan; admission replies travel privately
to the successor through the predecessor. Each rider validates their own address policy and
the frozen group, complete membership/metadata, selection and fresh pin before opening links.
Replacement admission binds a transport-authenticated credential (since protocol 7, the rider's
TLS client certificate; ADR 0035) to the reserved original rider
slot, preventing a claimed rider number or matching display name from taking another slot.

A fresh random proof stays with the old organizer. Its SHA-256 commitment binds the root,
proposal epoch, old/new organizer, complete roster/selection, pinned invitation and original
group/workout clock. All replacement links prepare with state and voice disabled. The source
reveals the proof on its new pinned link only after each member reports readiness and the
successor confirms the complete server roster. The successor validates the commitment and
full roster before activating; every peer validates the proof before replacing its old owner.
Cancelled proposals consume epochs too, so delayed decisions and asynchronous replies cannot
authorize a later attempt. Credentials, plans and proofs are redacted from Debug output.

Consent lasts 30 seconds; accepted preparation uses a common deadline of 60 seconds on the
existing group clock. Each app owns one cancellable job and at most an old and replacement
session (14 peer links for eight riders). Existing frame, ingress, voice and transfer budgets
remain in force. Membership changes or precommit failure discard replacement owners and retain
the old authority. Once the proof is revealed, rollback cannot restore the old authority.
Old-link loss allows at most five seconds, bounded by the original deadline, to finish an
already prepared valid commit; otherwise group networking ends and the local ride continues.
Prepared peers also defer old membership departures briefly: the successor may close its old
link before their new proof arrives. The old source cancels precommit departures; a valid
commit discards deferred old departures, while cancellation applies them to the old roster.
This grace completes planned consent and does not elect an organizer after a crash.

The switch keeps local recording, course position, assistance eligibility, workout/intensity,
membership generations and the original monotonic start. It clears old peer motion history
and blends assistance to the new reference over two seconds. Voice closes the old transmission
gate synchronously, replaces the port and reconnects previously opted-in listening with a fresh
generation and the microphone as it was (2026-11-01, below); device choices, gain, deafen and
local mutes remain personal.
The new host shares its already verified content and publishes a fresh invitation. No extra
service, endpoint, package or dependency version is introduced.

### Fallback organizer — 2026-11-01

Riders reach only the organizer, so once it is lost nobody can reach anyone else (#426). A
fallback is therefore prepared **while the organizer is there**: a standby is the planned
handover's preparation (offer, consent, fresh listener, plan, private replies, prepared links)
kept for the ride instead of committed. `Offer` carries a `standby` flag. Once every standby
link is ready the organizer sends the fallback the proof privately (`Proof`) and announces
`Standby`; it never commits. Prepared standby links stay idle, without state or voice, and
reconnect like any link (R186); the fallback's prepared lobby holds their seats too.

While riding, the organizer offers the standby to the first prepared, connected rider in
joining order who has not declined. That rider's app declines by itself when its own discovery
suggests friends could not reach it (R176: strict, IPv6-only or unknown), accepts by itself a
standby renewed after it consented once in this group, and otherwise asks over the ride. A
refusal, a consent timeout or the fallback's own failure marks that rider declined for this
organizer; another rider's failed standby link pauses offers for 30 seconds instead. A change of riders invalidates the
plan's bindings, so the organizer cancels and renews the standby. A planned change replaces the
standby, and each new organizer, after a planned change or a takeover, renews it.

Each rider counts the organizer lost from 30 seconds after its link began reconnecting, or at
once after its link ended; the organizer counts itself lost when every rider is away. Riders
report `Lost`, and `Found` again, over their standby links. The fallback takes over when it has
lost the organizer itself and no rider connected to it still reaches the organizer. Its own
`take_over` activates the prepared lobby with the proof it holds and continues the workout
from the last clock it heard. The commit goes to every connected standby link, and to each
held seat's rider on return in the rejoin prelude, which is how the old organizer rejoins as a
rider. A standby activates without riders who left it, the old organizer among them, but never
with anyone unplanned. Riders accept the takeover only once they lost the organizer
themselves, so a misbehaving fallback can gather no rider who still reaches the organizer.

The voice roster check required an epoch for the first slot, assuming it is the organizer's.
After a handover that rider may be gone, which would have dropped every listener. It now
requires the organizer's slot. Protocol/invitation version 12 and ALPN `orqwa-group/12`
require matching builds and fresh codes.

### Microphone kept across voice restarts — 2026-11-01

Every automatic voice restart now keeps the microphone as it was, on if it was on and off if it
was off: a device change, a planned organizer handover and a fallback's takeover. Each still
closes transmission synchronously before the old owner goes, and records first whether the
microphone was transmitting. The restarted chat re-applies it once its devices are open, unless
the rider is deafened. A first start of voice still begins muted, and Voice off, Mute all sound
and device failures still close the microphone. While a link reconnects (R186) the microphone
stays as it was too, sending nothing until the new link carries it. This supersedes the earlier
rule that a microphone always needs a fresh action after a restart, at the rider's request. App
tests open voice on silent audio, so these restarts are tested without audio hardware.

### Courses riders already have — 2026-11-01

A joiner found an existing course only under its hash-derived name (#435). A `.oqc` sent
beforehand and imported under the rider's own name was downloaded again, and so was any copy
the app had changed since: renaming, importing, a new map picture, countries or map detail
rewrite the archive. Worse, a received `<hash>.oqc` changed since then made publication fail,
so the rider could not prepare that course again.

Each course selection now also carries a **course key**: SHA-256 of key material the storage
layer reads from the archive without unpacking it. The material is the manifest without its
`name` and `countries`, serialized with sorted keys, followed by every other entry except
the map picture (`map.png`, legacy `preview.png`) by path, stored CRC-32 and uncompressed
size, sorted by path and length-prefixed. Renaming, importing under another name, a new
picture or countries keep the key; other route, data or manifest changes, map detail among
them, change it. Before downloading, a joiner keys every `.oqc` in its library, which costs a
zip directory and manifest per course, so no index or cache is kept, and rides the first
match without downloading. Courses open in shared mode in a group, so the match is not
changed while riding.

Transfers keep checking exact bytes. A workout response must name the requested identity. A
course response may announce the sender's own copy instead, renamed or with another picture:
the receiver checks its kind and size limit before creating a temporary file, its streaming
SHA-256 against the announced identity, then the course validator and the course key against
the selection's before publication. The serving lobby sends its local course as it is now. So
a successor riding their renamed copy after a handover still serves late joiners, and so does
a host whose file changed after selecting it.

CRC-32 is not collision resistant, so a hostile organizer could make another course match a
key. That gains nothing over selecting that course: a match only picks a course the joiner
already has, and a forged download still passes the shared validator. It does not reach
riders who are already prepared. Exact SHA-256 of whole files was rejected because the app's
own rewrites change bytes, and hashing decompressed contents because it costs seconds per
large course.

Publication still never replaces a file. When a hash-derived name holds other bytes, the
content takes the next free `-2` … `-8` name beside it. It reuses a name that already holds
identical bytes, as before. Selection encoding version 2 and protocol/invitation version 10
with ALPN `orqwa-group/10` require matching builds and fresh codes.

### Previews before sharing — 2026-11-02

Friends saw nothing of a course until the organizer shared it, and then started downloading
and building it (#418), so a group could not talk over what to ride. Every rider now sees the
preview the organizer's picker shows, as the organizer browses: the course's map picture,
elevation profile and figures, and the workout's power chart by the rider's own FTP, with its
duration and description. Choosing nothing, sharing the activity or going back to what is
shared puts the preview away; so does the countdown.

A preview is its own bounded encoding (`orqwa_net::preview`): a printable name, finite
figures, at most 256 track and 256 profile points, an optional PNG of at most 512 KiB and an
optional workout in the shared encoding without its cues, 1 MiB in all. It is never written to
disk, published or ridden, and carries no route, map data or path. A course with a larger
picture is previewed without it. The organizer's app builds it on a blocking worker from the
course's manifest and map picture, halving pictures wider than 640 pixels with a box filter
through the workspace's existing `png` crate: the link's flow-control windows make every
kilobyte take time, and half the size arrives about three times sooner.

The organizer's lobby holds the latest preview in memory and names it in a `Preview` control
of its content identity, a new `ContentKind::Preview`, at most every 500 ms however fast the
organizer browses; newcomers learn the current one with the selection. Riders fetch it with
the existing request header on a separate reliable stream into memory, checking the
announced identity, exact length, clean end and SHA-256. Each rider has at most one preview
request at a time and asks at most every two seconds, for the latest named, within the same
request budget as downloads (a burst of eight, one a second). The organizer serves a preview
only while it is the current one, cancelling the transfer when it changes, and a selection
only while it is selected, as before. Both may run beside each other: each peer may now have
two transfer workers, and QUIC allows three bidirectional streams, the third for previews;
a peer opening more is disconnected as before. Downloads to disk refuse the preview kind,
and selections cannot name it. A malformed or unfetchable preview is only not shown.

Previews add a fuzz target with round-trip properties and seeds. Protocol/invitation
version 13 and ALPN `orqwa-group/13` require matching builds and fresh codes.

### Shared rider playback — 2026-11-02

The arrival-time smoother treated each packet's journey as riding time. Two retained
samples and a 100 ms delay could not cover even the 200 ms publication interval, much less
host fan-out and receiver coalescing. A delayed packet changed interpolation speed or
replaced an extrapolated position with an older one. Each computer also compared these
delayed friends against its live local rider, so two friends could both see themselves ahead.

App lobbies now return every host-accepted state to every rider, including its sender.
The organizer consumes the same validated, wire-rounded state as its friends. State events
include the local echo only for app lobbies; legacy CLI behavior stays unchanged. The same
eight slots, 5 Hz limits and bounded mailboxes apply, with at most eight forwarded streams
per connection as the existing receive budget already permits. Self-echoed state remains
authenticated by the organizer's link; peers still cannot assert another sender. Voice
continues to refuse self packets. Protocol/invitation version 14 and ALPN `orqwa-group/14`
prevent older clients interpreting the new echo as a protocol violation.

Each membership keeps sixteen samples indexed by their shared-clock source timestamps.
All avatars, including the local rider, play 800 ms behind the shared clock: three 200 ms
publication/coalescing periods plus 200 ms for ordinary network jitter. Arrival times only
measure freshness. A monotone cubic uses endpoint speeds without overshooting a stopped
rider; power and cadence interpolate between the same samples. Missing samples can predict
at most 300 ms before holding. The greater of source and arrival age retires state after two
seconds, so a freshly delivered old packet cannot revive a stale rider. Reentry, backward
course jumps, authority changes and reused memberships discard incompatible history.

The app supplies one view with the local presentation distance and all peers. The road,
map/profile markers and both gaps panels use it; the binding's `position_m` is distinct from
live `distance_m`. Before a local accepted echo, or while reconnecting, peers are omitted
rather than compared against a different timeline. Physics, HUD values, trainer controls,
workouts and recording retain live local state. Assistance predicts on the current clock
and excludes the local echo. Outgoing state is stamped after each frame's physics step.

Deterministic frame traces cover differing local clock epochs, jitter up to 510 ms, reordered
delivery, bursts, isolated loss, close overtakes, acceleration, stopping and stale reentry.
Loopback transport tests check identical host-accepted samples on both computers including
self echoes; app tests check presentation/recording isolation and own-echo assistance
exclusion. No service, endpoint, dependency or rider control changes. The shared start still
uses the existing half-RTT estimate; arbitrary network outages cannot provide exact current
positions. Cross-home and native trainer acceptance remain manual.

### Reusable lobby — 2026-11-08

R237 separates ending an activity from closing group networking. The authenticated organizer
can end the selected revision during countdown or riding; the app does the same when its
local ride finishes. Each app releases its trainer, saves its own samples and returns to
Group Rides. A participant finishing locally does not end the shared activity. Structured
workouts on courses retain their existing continuation to the course finish.

Protocol/invitation version 16 and ALPN `orqwa-group/16` add a reliable organizer-only
`Lobby(ended_revision)` control. The activity actor retains its revision high-water mark
and membership generations while clearing selection, readiness, selected-content serving,
entry permissions, start/workout clocks and motion. The app cancels preparation and fallback
standby work, clears launch state and retains the original lobby and voice owners, consent,
microphone gates, local mutes and assistance choices. Reusing content still requires a new
selection revision and fresh readiness. Initial admission/reconnection controls include the
last ended revision before any current selection/running state, so missing an end during a
dropout cannot leave a rider locked into the previous course.

App state datagrams append the selection revision to the existing fixed state encoding.
Previous-revision packets are discarded under the existing bounded ingress budgets, including
across successive rides with new elapsed clocks. Motion histories clear without renewing
connection ingress allowances or voice generations. Revision-bound end requests cannot stop
a later activity. CLI single-course lobbies keep their existing state encoding and lifecycle.
No endpoint, service or dependency changes.

### Organizer setup before starting — 2026-11-08

R238 keeps one organizer-local preferred fallback while the group is in the lobby. Choosing
it immediately sends a private consent-only request over the authenticated group connection,
independent of activity selection and readiness. The bounded request has its own epoch and
deadline; only its authenticated selected member can answer it. Replacing the choice, departure
and timeout withdraw it. Consent and preference persist across activities for that membership.
After the shared start, the existing coordinator prefers that member when every rider is ready,
then uses the unchanged reachability, activity/epoch binding and standby preparation. No standby
links open before riding. An accepted request answers subsequent standby offers automatically;
a decline permits the normal automatic alternative without asking that member again.
Leaving or replacing a membership clears its preference; queued choices bind its generation.
Activity changes and returning to the lobby retain a preference for a member still present.

The countdown's Cancel action occupies the reserved Start slot. The separate End course for
everyone action appears only in the in-ride group panel. Both use the existing revision-bound
activity end. Protocol/invitation version 17 and ALPN `orqwa-group/17` distinguish the two new
consent controls from older clients. No endpoint, service or dependency changes.

### Automatic fallback in the lobby — 2026-11-09

R239 moves the coordinator's initial automatic candidate selection into the lobby. The first
eligible connected friend is asked using the existing consent-only controls, independent of
activity selection and readiness. A manual choice takes precedence. Pending requests and
accepted members are left alone; declined or timed-out members are skipped until explicitly
chosen again or replaced by a new membership. Departure permits the next eligible candidate.
No standby links are prepared until the shared ride has started and everyone is ready.

Once the original organizer is lost and the fallback becomes organizer during a ride, the
existing in-ride automatic selection and consent still arrange a replacement fallback. The
initial lobby decision and consent remain valid across subsequent activities. No protocol,
endpoint, service or dependency changes.

Cancelling a standby discards departures observed only on its disposable links. They become
group departures only when a takeover commits; the original authority's roster remains valid
while it stays connected. This keeps old standby teardown from removing the organizer before
the next activity's fallback can be prepared.
