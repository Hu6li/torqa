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

The segments themselves, the rider's definitions, were one list per rider and course. Two
computers changing segments before they sync would each rewrite that list, and the sync
client would keep one and leave the other as a conflict copy. A segment is also only a pair
of distances along the course: when the same GPX is prepared again with different map or
elevation data (snapped to other roads, say), the distances can land somewhere else, and a
time there would be compared with times on another stretch of road.

## Decision

### Segment times

A ride's segment times live in their own file beside its FIT file and metadata:
`<ride>.segments.json`, for `<ride>.fit`.

```json
{
  "format": 1,
  "segments": [
    { "id": "4f0c9a1e2b7d48c6a3e5f1029b8d7c6e", "name": "Wabern sprint",
      "revision": "9e1c03b7a5f24d68", "start_m": 200.0, "end_m": 500.0, "time_s": 21.0,
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
- **Keyed to the stretch, not the name.** Each time keeps the segment's id and the prepared
  route's revision it was ridden on, beside its ends.

### Segment definitions

Each segment is a file of its own, `profiles/<rider>/segments/<route>/<id>.json`:

```json
{
  "format": 1,
  "id": "4f0c9a1e2b7d48c6a3e5f1029b8d7c6e",
  "name": "Wabern sprint",
  "start_m": 200.0,
  "end_m": 500.0,
  "revision": "9e1c03b7a5f24d68",
  "earlier": [{ "revision": "51d2e08f3c6a9b47", "start_m": 195.0, "end_m": 495.0 }],
  "added_ms": 1795795200000
}
```

- **A stable id.** 128 random bits in hexadecimal, from the standard library's randomly keyed
  hasher, so two computers never pick the same one and no dependency is added. The id never
  changes: renaming or moving a segment rewrites only its own file, and removing it deletes the
  file. Segments added, changed or removed on two computers before they sync come together as
  separate files. A conflict copy of one file (the same id under another file name) is read
  once, preferring the file named after the id, and removing the segment deletes both.
- **The route's revision.** `orqwa_routes::Route::revision` is a fingerprint of the prepared
  points (positions, elevations and distances to the centimetre), unlike the route key, which
  comes from the GPX alone. A segment keeps the revision its ends were set or last checked on.
- **Checked by the rider when the course comes out differently.** A segment whose revision is
  not the course's is still listed, marked to be checked, but not timed or told, nor drawn on
  the profiles and maps, where it may lie on another stretch now. The rider opens it, sees on
  the profile that its ends lie where they did (or moves them) and saves: it takes the new
  revision, and its old placement goes into `earlier`, so its times so far keep counting.
  ORQWA cannot move the ends itself: whether the segment still starts at the same bend is the
  rider's call, and a guess would compare times on different roads without anyone noticing.
- **Times compare by stretch.** A time counts towards a segment's best if its revision and ends
  (to the centimetre) are where the segment lies now or lay on an earlier revision. Moving a
  segment on the same revision starts its bests afresh and drops `earlier`; renaming changes
  nothing. A time whose segment is gone still compares with others on the same stretch.
- **Versioned like the times.** Added fields do not raise `format`; a file of a newer format,
  or one that does not read as a segment, is left out and left alone.

## Consequences

- Each ride that timed a segment has a third file. Rides without segments look as before.
- The history index's layout goes to 2: its rows carry the segment times and the second file's
  state. An index of layout 1 is built afresh from the files, as ADR 0002 provides.
- Segment times from development builds before this decision, kept in the metadata, are not
  read, nor are segments kept in the earlier `segments/<route>.json` list. No release wrote
  either.
- Preparing a course again with other data asks the rider to check each of its segments once.
  A course prepared the same way gives the same revision, so nothing is asked.
- A course has up to 100 small files, one per segment, read when its page or a ride opens.
- **Rejected: FIT developer fields.** The FIT file is the activity riders upload and export
  (R28, R29); it would carry ORQWA's private segment names to training platforms, and the
  times would still have to be read from every FIT file to list them.
- **Rejected: one results file per rider or course.** Two computers riding before they sync
  would each rewrite it, and a sync client cannot merge the copies (ADR 0002). One file per
  ride is only ever written by the computer that recorded it.
- **Rejected: matching a segment to the rebuilt route automatically**, by projecting its old
  ends onto the new points. It would usually be right, and wrong without a word where the
  route changed most, beside the roads the new map data moved.
- **Rejected: comparing times by segment name**, as the first version did. Two segments can
  share a name, and a renamed one would lose its bests.
