# ADR 0043 — Signed world files, sold without activation

- Status: proposed
- Date: 2026-11-24
- Issue: #433
- Amends: [ADR 0042](0042-world-files.md)

## Context

World files (`.oqx`, [ADR 0042](0042-world-files.md)) carry complete worlds that the
maintainers build, and some of them will be sold, as will premium video courses. ADR 0042 left
the files unchecked: any `.oqx` opened, including one changed after it was made.

The maintainers have decided:

- ORQWA opens only world files the maintainers signed. The world editor stays private.
- A bought world is the buyer's to use on **all their own devices**, offline, for good. There
  is **no activation, no device limit and no server**: ORQWA contacts nothing for a world.
- Signatures must hold against quantum computers where that is practical today.
- Signing keys must be replaceable without breaking files riders already have.

Options considered for protecting sold worlds:

- **Encryption with activation per device**, counted by an activation server. Built and then
  dropped: an open GPL client cannot stop a determined buyer from extracting the scenery, so
  it mostly burdens honest buyers (new devices, reinstalls, releasing slots), and it needs a
  server to run for as long as anyone owns a world. It also cuts against what ORQWA stands for:
  open, offline-first and private.
- **Encryption without a server.** The key would have to travel with the world, so it would
  protect nothing.
- **Signed worlds, sold under a licence.** Riders know a world is the maintainers' and intact;
  copying a sold world breaks its licence, which is copyright law, not the GPL. This is how most
  open programs with sold content work (an open engine with commercial data, as with id
  Software's engines or OpenMW).

Options considered for the cryptography:

- **Classical only (Ed25519).** Small and well known, but a world signed today could be forged
  once quantum computers break elliptic curves, and it should stay valid for years.
- **ML-DSA only.** Post-quantum, but young: an implementation flaw or a new attack would leave
  nothing behind it.
- **Hybrid: Ed25519 and ML-DSA-65 together**, both required. Safe as long as either holds.

Options considered for the library:

- RustCrypto's `ml-dsa` and `ed25519-dalek`: pure Rust, but new crates, and `ml-dsa` only
  reached 0.1 in 2026.
- **aws-lc-rs**, which ORQWA already builds for TLS (rustls, quinn, rcgen). From 1.19 it has
  stable ML-DSA next to Ed25519, SHA-2, PBKDF2 and AES-GCM. It becomes a direct dependency,
  with no new crate in the tree.

## Decision

### Cryptography (suite `orqwa-1`)

| Purpose | Algorithm |
|---|---|
| Signatures | Ed25519 **and** ML-DSA-65 over the same bytes; both must verify |
| Hashes | SHA-512 |
| The maintainers' key files | PBKDF2-HMAC-SHA-512 (600,000 rounds), then AES-256-GCM |

Each signature carries what it is for (a certificate, a withdrawal list, a manifest), so a
signature on one kind of document never passes for another. The suite has a name, so a later
one can replace it in a new format without guessing.

### Keys and their rotation

- **Root keys**: two hybrid key pairs, a primary and a backup, kept offline in separate
  places. Only their public keys are built into ORQWA. A root signs certificates and withdrawal
  lists, and nothing else. A new root comes with an ORQWA release; a root is withdrawn only if
  it leaks. **Either root alone is trusted fully** (1 of 2): the backup exists so that losing
  one root costs nothing. Lists only add withdrawals, so a leaked root cannot take one back; it
  could still withdraw the other root and its keys, which stops new worlds until a release
  removes the leaked root. Both are kept offline for that reason.
- **World keys** seal world files. Each has a certificate signed by a root, with its purpose,
  a name and both public keys. A certificate has no end date: a device's clock cannot be
  trusted, and a bought world must keep opening, so a key ends only by withdrawal. Every world
  file carries its certificate, so a world key is replaced by making a new one and having a
  root certify it. **ORQWA needs no update for that**, and files signed with the old key keep
  working.
- **Withdrawals**: lists signed by a root, naming withdrawn keys and withdrawn versions of
  worlds, built into each ORQWA release (no server delivers them). ORQWA merges every list a
  trusted root signed: a withdrawal is never taken back. A mistaken one is undone only by a new
  key. A world signed with a withdrawn key is sealed again and offered to its buyers. Fields a
  later ORQWA adds to a list are ignored, not a reason to drop it.
- **Development root**: a separate root for tests, fuzzing and worlds still being built. ORQWA
  trusts it only when built with the `dev-keys` feature. The programs ORQWA ships (`orqwa-gd`,
  `orqwa-cli`) refuse to compile a release build that has it; tests and examples may. Godot's
  debug and release exports use the same library, so a build that trusts it says
  "development keys" beside its version in About and at start, and CI's release jobs check
  that their release CLI refuses the development-signed test world. A tagged build also fails
  while no release root is built in.

### The file

Two entries are added to format 1. No `.oqx` without them has been released.

```
manifest.json    suite, world id and version; every other entry with its size and SHA-512
signature.json   the world key's certificate and both signatures over manifest.json's bytes
```

- ORQWA checks the signature **before** reading anything else, and refuses a file that is
  unsigned, signed by a key it does not trust, or withdrawn.
- Each entry is checked against its signed size before it is read, and against its hash as it
  is read, at import and again on every ride. A changed part is reported as changed, whatever
  its parser would have made of it.
- Nothing is encrypted.

### Selling worlds

- **The shop** is a web page outside ORQWA, with a payment provider that handles VAT. The buyer
  downloads the world file and imports it like any other. Nothing in ORQWA asks for a key,
  contacts a server or counts devices.
- **The licence** is the world's own, shown on its page (`licence` in `world.json`): for the
  buyer's own use on any of their devices, not to be shared. That is copyright in the world,
  not the GPL; GPL-3.0 §3 does not touch it.
- **Content licences** of what goes into a sold world must allow selling it: own work, CC0 or
  CC-BY with credit, swisstopo data with its credit; no CC-BY-SA material and no OpenStreetMap
  data.
- **Sealing tools** (`orqwa-cli key-new`, `key-certify`, `revocation-sign`, `world-seal`) are
  open source and left out of the CLI's help; only the keys are secret. Private keys are kept
  encrypted with a password and never stored in CI. Sealing streams each part into the file, so
  a world may be larger than memory.

### Telling ORQWA from a fork

There is nothing to tell apart: worlds open offline and ORQWA contacts no server for them. A
fork may open the maintainers' worlds only if it trusts their roots, and copying a sold world
into it breaks the world's licence. A secret or a signed hash built into official builds
would ship in every copy and prove nothing.

### A demo world

Every rider can try a world without buying one: a **demo world**, signed by the maintainers.

- **Downloaded by the rider, not built in or fetched by ORQWA.** A world is gigabytes; built
  into ORQWA it would grow every installer, every update and the Android APK by as much. The
  project's website and release pages link to it; the rider downloads it and imports it like
  any world file. The small sample course (R95) stays built in for riding offline at once.
- A newer demo replaces the old one on import, by world id; rides on it are kept.
- **Openly licensed.** Published by ORQWA's project, it carries a licence that allows
  redistribution (R47) and must hold nothing that is sold. The demo is the Twann – Chasseral
  world; paid worlds are other worlds.

### Video courses (later stage)

A world file may hold a video course: a route with `video` (the entry, its offset and sync
marks) and no sections, the video **stored**, not deflated, so FFmpeg can seek in it. Reading
it from inside the archive needs a custom input in `orqwa-video` and an amendment to
[ADR 0010](0010-video-decoding.md); it comes in its own change.

## Consequences

- ORQWA opens only the maintainers' worlds. Third-party worlds are not possible until the
  maintainers decide to certify other publishers' keys, which the certificates allow without a
  format change.
- A bought world works on all of the buyer's devices, offline, for good, with no server to
  keep running. Copies can be shared against the licence; that is handled by the licence and,
  if needed, takedowns, not by ORQWA.
- aws-lc-rs becomes a direct dependency, updated to 1.19. Nothing new is added to the tree.
- Tests, the fuzzer and the test world use the development root. The fuzzer skips signatures,
  so the parsers behind them are still fuzzed.
- Losing both roots would mean no new world keys could be certified until a release adds a
  new root. Worlds already sold keep working.
- Device activation (encryption, an activation server, a 5-device limit) was built in #672
  and removed before merging; its history stays in the repository if it is ever needed.
