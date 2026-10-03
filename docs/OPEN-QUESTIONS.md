# Open questions and findings

Things to review or decide together. Newest first; remove entries once settled.

## 2026-10-03 — Course files (Phase 3c)

### Needs a decision

1. **Course files store inputs, not the pre-built world (deviation from ADR 0007).**
   A `.tqc` holds the GPX plus every terrain tile and map tile the course used. Opening it
   rebuilds route and world offline from those files — which also satisfies "rebuild with a
   newer generator" (R35) for free. Storing the pre-built world for an instant start needs a
   binary mesh format (own codec or a crate like `postcard`); I left it for a later step. OK,
   or do you want the stored world now? Measured: the 7 km Lake Biel course is 6.2 MB and
   opens offline on an empty machine in ~23 s with the debug build (release not measured).

### Findings / limits

- **History (PR #11)**: the history scans each rider's `rides/` folder and reads the JSON
  summaries; the SQLite index from ADR 0002 is not built yet — not needed until there are
  hundreds of rides or cross-ride queries (PRs, climbs).

- **Profiles (PR #10)**: rides are now saved per rider in `profiles/<rider>/rides/`; rides
  saved earlier in `rides/` are not moved. The setup screen's mass field is gone — mass comes
  from the profile (rider + bike). New dependency `toml` 1.1.6 (chosen in ADR 0004).
- **Lake Biel fixture** reports a steepest grade of 19 % — the elevation profile has a sharp
  spike around 2.3 km, probably a bridge/underpass the smoothing does not catch. Worth a look
  on a real ride.

- **Aerial imagery dropped** (PR #8): SWISSIMAGE draped on the terrain looked worse than the
  land-cover shading and was removed. PR #8 now only brings zoom 15 terrain and the
  first-person camera.
- **60 fps on M1** not yet measured (software renderer in the container only).
- **GitHub token** cannot read check results (`checks:read` missing), so I can't see CI status
  of PRs; I rely on `scripts/check.sh` locally.
- **PR order**: #6 (world details) → #7 (visual polish) → #8 (terrain, first person) → #9
  (course files) → #10 (rider profiles); each is based on the previous one. Merge in that order.
