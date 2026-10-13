# ADR 0015 — Group rides without a server

- Status: accepted
- Date: 2026-10-13

## Context

Riders want to ride a course together, each on their own trainer at home (R67). R27 left
multiplayer for later; ADR 0001 kept a headless core for "a future multiplayer server". But
Torqa has no server of its own and no budget for one (R2, R3), and a server would need running,
accounts and someone's trust with everyone's rides.

Without a server, two Torqas have to find each other on their own:

- **A computer does not know the address the internet sees.** Behind a home router it has a
  private address (`192.168.…`); the router maps its UDP port to a public one only once a
  packet goes out. Ways to learn that public address:
  - **A STUN server** (RFC 8489) answers a single packet with the address it came from. Public
    ones are run for free by Google, Cloudflare and others; they keep no state and never see a
    ride.
  - **Asking the router** for a mapping (UPnP IGD, NAT-PMP, PCP). No third party, but often
    switched off.
  - **IPv6** addresses are public; only the firewall stands in the way.
- **Both sides need each other's address.** Each router lets a packet in only after one went out
  to its sender, so both send at the same time ("hole punching"). This works through most home
  routers, whose mapping is the same for every destination (endpoint-independent). It fails
  through **symmetric NATs** (some carrier-grade NAT, mobile hotspots, company networks), which
  pick a new public port for every destination — a minority of connections, but not a rare
  one. Without a relay there is no way around that.
- **Router mappings expire** after 30 s to a few minutes without traffic.

Libraries: `iroh` and `libp2p` do hole punching, but both lean on relays (iroh's public ones, or
servers to run) for rendezvous and fallback. WebRTC (`webrtc-rs`) brings ICE, DTLS and SCTP
for what is a few hundred bytes a second. QUIC (`quinn`) gives encryption, reliable streams and
unreliable datagrams over one UDP socket, and that socket can be punched first.

## Decision

- **Invites are codes the riders pass on themselves**, as text (to paste into any messenger)
  and as a QR code showing the same text, for a phone or someone in the same room. Torqa has no
  rendezvous server.
  - The **host's invite** carries: format version, the host's address candidates, the SHA-256
    fingerprint of the host's certificate for this ride, the course (name and a hash of its
    route) and the host's name. One invite serves everyone who joins.
  - A **joiner's reply** carries the joiner's address candidates and a random secret of 16
    bytes. The host pastes each reply.
  - Codes are `torqa:` followed by the base64url of a compact binary encoding; a few hundred
    characters, well within a QR code.
- **Address candidates** of each side, all tried: its local addresses (riders on one network),
  its global IPv6 addresses, the public IPv4 address from **public STUN servers** (on by
  default; can be switched off in the settings, leaving local addresses, IPv6 and the router),
  and a mapping from the router by **UPnP, NAT-PMP or PCP** where it allows one. The STUN
  request is Torqa's own (one binding request, RFC 8489), not a crate.
- **Hole punching on one UDP socket per rider.** A joiner starts sending small probes to every
  candidate of the host as soon as the invite is pasted; the host does the same towards a
  joiner's candidates once its reply is pasted. The probes open both routers; the joiner's QUIC
  connection then gets through. Probing goes on for 60 s; a joiner whose reply is not pasted by
  then is told so and makes a new reply.
- **A star around the host**, up to **8 riders** (host included). Each joiner exchanges codes
  with the host only; the host forwards everyone's state to everyone. A full mesh would need
  every pair to exchange codes. If the host leaves, the ride goes on for everyone alone.
- **QUIC (`quinn` with `rustls`)** on the punched socket. The host makes a self-signed
  certificate per ride (`rcgen`); a joiner accepts only the fingerprint from the invite, so
  nobody can step in between without a certificate authority. A joiner's first message is its
  secret; the host accepts only secrets from replies it pasted. Keep-alives every 10 s hold the
  routers' mappings open.
- **What goes over the wire**: a reliable stream for the lobby (who is in, the countdown, who
  left) and **QUIC datagrams 5 times a second** with each rider's distance, speed, power and
  cadence (plus their name and weight once). Heart rate stays private. A lost datagram is not
  resent; the next one replaces it.
- **Each rider rides their own physics**, on their own trainer, settings and difficulty, as
  alone. Others are only shown: interpolated between datagrams and drawn like ghosts (R20) —
  a rider in the world, a dot on the map and the profile, and a panel with the gaps. No drafting
  for now.
- **A common start**: the host starts a countdown once everyone is in; it is sent as "start in
  n ms" and each joiner subtracts half the round trip. Nobody can join after the start.
- **Same course for everyone**: a joiner must have the course in their library (matched by the
  route's hash); otherwise Torqa says so and the host shares the `.tqc` as usual (ADR 0007).
- Everything lives in a new crate **`torqa-net`**: codes, discovery, punching, the protocol and
  the lobby. `torqa-session` gets remote riders beside its ghosts; Godot only shows them. No
  plugin trait: there is one way of connecting.
- **New dependencies**: `quinn` and `rustls` (QUIC and TLS), `rcgen` (the ride's certificate),
  `qrcode` (drawing the QR code), `igd-next` (UPnP) and `crab_nat` (NAT-PMP and PCP). All MIT
  or Apache-2.0. Scanning a QR code with a camera is left out: it needs camera access on three
  platforms for something pasting already does.

## Consequences

- No server to run, no accounts, no ride data through anyone else. STUN servers see only that
  a Torqa asked for its address.
- Passing codes takes a minute and both must be at their computers within 60 s of each other.
  The user docs walk through it.
- Some riders cannot join: behind a symmetric NAT on both ends of a pair, or where UDP is
  blocked. Torqa says the connection failed and why it may have (try IPv6, enable UPnP on the
  router, another network) rather than waiting. A relay server would be needed to fix this for
  everyone, which this ADR rules out.
- The host's upload carries everyone's state: 8 riders at 5 Hz is roughly 8 × 7 × 5 datagrams
  of under 100 bytes a second, about 30 KB/s, which any home connection carries.
- A joiner's code and the host's invite reveal their IP addresses to each other, as any
  peer-to-peer connection does. The user docs say so.
- Rides are saved as each rider's own, as today.
