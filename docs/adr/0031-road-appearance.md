# ADR 0031 — Mapped road hierarchy, surfaces and cross-sections

- Status: accepted
- Date: 2026-10-23
- Requirements: R119, R89, R35, R101

## Context

All ridden routes were six-metre asphalt roads with edge and centre lines. Surrounding
motorways, trunk roads, primary and secondary roads shared one width; all tracks and paths
were gravel. Paved cycleways became gravel beside the route and wide carriageways when ridden.
The existing OpenFreeMap transportation tiles already contain more information, bundled in
each prepared course. Their surface field normally distinguishes only paved and unpaved;
surveyed width and lane counts are absent.

## Decision

- Retain road hierarchy, surface, one-way and ramp tags separately from the existing broad
  routing classes. Do not change route snapping priorities in this change.
- Use one Rust appearance policy and one shader for ridden and surrounding ways. Explicit
  paving overrides road class. Unknown surfaces use deterministic visual defaults; they are
  not recorded as surveyed materials. Specific material tags are accepted when supplied,
  but current tiles cannot reliably distinguish asphalt, concrete or cobbles.
- Estimate motorway carriageways at 10.5 m, one-way trunk roads at 8.5 m, two-way trunk roads
  at 8 m, primary/secondary/tertiary roads at 7/6.5/6 m, local streets at 5.5 m, service roads
  at 3.5 m, tracks at 2.8 m, cycleways at 2.5 m and other paths at 1.6 m. Ramps are 4.5 m;
  ordinary one-way streets are narrower. These are appearance defaults, not measurements.
- Motorway carriageways have shoulders and long dashed lane dividers. Through roads have
  centre and edge lines, one-lane ramps edge lines only, local roads and paths no inferred
  paint. Only unpaved tracks get a grass centre strip. Keep mapped carriageways separate:
  do not create a duplicate carriageway or median from an offset of the line.
- Match route segments spatially to nearby, aligned map segments on the same bridge/tunnel
  level. Prefer the nearest aligned way so a parallel cycleway retains its own appearance.
  Unmatched route pieces keep a six-metre asphalt fallback. Ease changes of width along the
  route. Terrain, junctions, vegetation clearance, bridge decks, sidewalks and roadside props
  follow the drawn widths. Motorways, trunk roads, ramps and unpaved ways get no inferred
  urban sidewalk.
- Suppress surrounding road geometry only when its whole cross-section is covered by an
  aligned ridden road on the same structure level. Retain the transition segment into that
  covered section; shallow branches must meet the ridden road without a grass gap. Nearby
  parallel paths keep their visible surface. Open the ridden road's shoulder at these mouths.
- Reuse existing own-work textures. Road vertex colours encode material, width, marking
  style and estimated lane/track flags. Geometry without an across-road coordinate, such as
  junction corners and round caps, has no painted markings. Shore strips appended to the
  unpaved-road mesh carry complete plain-gravel metadata too; a partial colour array would
  cause Godot to reject the combined mesh.
- Add **no provider, network request, dependency or preparation download**. Decode the saved
  tiles at generation; course opening and regeneration remain offline. Course files and their
  format are unchanged. Detailed raw OSM enrichment needs a separate bounded preparation
  design and measurements; do not reinstate the unreliable public Overpass bulk download
  rejected in ADR 0008 or fetch missing surface details at ride start.

## Kept worlds and validation

Generator **10**, stored format **3**. The new vertex-colour meaning requires rebuilding
older kept worlds; the stored-world fixture records the contract. Generator 9 introduced the
road profiles; 10 also offers regeneration for the junction and shore-material corrections.
Course inputs stay valid.
Tests cover retained tags, hierarchy and paving overrides, route matching across neighbouring
ways and levels, narrow ridden cycleways, surrounding motorway widths, mesh attribute lengths,
shallow junction continuity across chunk boundaries, nearby parallel paths, and existing
junction, terrain, urban and stored-world behaviour. Render comparisons cover
local streets, main roads and cycleways, plus the bundled Twann–Chasseral course near
Lamboing at 4.2 and 4.6 km with networking disabled. Native M1 frame time remains a manual check.

## Limits

Lane count, paint and width are inferred from hierarchy and direction. National marking
rules, surveyed lane layouts, exact surface materials, medians and motorway barriers are not
reconstructed from tags that the provider omits. Some mapped data is missing or inaccurate;
the renderer makes a plausible world without claiming a surveyed road model.

## Amendment — surrounding road earthworks (2026-10-25, R141)

Surrounding roads were draped on the terrain, tilted with every hillside. Ground vehicle ways,
unpaved service roads, tracks and cycleways now get a shared elevation profile and a level road
bed. Generic paths and footways remain draped; surface paving alone does not turn a hiking path
into an engineered road.

Profiles sample one cached global terrain lattice before chunks are built. They smooth local
humps over 24 m and keep intermediate grades within 20 %, preserving the terrain height at
shared chain, tag-change and branch endpoints; where those endpoints need a steeper grade, the
connection wins. Closed loops keep their terrain datum for roundabout islands. A way joining
the ridden road takes the ridden road's own ground at its edge as its datum there.

**One generic priority rule** decides every conflict, instead of per-case fixes: the ridden
road, then railways, then surrounding ways. Each way proposes ground: the ridden road and
railways their existing earthworks (unchanged from before), a surrounding way its bed, earth
banks and, on steep hillsides, a battered face. The proposals combine as follows.

- **Hard ground.** The ridden carriageway with its verge, the cover over a shallow tunnel tube
  and the ground before, around and over the start of each portal keep exactly the ground
  the ridden road gives them. A surrounding way yields there: its earthworks do not apply,
  and its ribbon follows the ground it is left. At a portal this cuts the way's approach
  naturally beside the headwall instead of notching the hill or moving the portal. Portal
  placement itself sees that same ground, so the portal stands where it stood without the
  way. Lengthening a tunnel over a way crossing its approach would be the structural
  alternative; the saved Alpenbrevet Gold course has no such crossing, so it is not needed.
- **Soft ground.** Beyond the hard ground the ridden road's embankments and cuttings yield to a
  surrounding way's bed by how firmly the way holds the point: fully on its bed, fading
  across its banks and towards the ridden verge. Every term reaches zero at the end of its
  own reach, so no step opens where one earthwork meets another.
- **Shadow.** Ground behind a surrounding way, seen from the ridden road, is released from the
  ridden road's earthworks: an embankment ends at a lower road instead of burying it or
  rising again as a berm beyond it.
- **Room.** The ridden verge keeps its full width unless a way runs so close that both cannot
  fit; it then narrows only as far as that way needs, never past the margin that keeps every
  terrain piece touching the carriageway below it.
- **Detail where seen.** Beds are formed within 120 m of the ridden road. Farther away tracks
  and cycleways follow the terrain as before: formed shelves there gained nothing riders see,
  and Godot's simplified distant ground swallowed their ribbons. Paved roads keep a plain
  shelf without walls, on three pieces a side rather than six (amended 2026-11-10, for the
  memory of densely mapped courses: Wada's world 20 % smaller); cells split for the ridden
  road, water or a chunk's edge keep six. Cells are split finer than the usual six pieces a
  side only beside a close bed at another height than the ridden road or a railway (24, or 48
  within 3 m; a joining road at their level needs none) and under walls or narrow shelves on
  steep hillsides (12). The saved Alpenbrevet Gold world has 17 % more ground triangles than before
  R141 (streets 17 %, tracks 13 %) and takes about 30 % longer to build, against 79 % more
  ground triangles and about twice the time for refining every close or steep piece at 24.

Pieces of the ridden road's own mapped line never form a bed, whatever structure tags either
source carries: map and route tags change a few metres apart at bridges and tunnels, and such
a remnant had formed a second road with walls on Alpenbrevet's gorge rock at 36.40 km.

Tracks, unpaved service roads and cycleways keep earth banks. A level shelf as wide as the
usual terrain pieces need (about 4 m beyond the edge) cut tall bare scars into steep meadows,
strewn with boulders, so across hillsides steeper than 0.35 these ways get finer pieces and a
shelf of about 2 m. Their banks rise and fall at 0.7, short of the slope where the terrain
shows earth through grass, or just steeper than a steeper hillside so they meet it within
their reach; boulders beside a formed way follow the natural hillside's slope, not its
banks'.

Concrete faces use the completed terrain triangles and the existing zero-alpha structure tint,
so they share openings and chunk edges and need no shader or storage change. They dress only
steep banks a paved vehicle road's own earthworks moved by more than 0.3 m and that the road
holds; natural rock, track and cycleway banks and the ridden road's ground stay bare.

Generator **36** offers regeneration; stored format **3** and course inputs are unchanged.
Tests cover the intent: the ridden roadside unchanged beside a track, no step or berm beside a
lower road on a steep hillside, a portal keeping its hillside while a track climbs beside it,
no bed from the ridden road's own mapped pieces at tag changes, walls only on paved roads and
refinement only beside close carriageways, besides the level beds, junction datums, railway
neighbours and tunnel approaches. The saved full Alpenbrevet Gold course is the visual
reference (32.40, 35.75, 36.40, 40.07 and 54.60 km), compared with the build before R141.

## Amendment — local traffic lanes (2026-10-23, R120)

The avatar rides halfway into the local half of the carriageway. A spatial width index reads
the flat road mesh between its bevels, interpolates width changes and distinguishes stacked
roads by elevation. This follows the road actually drawn, including kept worlds and a road
ridden again in the opposite direction. Ghosts stay nearer the centre on that same side;
on very narrow paths their translucent models may overlap.

Country-border lookup retains the country at each route point, including short stretches and
repeat crossings. ISO country/territory defaults determine the traffic side, independently of
language and steering-wheel position. Border tiles used on GPX preparation are recorded with
the other course inputs. Course opening reads the existing cache offline; unavailable or
unknown countries keep right. Bundled, simplified outlines distinguish Hong Kong, Macau and
the US Virgin Islands from their parent countries, whose traffic side differs (ISC data,
credited in `docs/CREDITS.md`); these need no downloads. Road-specific exceptions are not
inferred. An older course lacking border tiles uses its saved country when it knows exactly
one; otherwise unknown stretches keep right until the border inputs are available.

Only avatar coordinates change. The original centre-line route still drives recording,
physics, distance, video synchronization and the minimap. Geometry and its stored contract
are unchanged, so generator 10 / format 3 remain valid and no world regeneration is needed.

## Amendment — surfaces from map detail (2026-10-25, ADR 0033)

The tiles still tell only paved and unpaved. Where a course was prepared with a region's map
detail ([ADR 0033](0033-map-detail.md)), each road takes the surface of the mapped way along
it: the same kind of road, within 4 m, aligned (cosine at least 0.9), at most of three points
along each segment, and only a surface the tile's paved or unpaved agrees with — a paved
sidewalk beside an asphalt street, or a dirt surface on a paved road, changes nothing. A
road is split at its own points where the surface changes; pieces under 5 m join their
neighbour, so junctions leave no slivers. The detail is applied after the route is matched,
which still reads the roads as the tiles have them. The rule above stands: the detail comes
from the cache and the course file, never from a download on preparation, opening, ride start
or regeneration. Courses without detail come out as before, so the generator stays.
