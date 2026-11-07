# ADR 0030 — River surfaces independent of bank terrain

- Status: accepted
- Date: 2026-10-22

## Context

Rivers copied the shaped terrain at every water vertex. On Alpenbrevet Gold, steep valley
sides and road embankments became towering water slopes. Subtracting a fixed channel depth
could not correct the water surface or keep its bed submerged (R118).

## Decision

Build water elevations once, before generating chunks, from the course's existing terrain
and map inputs. Sample mapped waterway centre lines every 10 m and interpolate elevations
along them, keeping each cross-section level. Infer downstream direction from endpoint
elevations and pool conflicting elevations with isotonic regression to remove uphill DEM
bumps. Missing elevations fall back to the route's heights.

Water polygons use the profiles of centre lines inside them. Without a centre line, an
elongated area gets a profile through the middle of cross-sections along its principal
axis; a broad pool uses one interior elevation. This fallback is an approximation: the
map currently preserves neither lake/river classification nor a complete river network.

Water elevations are shared across chunks and are independent of road and street shaping.
The terrain's bed follows the same datum, widening the bank correction as needed (up to
40 m) to avoid vertical cuts. Exposed steep channel sides use the existing triplanar rock
material, with normals taken from the carved terrain and shared coarse borders to prevent
lighting seams. Ground crossings retain their culverts and bridges retain open channels.
The existing gravel shore band continues to follow the bank terrain.

## Consequences

Rivers can descend along the valley without climbing its sides. Polygon water and an
overlapping symbolic stream use the same datum. World generation does additional cached
terrain sampling and spatially indexes profiles; no new service, dependency or download
source is needed. This is visual reconstruction, not a hydraulic simulation.

Generator 8 offers regeneration for older kept worlds (R101, ADR 0025). The stored layout
stays at format 2; existing course files remain usable offline.

## Lakebeds around stream lines (2026-11-06)

A mapped stream passing through a lake could override the lake's interior channel distance.
Far from a shore, the nearby stream's positive distance suppressed bed carving beside its
narrow channel. This exposed broad strips of lakebed above the water, with stepped edges
following the stream index cells.

Inside mapped water areas, channel distance now uses only area shores. Without a nearby
shore, the existing deep-water bed applies even when a stream line is indexed nearby.
Standalone streams, water elevations, gravel shores and ground-crossing clearance keep
their existing rules. Generator 40 offers regeneration; stored format remains 3.

The lake meshes from Moorgarte's saved inputs at 81.17 km show the exposed strips
[before](../images/lake-bed/moorgarte-before.jpg) and the submerged bed
[after](../images/lake-bed/moorgarte-after.jpg), with the same camera and materials.
