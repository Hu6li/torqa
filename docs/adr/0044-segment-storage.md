# ADR 0044 — Segment storage

- Status: proposed
- Date: 2026-11-27
- Pull request: #682
- Amends: [ADR 0002](0002-sync-safe-storage.md)

## Context

Riders mark their own segments of a course (R302), and ORQWA records a time when they ride
the whole of one. Those times cannot be worked out again: the FIT file has the samples, but
not the segments as they were marked when the ride was saved. Bests are derived from them,
so losing them loses the rider's records.

The first version kept a ride's segment times in its metadata JSON (ADR 0002). Releases up
to v0.7.0 read that file into a fixed set of fields and write it back from those fields when
they rename a ride, so a rider who shares a data directory between two computers, or goes
back to an older ORQWA, would lose the times without notice. Raising the metadata's
`format` would not help: an older ORQWA treats a newer file as missing and rebuilds it from
the FIT file, losing the ride's name as well.

The data directory may be synced between computers by any file sync tool (R30), and the
history index (R79) decides from a file's size and modification time whether to read it again.

## Decision

A ride's segment times live in their own file beside its FIT file and metadata:
`<ride>.segments.json`, for `<ride>.fit`.

```json
{
  "format": 1,
  "segments": [
    { "name": "Wabern sprint", "start_m": 200.0, "end_m": 500.0, "time_s": 21.0,
      "avg_power_w": 780.0 }
  ]
}
```

- **Written once**, when the ride is saved, atomically (a `.part` file renamed into place),
  and only if the ride timed a segment. Nothing rewrites it afterwards: renaming, uploading or
  rebuilding a ride's metadata leaves it alone. A ride's own name for a segment is the one it
  had when ridden.
- **Left alone by older releases.** They list rides by their `.fit` files and read only the
  `.json` with the same name, so they never open, rewrite or count this file.
- **Versioned.** `format` is raised only for a change older readers would misread; added
  fields do not raise it, and readers pass over fields they do not know. A file of a newer
  format is not misread: the ride is listed without its segment times, and the file is kept.
- **Deleted with the ride.** Deleting a ride removes its FIT file, metadata and segment times.
  An older release deleting a ride leaves the segment times behind; they are never read
  without their FIT file, so they count for nothing.
- **Synced with the ride.** The history index keeps the segment times with the metadata it
  serves and knows the file by its own size and modification time, so times that a sync
  client brings in after the metadata are read when they arrive.
- **Reported when lost.** The metadata can be rebuilt from the FIT file, so a failure to write
  it is only logged. The segment times cannot, so when the FIT file is saved but they are not,
  the rider is told where they could not be written.

## Consequences

- Each ride that timed a segment has a third file. Rides without segments look as before.
- The history index's layout goes to 2: its rows carry the segment times and the second file's
  state. An index of layout 1 is built afresh from the files, as ADR 0002 provides.
- Segment times from development builds before this decision, kept in the metadata, are not
  read. No release wrote them.
- **Rejected: FIT developer fields.** The FIT file is the activity riders upload and export
  (R28, R29); it would carry ORQWA's private segment names to training platforms, and the
  times would still have to be read from every FIT file to list them.
- **Rejected: one results file per rider or course.** Two computers riding before they sync
  would each rewrite it, and a sync client cannot merge the copies (ADR 0002). One file per
  ride is only ever written by the computer that recorded it.
