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

Course transfers carry `.tqc` inputs, not built worlds or video. Structured workout transfer
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
which is cancelled on selection changes or departure. A completed worker is reaped before
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
charts, and require explicit confirmation before sharing. Readiness names the rider's next
action, including trainer connection. These are presentation changes over the existing
admission and transfer protocol.

Audio device selection remains available during voice. The app synchronously disables
transmission and advances its activation generation before queueing the switch. The owner
cancels pending startup, reconnects opted-in voice and retains playback gain, deafen and
per-speaker mutes. The microphone always requires fresh explicit consent after reconnection.
Selected device IDs and playback gain in the snapshot keep lobby and in-ride controls consistent.
Device enumeration has its own single-job budget, independent of pending voice startup, so
opening audio settings cannot consume or suppress the rider's voice activation request.

### Remote motion and assistance delivery — 2026-10-28

The app now renders up to seven remote riders beside the local rider and ghosts, with
matching map/profile markers and HUD distance gaps. Eight fixed histories are bound to
local membership generations. Positions interpolate 100 ms behind packet arrival,
extrapolate for at most 300 ms, hold briefly, then disappear after two seconds. Reordered,
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
Replacement admission binds a transport-authenticated secret to the reserved original rider
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
generation and microphone off; device choices, gain, deafen and local mutes remain personal.
The new host shares its already verified content and publishes a fresh invitation. No extra
service, endpoint, package or dependency version is introduced.
