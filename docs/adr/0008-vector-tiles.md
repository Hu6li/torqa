# ADR 0008 — Map data from OpenFreeMap vector tiles

- Status: accepted, supersedes [ADR 0006](0006-map-data.md)
- Date: 2026-10-02

## Context

ADR 0006 fetched OpenStreetMap data from the public Overpass API. A rider's 40 km route took
minutes to prepare and lost tiles: Overpass answered *429 Too Many Requests* and *504 Gateway
Timeout*, and mirrors timed out. Overpass is a shared query service, not meant for bulk downloads.
Measured on a 36 km test line: 344 s, 6 of 20 tiles failed.

## Decision

- Use **[OpenFreeMap](https://openfreemap.org)** vector tiles (`OpenMapTiles` schema, zoom 14),
  a free, keyless EU-run service on a CDN, decoded with `mvt-reader` (MIT).
- Same features as before: buildings (now with `render_height`), land cover and land use, parks,
  water polygons, waterways, roads for the minimap, and bridges/tunnels from the road `brunnel`
  attribute.
- Tiles are downloaded 6 at a time, cached under a versioned folder, and merged in tile order so
  the world is identical on every load. Buildings crossing tile borders are kept once (by id).
- The tile URL comes from OpenFreeMap's TileJSON (latest weekly planet snapshot); cached tiles
  stay valid offline.

## Consequences

- The same 36 km line now loads in **5 s cold, 1 s for map data** with no failures.
- Attribution: "© OpenFreeMap © OpenMapTiles · Data © OpenStreetMap contributors".
- Features crossing tile borders arrive clipped per tile; land cover and roads are unaffected,
  very large buildings may be cut at a tile edge.
- One data provider; VersaTiles (Shortbread schema) is a possible fallback if needed.
