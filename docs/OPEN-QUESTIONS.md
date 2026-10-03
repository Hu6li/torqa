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

- **Aerial imagery dropped** (PR #8): SWISSIMAGE draped on the terrain looked worse than the
  land-cover shading and was removed. PR #8 now only brings zoom 15 terrain and the
  first-person camera.
- **60 fps on M1** not yet measured (software renderer in the container only).
- **GitHub token** cannot read check results (`checks:read` missing), so I can't see CI status
  of PRs; I rely on `scripts/check.sh` locally.
- **PR order**: #6 (world details) → #7 (visual polish) → #8 (terrain, first person) → #9
  (course files) → #10 (rider profiles); each is based on the previous one. Merge in that order.
