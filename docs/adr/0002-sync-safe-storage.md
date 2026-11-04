# ADR 0002 — Sync-safe storage

- Status: accepted
- Date: 2026-10-02

## Context

The data directory is configurable and may live in a file-synced folder such as Nextcloud (R30).
SQLite databases inside synced folders are prone to corruption and sync conflicts, because sync
clients copy files while they are being written and cannot merge concurrent changes.

## Decision

- **Files are the source of truth**: each ride is one FIT file plus one JSON metadata file; routes
  and generated worlds are stored as files too. Files are written atomically (write temp, rename).
- A **SQLite index** (history, PRs, search) is a rebuildable cache stored **outside** the data
  directory, in the platform's local cache location. If it is missing or stale, it is rebuilt by
  scanning the data directory.
- Per-profile data lives in per-profile subdirectories to avoid write contention between users.

## Consequences

- Syncing between machines works with any file sync tool; at worst a conflict produces a duplicate
  file, never a corrupt database.
- Index rebuilds cost a scan of the data directory at startup when changes are detected.

## Amendment 2026-10-16: settings per rider and per computer (#257, R78)

`settings.toml` in the data directory held every setting, shared by all riders and, with the
directory synced, by all computers: the last to save won, so one machine's graphics quality
overwrote another's. Now:

- **The computer's own** settings — graphics quality (R43) and full screen (#218) — live in
  `computer.toml` in the cache directory (`orqwa_storage::computer`), which is this computer's
  and never synced. Clearing the cache loses them, which costs one choice; the alternative, a
  platform configuration directory, would be a third location to explain.
- **The rider's own** overlay place and size (R55) live with the rider, in
  `profiles/<id>/overlay.toml`, like the HUD layout: they follow the rider between computers.
- `settings.toml` keeps what is shared by design: the active rider, the remembered devices,
  the shifter's buttons, the ride bar and the Courses view.

Values from before are carried over once at start (`profiles::carry_over`): the overlay's
place to every rider without their own, the quality and full screen to the computer where the
app starts first, unless it has chosen already; then they are removed from `settings.toml`,
so a second computer sharing the data starts from its own defaults rather than the first's.

## Amendment 2026-10-16: the index as built (#262, R79)

The index is `history.sqlite` in the platform's cache directory (`orqwa_storage::index`,
through `rusqlite` with SQLite compiled in, so no platform needs a system library). It holds
one row per ride: the FIT file's path, the metadata file's size and modification time, and
the metadata as its JSON. The history asks the index for the rider's rides; a metadata file
whose size and time the index knows is served from the row, any other is read from disk
(rebuilt from the FIT file or completed with the analysis data where needed, and indexed as
it then is), and rows of files that are gone are dropped. What a session has read it keeps
in memory too, so the history, the records and the analysis, which all go through the same
list, cost one `stat` per ride after the first look.

Staleness is by size and modification time, not content hashes: every write ORQWA makes to a
metadata file (a name, an upload, a rebuild) changes at least the time, and a sync client
that replaces a file sets a new one. An index of another layout (`PRAGMA user_version`) or
missing is built afresh; a rider may delete it at any time. Without a usable index the
history reads the files as it did before, with a warning in the log. SQLite runs in WAL mode
with `synchronous = NORMAL`: a crash costs at most the last writes, which the files replace.

## Amendment 2026-10-17: devices per rider (R82)

The amendment for #257 left the remembered devices, the cadence's source and the shifter's
buttons in `settings.toml`, shared by every rider. Riders sharing a computer often bring
their own strap, pedals or shifter, so these now live with the rider in
`profiles/<id>/devices.toml` and follow them to another computer, like the HUD layout. A
device's system identifier differs between computers; its advertised name, already the
fallback when the identifier changed, finds it there.

`profiles::carry_over` gives the shared values once to every rider without a `devices.toml`
(the D-Fly channels from before #139 as button assignments), then removes them from
`settings.toml`. That file now keeps the active rider, the ride bar and the Courses view.

## Amendment 2026-11-04: separate ride and workout HUDs (#483, R207)

`profiles/<id>/hud.toml` retains `metrics` for the ride HUD and adds `workout_metrics` for
the workout HUD. A file without the new field supplies its existing figures to both uses.
The first save snapshots those figures into the workout field before changing either layout,
so editing the ride HUD cannot silently change a workout HUD that has never been edited.
Both layouts are written together atomically and follow the rider through file syncing.
