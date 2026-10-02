# ADR 0009 — Swiss aerial imagery and high-resolution terrain

- Status: accepted
- Date: 2026-10-03

## Context

Land cover coloured from OpenStreetMap reads as a game, not as the real place. Google
Photorealistic 3D Tiles were rejected: no offline use or caching allowed, unavailable for
applications in the EEA, billed per user and blurry at street level. Switzerland publishes its
national geodata as free open government data.

## Decision

- **Imagery**: swisstopo **SWISSIMAGE** orthophotos via the federation's public WMS
  (`wms.geo.admin.ch`, layer `ch.swisstopo.swissimage`, EPSG:4326, JPEG), one image per 480 m
  world chunk: 1024 px (~0.5 m per pixel) for chunks along the route, 256 px further out.
  New crate `torqa-imagery`; images are cached on disk like terrain and map tiles, so prepared
  routes ride offline. Only requested for chunks inside Switzerland; elsewhere the OSM land-cover
  colours stay.
- **Draping**: terrain and roofs sample the chunk's photo by world position. Photos are darkened
  and re-saturated slightly because they already contain sunlight and haze; fine noise adds grain
  where the camera is close. Full-size textures exist only for chunks within 900 m of the
  camera, the rest use 256 px copies, which bounds video memory on long routes.
- **Terrain**: Mapterhorn at zoom 15 (~1.6 m per pixel, swissALTI3D in Switzerland) is tried
  first, falling back to Mapterhorn zoom 12 and AWS Terrain Tiles (ADR 0005).

## Consequences

- Roads, fields, parking lots and roofs match reality from above; at eye level the ground is
  still soft (0.5 m per pixel). SWISSIMAGE offers 10 cm; higher resolution along the road only is
  a possible next step.
- Trees and walls are still generated; real 3D buildings (swissBUILDINGS3D) are a separate step.
- Attribution: "Imagery © swisstopo".
- The WMS is a public service without published rate limits; downloads run 4 at a time.
