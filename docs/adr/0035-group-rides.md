# ADR 0035 — Group rides with bounded peer-to-peer networking

- Status: proposed
- Date: 2026-10-27
- Replaces the unmerged proposals #241, #243, #246 and #247

## Context

R27 leaves multiplayer for later. Riders want to ride the same course from different homes,
without running a service, creating accounts or paying for a relay (R2, R3). The earlier
networking PRs were never merged. They predate the ORQWA rename and reuse requirement, ADR
and phase numbers now assigned to other features. Their security review found unbounded
connection and event resources, unchecked destinations and numbers, incomplete deadlines,
and detached listeners and router-renewal tasks.

## Decision

Implement the replacement as `orqwa-net`, in separate, reviewable steps on application-owned
branches based on current main. Requirements R159–R161 and Phase 13 identify this work.
The initial usable networking delivery is a developer CLI; app/session integration remains a separate step.
The existing Android phase, requirements and usage-wiki ADR keep their identifiers.

### Trust and privacy

- Riders exchange a host's invite and each joiner's reply through an authenticated, private
  channel they choose. Replacing an invite replaces the identity it authenticates; TLS cannot
  establish who sent a code. Replies contain bearer credentials and must not be published.
- The invite contains the host's per-ride certificate fingerprint, a fresh ride identifier,
  course name/hash and bounded address candidates. Each reply binds its fresh admission
  credential to that ride and certificate. A reply for another ride is refused before probing.
  Credentials are generated with a cryptographic random source and redacted from Debug/logs.
- Joining pins the complete host certificate fingerprint and verifies the TLS handshake
  signature. Admission credentials are sent only after that verification. Authentication
  messages also bind the ride and course. A used, expired or revoked credential cannot admit
  another connection. No early application data or cross-ride resumption is required.
- The host assigns the sender of forwarded state from the authenticated connection. Remote
  state never changes a rider's trainer commands, physics, heart rate or saved activity.
- Codes reveal names, course metadata and the advertised addresses to recipients and the
  service used to exchange them. The host sees its joiners' addresses; it does not forward
  those addresses or their admission credentials to other riders.

### Network policy

- No networking starts merely because the app opens. A developer explicitly runs a network
  command. Ordinary rides remain offline. Public STUN discovery has a separate switch;
  turning it off does not enable or disable router forwarding.
- Advertise addresses of a selected interface, using the default interface when none is
  selected. Do not enumerate every VPN/container/interface address into an invite. LAN
  candidates require explicit LAN mode; loopback is a separate, explicit local-test mode.
- Validate every imported candidate before any probe or QUIC connection. Public labels do
  not override address policy. Reject zero/privileged ports, unspecified, multicast,
  broadcast, link-local, reserved/documentation and IPv4-mapped IPv6 destinations. Private
  addresses require LAN mode and loopback requires local-test mode. Bound and deduplicate
  candidates before scheduling attempts. Connectivity checks have a finite packet budget.
- Router forwarding is deferred from the first implementation. A future optional mapping
  feature is **off by default**, independently enabled by the rider. Ask for
  short, finite leases; renew only while the session owner is alive. Stop renewal on drop
  and attempt bounded removal on explicit close and cancellation. A router may refuse
  removal or lose its response, so the finite lease is the fallback, not a claim that every
  mapping disappeared immediately. Select/release competing mappings without an indefinite
  cleanup wait. Do not use a private or special-use external address as a public candidate.
- STUN uses cryptographically random 96-bit transaction identifiers, source/transaction
  matching, strict bounded parsing and retransmission within the common deadline. Discovery
  is not peer authentication. Two matching observations are evidence about those mappings,
  not proof that every destination will be reachable. No relay means some networks cannot
  connect; failure explains uncertainty rather than claiming a particular NAT was proved.

### Resource and lifecycle limits

- One absolute deadline, at most 60 seconds, includes discovery, punching, TLS, credential
  exchange and lobby admission. Every read, write, retry and router operation is bounded by
  the remaining time. Cancelling an operation tears down its owned resources.
- Limit concurrent transport connections and pending handshakes **before** spawning work or
  doing TLS. Apply a global handshake rate budget and a bounded per-source pending limit.
  Exhaustion rejects work without adding another task or growing a source-address table.
  QUIC stream counts, stream windows and datagram buffers are small explicit limits.
  The protocol foundation allows 16 total pending/established connections, at most eight from
  one source address, an initial handshake burst of 16 and sustained replenishment of four
  attempts per second. Permits are held until their connections are dropped.
- Up to eight riders including the host. Pending replies, admission events, commands and
  control messages have fixed capacities. Full queues fail/close the affected peer; they do
  not allocate indefinitely or stall the trainer loop. No peer-supplied input creates an
  unbounded channel or task list.
- State is disposable: retain at most one latest state per rider, at most 5 Hz per peer,
  and coalesce before fan-out. Limit incoming work before queueing it, including duplicate
  or malformed traffic. Sustained offenders disconnect. Reliable joined/left/start events
  remain separate and bounded. A slow rider cannot backlog everyone else's state.
- Validate finite physical values, sensible ranges and timestamps against the session before
  accepting/forwarding them. Names are bounded UTF-8 without control characters. Validate
  counts, rider identifiers and message direction/state as well as frame lengths.
  The state format bounds a session to 48 hours, distance to 10,000 km, speed to 150 m/s,
  power to 5,000 W and cadence to 300 rpm. A timestamp may lead local elapsed time by at
  most two seconds. The ingress budget allows a jitter burst of ten packets followed by
  five per second; malformed and duplicate packets also consume it. A reused rider slot
  has a new local generation so an old connection cannot submit for or remove its successor.
- Dropping the public host/joiner/lobby owner closes its endpoints and cancels all owned
  tasks. Explicit async close also waits for bounded cleanup. Background tasks must not keep
  the service or router lease alive after the owner disappears. No strong ownership cycle
  may depend on receiving a future network event to end.

### Rider behavior and scope

- A star around the host, up to eight riders. Each has the same course locally, matched by
  its route hash; course distribution remains file-based. No new content download service.
- A shared countdown, no late admission after it starts, no drafting. Each rider keeps their
  own difficulty and physics. Losing the host or another rider does not end the local ride.
- Exchange distance, speed, power and cadence; heart rate stays private. Others eventually
  appear beside the existing ghosts, on the map/profile and in a gaps panel. That app/UI work
  is not part of the initial CLI networking steps.
- Reuse existing QR support if needed later. New dependencies require approval and current
  stable releases; preserve main's patched dependency graph. Do not restore an old lockfile.
- Add CLI instructions and the wiki's internet/firewall entries with the command that first
  makes those calls. Design and security internals stay in this repository, not the usage wiki.

## Validation

Tests assert the intended security properties: bounded decoder allocation, destination-policy
bypasses (including mapped IPv6), wrong pins/ride/course credentials, replay and expiry,
connection admission before limits, one end-to-end deadline, drop/cancellation cleanup,
slow peers, state floods and invalid numbers/timestamps. Use loopback-only stand-ins and fake
trainers; no tests flood public hosts or depend on public STUN availability. Simulated router
leases must expire and stop renewing after drop. Two real home networks and native trainer
rides remain separate manual acceptance checks; passing local tests does not prove them.

## Delivery status

The protocol foundation provides bounded payloads, destination policy, connection permits and
state validation/coalescing. Discovery adds bounded base64url text, cryptographic identifiers
and SHA-256, selected-interface UDP sockets, strict STUN parsing (including FINGERPRINT) and
the explicit `net-check` command. Loopback mode never contacts STUN. Transport adds per-ride
TLS 1.3 certificate pinning with signature verification, one-use admission credentials and
`net-ping`. Its TLS handshakes require a pending reply, address validation and a capacity/rate
permit first. QUIC has explicit stream, flow-control, datagram and handshake-buffer limits.
Both the application's tasks and Quinn's runtime drivers have owners; cancellation permits
at most 100 ms to flush close packets, then aborts remaining drivers. The driver registry
itself is capped at 128. Used/expired reply tombstones are retained within a sixteen-record
host-lifetime budget.

The lobby now assigns up to eight roster slots, validates printable names and finite 20–300 kg
rider-plus-bike masses, and forwards no peer addresses or credentials. Reliable controls are
length-prefixed and limited to 1,024 bytes; a partial frame has two seconds to finish. Application
controls have 32 entries, peer control queues 16 and host commands eight. Overflow closes the
affected peer or lobby. The host counts down once and seals pending admissions. Joiners estimate
the common start from the remaining countdown minus half their measured RTT; this is approximate
clock alignment, not a synchronized clock service. There are no late joins or slot reuse after
the countdown. Before it, state-mailbox generations protect reused slots from stale connections.

State passes through the fixed eight-slot mailbox before 5 Hz fan-out. Both local updates and
outbound snapshots use watch channels that replace a single value. Incoming datagrams charge
budgets before parsing; malformed values, forged senders, future timestamps and sustained floods
close the offender. A joiner also bounds total forwarded traffic and control-message count.
Fractional refill time is preserved so legitimate 5 Hz jitter does not consume the allowance.
Candidate attempts are staggered to avoid spending multiple TLS permits when one path works.

`ride-together` prepares the local GPX and devices before opening the join window, checks the
hash of the exact GPX bytes, and shows remote gaps. Countdown and remote display state cannot
call local physics/trainer/recording APIs. Each process records only its own activity and keeps
riding after network loss. Two loopback CLI processes with fake trainers have exercised the
countdown, gaps, independent FIT saves and continued recording after host exit. App rendering,
native trainers and real home-network acceptance remain outstanding.

ADR 0036 adds optional voice through the existing host links, with independent consent,
traffic/jitter budgets and membership epochs. Voice can run in the lobby; protocol version 2
requires fresh codes and matching builds. Router forwarding remains deferred.

The owner approved the core dependency selection on 2026-10-27 after its advisory, licence
and source checks: quinn 0.11.12, rustls 0.23.46, base64 0.23.1, rcgen 0.14.10 and netdev
0.47.0 (current stable releases verified against crates.io, again on 2026-10-28). Transport
uses quinn/rcgen; discovery uses base64, rustls and netdev. Router libraries crab_nat/igd-next remain
deferred; they are not needed for STUN, public IPv6 or LAN connections. Existing `fast_qr`
covers future QR display.

Address filtering follows the conservative direct-peer policy above, using the IANA
[IPv4](https://www.iana.org/assignments/iana-ipv4-special-registry/) and
[IPv6](https://www.iana.org/assignments/iana-ipv6-special-registry/) special-purpose registries.
It deliberately excludes translation/tunnelling aliases and special assignments even where
some are globally routed; those are not required direct interface candidates in this version.
The transport integration must use both [Incoming admission controls](https://docs.rs/quinn/0.11.12/quinn/struct.Incoming.html)
and explicit [QUIC buffer/stream limits](https://docs.rs/quinn/0.11.12/quinn/struct.TransportConfig.html).

### Connection reliability — 2026-11-01

A host asks STUN once, when it opens. Joiners probe it only after the host admits their
reply, which can be minutes later, while home and carrier NATs often forget an idle UDP
mapping within a minute; carrier NATs then usually pick another port, so the invite's address
stops working. Each path that advertises a STUN mapping now sends a 20-byte Binding indication
(RFC 8489, never answered) to the server that observed it every 15 seconds, the interval ICE
uses (RFC 8445). It pauses while a connection uses that path, whose QUIC keep-alives refresh
the same mapping, and ends when the invite is sealed or the host owner is dropped. Disabling
STUN sends none. Paths sort IPv6 first, as Happy Eyeballs does (RFC 8305): carrier NAT often
shares IPv4 while IPv6 stays directly reachable, and the existing stagger still tries IPv4
within 250 ms.

An admitted connection's lobby handshake now keeps the admission's single deadline; the
three-second limit bounds only the credential exchange. State that arrives before the local
start, by at most the two seconds a peer timestamp may lead, counts from the start: joiners
estimate it from half the round trip. State during the lobby remains a protocol violation.

There is still no relay. A host behind carrier-grade NAT whose mappings change per destination
cannot be reached; the usage guide tells riders that the host must not be behind CGNAT and how
to check, and a rider-provided relay remains a possible later decision.

### Joiner certificates — 2026-11-01

Replies carried a 256-bit bearer secret: anyone who read a reply in a chat, clipboard history
or screenshot could connect first and take that rider's place within the admission window.
Each joiner now generates a per-ride certificate with rcgen, as the host does, and the reply
carries its SHA-256 instead of a secret, so codes keep their length. The host requires TLS
client authentication. Its verifier lets any certificate finish the handshake but checks the
handshake signature, so the joiner proves its private key; admission then looks the
certificate's pin up in the existing one-use table, with its pending, used and expired
tombstones, deadlines and budgets unchanged. The authentication message still binds the
ride, host pin and route.

A copied reply therefore admits nobody else, and leaked codes reveal only names and
addresses. The invite must still arrive unaltered: someone able to replace it in transit
could substitute their own host pin, as before. A short comparison code against that was
considered and deferred; it would need at least 64 bits, because an attacker who controls
the chat can generate keys until a shorter code matches. Planned handover uses the same
mechanism: each rider's replacement reply names its fresh replacement-link certificate,
and the successor binds that pin to the reserved slot, so no secret passes through the
predecessor. Protocol/invitation version 7 and ALPN `orqwa-group/7` require matching builds
and fresh codes.

### Reachability hint — 2026-11-01

`Reachability::of` classifies a code's own candidates with its IPv4 STUN observation: a public
IPv4 address or the same mapping from two servers reads as reachable; a global IPv6 address
with strict or missing IPv4 evidence as IPv6 only; different mappings per server, the
endpoint-dependent behaviour typical of carrier-grade NAT, as strict; anything else as unknown,
or home-network only. Codes now report the IPv4 path's observation, where address sharing
happens. The host's invite drives a warning under Invite friends, and a joiner's reply the
same advice when organizer duties are offered. It is evidence rather than detection: nothing
here sees the provider's NAT or a firewall, so the wording says "probably" and points to the
usage guide's manual check. It adds no network request.

### Fuzzing — 2026-11-01

Every decoder that reads a peer's or a pasted code's bytes has a libFuzzer target in `core/fuzz`:
invite/reply codes, lobby and handover controls, voice and state datagrams, activity selections,
normalized workouts, STUN responses and the shared-course archive validator. Beyond "never panic",
each target checks that what a decoder accepts encodes again into an accepted, identical form
(binary codes and voice packets byte for byte), so a host cannot admit input it later fails to
forward. The properties live in `orqwa-net` behind a `fuzzing` feature, and stable unit tests run
them over valid seeds, every truncation and every bit flip. Seeds come from the same encoders on
each run, so they never go stale. CI runs each target for 30 seconds on every change with a
pinned nightly toolchain and cargo-fuzz in the dev image; the fuzz crate is its own workspace, so
the app never builds with nightly. Its lock follows the main one, and cargo-deny checks it too.

### Rider weight and avatar — 2026-11-01

Riders saw each other's names and gaps but not their effort, and every friend rode as the
default avatar (#422, #423). A roster entry now carries, beside the name and the rider-plus-bike
mass, the rider's **own weight** and **avatar**. Others show each rider's power and power per
kilogram beside their name in the riders panel and under it on the road, and draw them as they
chose to look. The weight is validated like the mass, from 20 kg up to that mass, and an
unknown avatar is a protocol error. The rider-plus-bike mass already revealed about as much as
the weight, so sharing it with the group adds little; heart rate stays private as before.
Protocol/invitation version 8 and ALPN `orqwa-group/8` require matching builds and fresh codes.

### Preparation progress — 2026-11-01

Riders waited for each other without knowing who was still downloading or building the world
(#419), and a rider's own page went quiet once the download finished (#420). Each rider now
reports what it is doing for the current selection, **downloading** or **preparing**, and how
far it has got in percent: a new joiner-to-host control the host checks against the current
revision and the rider's readiness, then forwards to everyone, the organizer's own report
included. Reports are advisory and rate-limited by the sender, at a change of stage and at most
every two seconds otherwise, well within the host's existing control budget; a full queue
drops one, as the next supersedes it, and readiness ends them. Preparing a course reports the
app's own build steps on one scale, so the percentage only grows. Protocol/invitation version
9 and ALPN `orqwa-group/9` require matching builds and fresh codes.

## Consequences

Code exchange exposes peer addresses and relies on the chosen channel's confidentiality and
authenticity. There is no service operator to run, but no guaranteed relay fallback either.
Opt-in LAN candidates and router mapping can require one extra choice on restrictive networks.
The host still carries everyone's state; strict budgets trade some join retries/disconnections
for keeping memory, CPU and the local ride independent of a peer's behavior.
