# ADR 0043 — Signed and licensed world files

- Status: proposed
- Date: 2026-11-24
- Issue: #433
- Amends: [ADR 0042](0042-world-files.md)

## Context

World files (`.oqx`, [ADR 0042](0042-world-files.md)) carry complete worlds that the
maintainers build and sell. Premium video courses will be sold the same way. ADR 0042 left
the files unprotected: any `.oqx` opened, and a copy worked as well as the original.

The maintainers have decided:

- ORQWA opens only world files the maintainers signed. The world editor stays private.
- A purchase works on at most **5 devices**. Activating a device needs the internet once; after
  that the world works offline on that device for good.
- Signatures and encryption must hold against quantum computers where that is practical today.
- Signing keys must be replaceable without breaking files riders already have.

ORQWA is GPL-3.0, so anyone may build it with any check removed. No design can make a world
impossible to run outside the official ORQWA. What can be done:

- Encrypt the world, so a copied file is useless without a key.
- Hand out keys only per device and count the devices.
- Show the buyer's name on the world's page. This does not trace a leak: what leaks is the
  content key or decrypted parts, which are the same for every buyer. Tracing would need a key
  or a hidden mark per buyer, which this decision leaves out.
- Rely on the world's own licence (copyright, not the GPL) and on the name ORQWA
  ([ADR 0026](0026-the-name-orqwa.md)).

A fork that speaks the activation protocol and holds a real licence key can still decrypt
that buyer's world, on one of their 5 devices. The server cannot tell the official build from
a fork, because a desktop computer offers no way to prove which program is asking. Under
GPL-3.0 §3, the check in ORQWA is not a technological measure under laws such as the DMCA.
Taking it out of ORQWA's code is allowed, but copying or sharing a world is not.

Options considered for the cryptography:

- **Classical only (Ed25519, X25519).** Small and well known, but a world signed today could be
  forged once quantum computers break elliptic curves, and it should stay valid for years.
- **ML-DSA only.** Post-quantum, but young: an implementation flaw or a new attack would leave
  nothing behind it.
- **Hybrid: Ed25519 and ML-DSA-65 together**, both required. Safe as long as either holds.

Options considered for the library:

- RustCrypto's `ml-dsa`, `ed25519-dalek` and `aes-gcm`: pure Rust, but three or more new crates,
  and `ml-dsa` only reached 0.1 in 2026.
- **aws-lc-rs**, which ORQWA already builds for TLS (rustls, quinn, rcgen). From 1.19 it has
  stable ML-DSA next to Ed25519, AES-GCM, SHA-2, HKDF and PBKDF2. It becomes a direct
  dependency, with no new crate in the tree.

## Decision

### Cryptography (suite `orqwa-1`)

| Purpose | Algorithm |
|---|---|
| Signatures | Ed25519 **and** ML-DSA-65 over the same bytes; both must verify |
| Hashes | SHA-512 |
| Encryption | AES-256-GCM in 64 KiB chunks |
| Keys for entries | HKDF-SHA-512 from the world's content key and the entry's name |
| Key delivery | TLS 1.3 with the hybrid group X25519MLKEM768, then AES-256-GCM under a key only the device can derive |

Symmetric keys of 256 bits and SHA-512 keep their strength against quantum computers. The
suite has a name, so a later one can replace it in a new format without guessing.

Each encrypted part is deflated (videos are not: they are compressed already and must stay
seekable), then split into chunks of 64 KiB and stored in the archive without the archive's
own compression, which encrypted bytes would not shrink. Each chunk is sealed with a nonce of
an 8-byte random prefix for the entry and the chunk's number, and a flag in the associated
data for the last chunk. A chunk cannot be moved, repeated or dropped, and an entry cannot be cut
short. Entries of up to 1 GiB stream through checks without being held in memory, and a later
video reader can seek.

### Keys and their rotation

- **Root keys**: two hybrid key pairs, a primary and a backup, kept offline in separate
  places. Only their public keys are built into ORQWA. A root signs certificates and revocation
  lists, and nothing else. A new root comes with an ORQWA release; a root is withdrawn only if
  it leaks. **Either root alone is trusted fully** (1 of 2): the backup exists so that losing
  one root costs nothing. Either can sign a revocation list. Lists only add withdrawals, so a
  leaked root cannot take one back or lock out a later list; it could still withdraw the other
  root and its keys, which stops new worlds and activations until a release removes the leaked
  root. Both are kept offline for that reason.
- **Working keys**: a **world key** that seals world files and an **activation key** that the
  activation server signs with. Each has a certificate signed by a root, with its purpose, a
  name and both public keys. A certificate has no end date: a device's clock cannot be trusted,
  and a sold world must keep opening, so a key ends only by withdrawal. Every world file and
  every activation carries its certificate, so a working key is replaced by making a new one
  and having a root certify it. **ORQWA needs no update for that**, and files signed with the
  old key keep working.
- **Revocation**: a list signed by a root, numbered for people, naming withdrawn working keys
  and withdrawn versions of worlds. Each ORQWA release carries the lists known when it was
  built. The activation server returns the newest with every activation. ORQWA merges every
  list a trusted root signed and keeps them: a withdrawal is never taken back, whatever the
  numbers. A mistaken withdrawal is undone only by a new key. A world signed with a withdrawn
  key is sealed again and offered to its buyers. Fields a later ORQWA adds to a list are
  ignored, not a reason to drop it.
- **Development root**: a separate root for tests, fuzzing and worlds still being built. ORQWA
  trusts it only when built with the `dev-keys` feature. The programs ORQWA ships (`orqwa-gd`,
  `orqwa-cli`) refuse to compile a release build that has it; tests and examples may. Godot's
  debug and release exports use the same library, so a build that trusts it says
  "development keys" beside its version in About and at start, and CI's release jobs check
  that their release CLI refuses the development-signed test world.

### The file

Two entries are added to format 1. No `.oqx` without them has been released.

```
manifest.json    suite, world id and version, content key id if encrypted; every other entry
                 with its stored size, SHA-512 and whether it is encrypted
signature.json   the working key's certificate and both signatures over manifest.json's bytes
```

- ORQWA checks the signature **before** reading anything else, and refuses a file that is
  unsigned, signed by a key it does not trust, or revoked.
- Each entry is checked against its hash as it is read, at import and again on every ride.
- `world.json`, `preview.png` and `routes/*` stay unencrypted, so the library can show a
  world's name, picture, distance, climbing and countries before it is activated. Everything
  else (sections, assets, instances, horizon, sky, videos) may be encrypted, and is in sold
  worlds. A world may be signed but not encrypted, as the test world is.

### Activation

- On first use, ORQWA makes a **device secret** of 32 random bytes and a random device id, kept
  in a private device folder (`paths::device_dir()`) with a hash of the machine id. The
  **device key** is derived from the secret and the operating system's machine id; the machine
  id never leaves the device.
- **No machine id, no activation.** Where the operating system gives none (or, on Android,
  before the app has handed it over), ORQWA neither activates nor opens activations: one not
  bound to the machine would open wherever its folder is copied.
- To activate, the rider enters the licence key from the purchase. ORQWA sends the licence key,
  the world's id, the device id and the device key to the activation server over TLS with
  X25519MLKEM768. Only HTTPS, and no redirects: one would carry the keys to another host.
- **The server counts devices.** For each licence it keeps the devices it activated: the
  device id and a hash of its device key.
  - A known device id with the same key gets its activation again and uses no new slot, as
    after reinstalling ORQWA.
  - A known device id with a **different** key is refused. Otherwise one id could be shared by
    many computers and counted once. ORQWA itself never sends that: a device file copied to
    another computer is noticed there by its machine hash and replaced by a new identity.
  - A new device is added while fewer than 5 are counted; a sixth is refused, with a message to
    release one first.
- The server returns an **activation** signed with the activation key: licence id, world id,
  device id, the buyer's name, and the world's content key sealed under a key derived from the
  device key. It also returns the newest revocation list.
- Activations do not expire. After activation, the world opens without any network access.
- **Release this device** releases the whole device: every paid world on it. ORQWA sets all
  its activations aside and asks the server to free the device's slot in every licence. On
  success the device gets a **new identity** (id and secret), so an activation copied aside and
  put back is sealed for a device that no longer exists and opens nothing. If the server cannot
  be reached or refuses, the activations are put back and the device stays as it was. A device
  that is lost or wiped is released from the shop's account page.
- **The server limits releases per licence** (a few a year, in a rolling window, releases
  from the shop included), and counts slot assignments, not reinstalls: at most 5 devices at
  once and a few new ones a year. With the new identity a kept activation no longer survives a
  release in ORQWA; the limit bounds what a modified ORQWA, which may skip the new identity,
  could still do.
- The world's course page names the buyer ("Licensed to …") next to its credits.
- **One content key per world**, kept across its versions, so an activation keeps working for a
  new version. If a content key must change (it leaked), the server issues new activations and
  ORQWA asks the rider to activate again, on the same slot.

### Nothing syncs to another device

- World files live in `worlds/` in the data folder, which riders may sync: they are encrypted
  and open nothing by themselves.
- The device secret and activations live only in the device folder, which never roams and is
  no cache: `~/Library/Application Support/org.orqwa.device` on macOS,
  `%LOCALAPPDATA%\orqwa-device` on Windows (beside the cache, not in it),
  `$XDG_STATE_HOME/orqwa` on Linux and the app's private `files/device` on Android (no sync app
  reaches it). ORQWA refuses to use a device folder inside the data folder, or none.
- Android's backup of app data stays off, so a restored phone does not carry them.
- If a copy reaches another computer anyway (a backup restored, a migration), its machine id
  derives a different device key, so the activations do not open there, and the copied identity
  is replaced by a new one: that computer becomes a device of its own and activates as one.
- The licence key is not stored.

### Telling ORQWA from a fork

The server cannot reliably tell the official ORQWA from a fork, and ORQWA does not try:

- A secret built into release builds ships in every copy; extracted once, it serves every
  fork. Under the GPL a hidden key that only our builds hold is also questionable.
- Code signing proves a program to the operating system, not to our server: a signed app's
  requests can be replayed.
- Only platform attestation would prove it (Google Play Integrity, Apple App Attest), and only
  for Play-installed Android and Apple's mobile systems, at the price of depending on Google;
  nothing comparable exists on macOS, Windows or Linux.

The content licence, the name ORQWA, the release limit and the server's watch for unusual
patterns (one licence activating from many places) carry this instead.

### Not in ORQWA

- **The shop** is a web page outside ORQWA, run with a payment provider that handles VAT. It
  creates licences on the activation server. ORQWA shows the shop's address as text and takes
  nothing but a licence key.
- **The activation server** is a private service. It is not distributed, so the GPL does not
  reach it.
- **Sealing tools** (`orqwa-cli key-new`, `key-certify`, `revocation-sign`,
  `content-key-new`, `world-seal`) are open source and left out of the CLI's help; only the keys
  are secret. Private keys and content keys are kept encrypted with a password and never stored
  in CI. Sealing streams each part into the file, so a world may be larger than memory.

### A demo world

Every rider can try a world without buying one: a **demo world**, signed by the maintainers
and not encrypted, so it opens without activation in every official ORQWA.

- **Downloaded by the rider, not built in or fetched by ORQWA.** A world is gigabytes; built
  into ORQWA it would grow every installer, every update and the Android APK by as much. The
  project's website and release pages link to it; the rider downloads it and imports it like
  any world file. ORQWA needs no code for it and contacts no further server. The small sample
  course (R95) stays built in for riding offline at once.
- A newer demo replaces the old one on import, by world id; rides on it are kept.
- **Openly licensed.** Published by ORQWA's project and readable by anyone, it carries a
  licence that allows redistribution (R47) and must hold nothing that is sold. The demo is the
  Twann – Chasseral world; paid worlds are other worlds.

### Video courses (later stage)

A world file may hold a video course: a route with `video` (the entry, its offset and sync
marks) and no sections, the video **stored** (not deflated) and encrypted in chunks. FFmpeg
reads it through a custom input that decrypts chunks as it seeks. That needs a second module
with unsafe code in `orqwa-video` and an amendment to [ADR 0010](0010-video-decoding.md); it
comes in its own change.

## Consequences

- ORQWA opens only the maintainers' worlds. Third-party worlds are not possible until the
  maintainers decide to certify other publishers' keys, which the certificates allow without a
  format change.
- Activating needs the internet once per device and world. The activation server is listed
  where riders read which servers ORQWA contacts.
- The device secret and machine id stay on the device. The server learns a random device id,
  the device key it needs to seal the content key, and the licence key; nothing that
  identifies the computer.
- A rider who reinstalls the operating system uses a new slot until the old one is released.
- aws-lc-rs becomes a direct dependency, updated to 1.19. Nothing new is added to the tree.
- Tests, the fuzzer and the test world use the development root. The fuzzer skips signatures,
  so the parsers behind them are still fuzzed.
- Losing both roots would mean no new working keys could be certified until a release adds a
  new root. Worlds already sold would keep working.
