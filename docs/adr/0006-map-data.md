# ADR 0006 — Map data from OpenStreetMap

- Status: superseded by [ADR 0008](0008-vector-tiles.md)
- Date: 2026-10-02

## Context

The 3D world (R16) needs land cover, buildings, water and the structures the road runs on.
Bridges and tunnels also matter for physics: terrain models are bare-earth, so without them a
bridge reads as a dip into the valley and a tunnel as a climb over the mountain. The data must be
free, need no account and work offline once fetched (R3).

## Decision

- Source: **OpenStreetMap** via the public **Overpass API**, with mirror fallbacks
  (overpass-api.de, maps.mail.ru, private.coffee).
- Queried in 0.05° tiles covering a 1.5 km corridor around the route, restricted to features
  Torqa draws (buildings, selected landuse/natural values, waterways, highway bridges and
  tunnels). Tiles are cached on disk under a versioned folder and reused across routes.
- Fetched at route import: bridges and tunnels shape the elevation profile (straight between the
  ends of each structure, matched by distance and direction so roads passing over or under the
  route are ignored). The rest feeds world generation.
- Missing map data never blocks a ride: the route then follows the terrain and the world is drawn
  without OSM features.

## Consequences

- Attribution "© OpenStreetMap contributors" (ODbL 1.0) is shown on the ride screen.
- First import of a new region downloads several MB per tile (e.g. ~19 MB and ~30 s for the two
  tiles around the Gurten including part of Bern); later imports are instant.
- Overpass is a shared community service: requests are sequential, identified by User-Agent and
  cached forever. If usage grows, self-hosting extracts (e.g. Geofabrik) is the next step.
