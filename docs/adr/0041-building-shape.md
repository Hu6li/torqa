# ADR 0041 — Building shape from map data

- Status: proposed (step 1 of 7: the measurement)
- Date: 2026-11-18

## Context

Buildings come from the OpenFreeMap tiles (ADR 0008): an outline, `render_height` and
`colour`. Everything else is guessed. The kind of building comes from land use, size and
points of interest, and the roof is a random gable or hip from its recipe. Fitted models keep
their own height whatever the map says. On slopes the ground floor stands at the highest
ground, over a blank plinth down to the lowest (#554).

OpenStreetMap can say more: heights and levels, roof shapes, their direction and height,
colours and materials of walls and roofs, the kind of building (`building=*`), and buildings
drawn in parts (`building:part`, "Simple 3D Buildings"). The tiles drop all of that. The
Geofabrik region downloads (ADR 0033) carry it. National surveys (swissBUILDINGS3D, 3D BAG,
the German states' LOD2) have measured roofs for every building, but only in their country.

The plan, in seven steps. Steps 2 to 4 need no download and change every world:

1. Measure how much of this OpenStreetMap has.
2. Place buildings on slopes as they are built (#554).
3. Use the heights the tiles carry for models too, and draw buildings mapped in parts as
   their parts: the tiles carry `building:part` outlines with `render_min_height`, and flag
   the outline of a building with parts `hide_3d`.
4. Walls along the mapped outline of every building, and a roof computed over it.
5. Keep the kind of every building (`building=*`) in the map detail extract.
6. Keep the shape tags, colours and materials there too, and build from them.
7. Assemble buildings on their outlines from Blender parts (windows, doors, balconies,
   chimneys, dormers) instead of fitting whole models to rectangles.

National data is to come later as an overlay, if step 1 shows the map is thin where riders
ride.

## Measurements

`orqwa-cli building-census <download>…` counts the building ways of a download and the share
carrying each tag, in one pass over its ways and relations (dev container, 18 cores, release
build). The downloads are Geofabrik's of 2026-10-26 (DACH, Japan), 2026-10-28 (Finland) and
2026-11-18 (Switzerland). DACH is Germany, Austria and Switzerland together, mostly Germany.

| | Switzerland | DACH | Finland | Japan |
|---|---:|---:|---:|---:|
| Building ways | 2,833,034 | 46,485,800 | 4,381,776 | 30,928,440 |
| Building multipolygons | 11,145 | 71,214 | 1,274 | 24,494 |
| `building:part` ways | 16,080 | 490,928 | 20,889 | 161,575 |
| Height or levels | 5.6 % | 9.1 % | 7.7 % | 18.4 % |
| Any shape tag | 8.9 % | 11.0 % | 8.1 % | 18.7 % |
| `height` | 0.3 % | 0.6 % | 0.5 % | 16.3 % |
| `building:levels` | 5.5 % | 8.9 % | 7.6 % | 10.7 % |
| `roof:shape` | 6.3 % | 8.0 % | 1.5 % | 0.4 % |
| `roof:levels` | 3.2 % | 5.8 % | 0.7 % | 0.1 % |
| `roof:orientation` | 1.0 % | 1.5 % | 0.1 % | 0.0 % |
| `roof:colour` | 0.6 % | 1.3 % | 0.3 % | 0.4 % |
| `roof:material` | 0.6 % | 0.9 % | 0.1 % | 0.0 % |
| `building:colour` | 0.2 % | 0.6 % | 0.2 % | 0.1 % |
| `building:material` | 0.1 % | 0.6 % | 0.5 % | 0.0 % |
| Extract estimate | 73 MB | 1,437 MB | 96 MB | 1,498 MB |
| Read in | 4 s | 49 s | 5 s | 19 s |

`min_height`, `building:min_level`, `roof:height`, `roof:angle` and `roof:direction` are on
0.2 % of buildings or fewer everywhere; they are mostly on parts. The extract estimate stores
the outlines of the buildings with any shape tag and of every part, at 22 bytes a node and
90 bytes a way. Today's map detail extract of Switzerland is 36.7 MB.

The most common roof shapes are gabled, flat, hipped, half-hipped, skillion and pyramidal:

| Region | Gabled | Flat | Hipped | Half-hipped | Skillion | Pyramidal |
|---|---:|---:|---:|---:|---:|---:|
| Switzerland | 113,051 | 42,903 | 14,742 | 3,610 | 2,266 | 2,236 |
| DACH | 2,485,228 | 923,384 | 232,726 | 88,059 | 93,777 | 41,950 |

In Switzerland and DACH, gambrel, mansard, saltbox, round and dome are each below 1 % of
the mapped shapes. Japan maps few roofs, but 5 % of those are round.

`building=*` names more than the shape tags do. In Switzerland 67 % of buildings say only
`yes`, but 175,851 are `residential`, 140,319 `apartments`, 130,972 `house`, 76,063
`garage`, 60,340 `roof` (carports and shelters, a roof on posts), 43,906 `shed`, 33,641
`barn` and 26,170 `farm`. In Japan, 16 % of buildings have a measured `height` but hardly any
a roof shape.

In the 3,104 zoom-14 tiles of the courses prepared on the author's computer, 188 of 9,863
building features (1.9 %) are outlines flagged `hide_3d` and 916 (9.3 %) start above the
ground. ORQWA read neither, so a building drawn in parts was drawn twice: its outline and
its parts, all from the ground. The tile with Bern's old town has seven such outlines.

## What this means

- **The map describes about one building in ten.** Switzerland's shape tags cover 9 %, and
  a height or levels 6 %. Most buildings keep the guessed kind, height and roof whatever an
  extract holds. Steps 2 and 3, which need no extract, change what every rider sees.
- **Where tags exist they are the right ones.** Roof shape and levels lead, then direction
  and roof colour. Six roof shapes cover nearly all that is mapped.
- **The kind of building is worth keeping for every building**, not only the described
  ones: garages, sheds, barns and carports (`roof`) are drawn today as small houses with
  walls. That makes the extract larger than the estimate above.
- **Colours and materials are rare**, at 1 % or less. A palette fit stays the rule.
- **Parts are few but tall.** Churches, towers and landmarks are drawn in parts, with
  heights, so step 5's parts matter more than their count suggests.
- **National data would fill the other nine in ten** in its own country: swissBUILDINGS3D
  covers every Swiss building with its measured roof. Whether to add it, and where first, is
  open (see below).

## Decision so far

Steps 2 to 4 go first, from the tiles alone. Step 5 keeps `building=*` for every building,
as a point and a kind, which is small; step 6 adds the shape tags, colours and materials of
the buildings that have them. Each step is described here as it is made.

## Open

- A national overlay (swissBUILDINGS3D first) behind a `BuildingSource` trait: worth it in
  Switzerland by these numbers, at the cost of a new server and a 3D format to read. This
  needs its own ADR.

## Step 2: buildings on slopes (2026-11-18, R257)

A building on a slope is set into it, as hillside houses are dug in: its ground floor lies at
the mean ground under its footprint, buried no more than `SET_IN` (2.4 m, about a storey) on
the uphill side. The terrain stays as it is: there it hides the walls' lower part, so the
building looks dug in without a pit or platform being shaped. Before, the floor stood at the
highest ground, so buildings on slopes stood a storey or more too tall from below (#554).

A wall whose ground, at its higher end, lies over a storey's window sills (1 m over its floor)
shows no windows in that storey (`Builder::facade`), so none is seen half sunk in the ground;
side walls across the slope lose their lowest windows where the uphill end buries them.

Downhill each wall reaches down to its own ground and shows as many lower storeys, up to four,
as fit with their windows clear of it: a storey whose floor lies up to 1 m below the ground
still shows, as a semi-basement's windows do. They have the windows of the building's ground
floor. Only the rest is plinth, so windows start within a storey of the ground on every
downhill wall. The shell shader draws windows from 1 m above its texture's zero, so lower
storeys lift their texture rows by 30 m, a whole number of storeys and of every wall
texture's repeat: rows and texture run on unbroken.

Fitted models, whose doors may face uphill, are set in no more than 1 m (`MODEL_SET_IN`), and
are used where the ground falls less than 1.8 m below their floor: their 3 m basement is a
plain stone plinth. Steeper plots get shells with lower storeys. The shell drawn far from a
model keeps the model's plinth, so the two still match when they swap.

In the tests, three houses lie across a 10 % slope:

| House | Fall | Set in | Downhill windows over the ground, on main → now |
|---|---:|---:|---:|
| 24 m | 2.4 m | 1.2 m | 3.4 m → 2.2 m |
| 36 m | 3.6 m | 1.8 m | 4.6 m → 2.8 m |
| 60 m | 6.0 m | 2.4 m | 7.0 m → 1.6 m, over a lower storey |

No uphill window lies under the ground. Generator 43; the stored format stays 3.

## Step 3: parts and mapped heights (2026-11-18, R258)

The tiles' `building` layer carries `building:part` outlines with `render_min_height`, and flags
the outline of a building with parts `hide_3d`. ORQWA now reads both (`Building::in_parts`,
`Building::min_height`). After churches, landmarks, purposes and shops are marked, each part
whose centre lies in an outline drawn in parts is drawn as that outline's part, standing on the
outline's ground, set in as step 2 sets a building in; the outline itself is no longer drawn, but stays a plot, so plants
and props still keep off it. A church, castle or lighthouse keeps its outline drawn whole, as
its builders make towers and keeps of their own, and its parts are left out. Parts are never
fitted models. A part starting above the ground has walls only from there, an underside facing
down, and floor bands and balconies only above it; one starting within half a storey of its
top is left out.

Mapped heights now reach fitted models. Where the map gives a height or levels, the walls it
means (`wall_height`) must be met by the model's eaves within 15 %, and the model's instance
is scaled up or down to them; models that cannot are not fitted, and the building is
generated instead. A model's whole storeys sit within 2 % of the generator's, so the effect is
mostly the number of storeys: a house mapped three storeys high no longer gets a two-storey
model. Unmapped buildings keep their models' heights. The instance transform already held a
vertical scale, so the stored format stays 3; generator 44.

## Step 4: outlines and roofs over them (2026-11-18, R259)

Shells were drawn as the rectangle around their outline wherever it filled 80 % of it, and
other outlines got a band of roof with a flat top. Over the outlines in the 3,104 cached
tiles, 746,540 of at least 20 m², the fill is:

| Fill of the rectangle | Share |
|---|---:|
| Below 80 % | 14.5 % |
| 80–90 % | 16.9 % |
| 90–95 % | 14.0 % |
| 95–97 % | 16.4 % |
| 97 % and more | 38.2 % |

Now a shell is drawn as its rectangle only from 95 %, which keeps gable roofs, balconies and
chimneys on the 55 % that are nearly one; a fitted model, which squares the outline on its
rectangle, stands only from 90 %. Every other shell's walls follow its outline, and its
pitched roof comes from the outline's straight skeleton (`buildings::skeleton`): each wall's
roof face rises at the design's pitch to the hips, valleys and ridges where faces meet,
flatter where the roof would rise more than the design allows, with the same overhanging
eaves as other roofs. Designs still choose the roof as before; a gable over a non-rectangle
becomes hipped.

The skeleton is our own sweep, with no new dependency: edges move inwards, corners follow
their bisectors, an edge whose corners meet is gone and a reflex corner reaching another edge
splits the outline. Building outlines put many events at one place and time (a T's inner
corners and ridges), so where the outline as it is fails, the sweep runs on it moved by a
ten-thousandth of its size and the faces are put back on its corners. Every result is checked:
the faces must cover the outline exactly and every point lie at its height from its edge, or
the roof falls back to the band, then flat. Over all 815,606 building outlines of those tiles,
with up to 256 corners, every outline but seven got a checked skeleton, in 1.3 s together
(slowest 5 ms; dev container, release build).

Generator 45; the stored format stays 3.

## Step 5: what buildings are (2026-11-18, R260)

The map detail extract keeps, for every building whose `building=*` names a kind that
changes what is drawn (`BuildingKind`: house, apartments, farmhouse, barn, garage, carport,
shed, church, industrial, commercial, public, hotel), one point and that kind, in the cell of
the point: halfway between its first corner and the one halfway round, which lies within the
outlines of buildings. `yes`, `residential` and the like say no more than the tiles and are
not kept. When a course is prepared, each tile building takes the kind at a point within its
outline (`MapData::apply_building_kinds`).

The world puts the mapped kind before what size and setting suggest, after churches and
landmarks marked by points of interest: garages, sheds and barns are windowless sheds,
carports and shelters a new `Canopy` (a flat roof 2.5 m up on posts at most 4 m apart),
churches churches, industrial buildings halls, commercial buildings stores or offices,
public buildings and hotels such. Houses, apartments and farmhouses stay homes as their
region and elevation build them, apartments over 150 m² (or 9 m) blocks.

Measured with `orqwa-cli detail` (dev container, release build, process peak):

| Region | Building kinds | Extract before → with them | Peak memory before → with | Time |
|---|---:|---:|---:|---:|
| Switzerland (2026-11-17) | 718,828 | 36.9 → 69.0 MB | 76 → 119 MB | 7 s |
| DACH (2026-10-25) | 13,365,330 | 609 → 1,205 MB | 933 → 1,794 MB | 83 → 95 s |

Each building held while the file is read takes 16 bytes, its kind packed into its first
corner's node id. DACH is among the largest downloads offered; on tablets, smaller regions
(a country's parts) keep the preparation within their memory.

What extracts hold is now versioned in each region's record (`Region::content`, `CONTENT`
2) rather than by raising the extracts' directory, `detail-v1`: courses carry the cells they
used under that directory, and a course opened where its region is not downloaded must still
find them. A region whose extract holds less is not ready, so ORQWA prepares it again from its
download at start-up (ADR 0033); cells without buildings read as buildings the map says
nothing more of. An extract's name ends in what it holds (`switzerland-2026-10-24-c2`),
so the extract made again from the same download has a new name: a course in the library
that carries the older one takes it when next opened, as after an update (R144 compares
extracts by name).

Generator 46; the stored format stays 3.

## Step 6: roofs, colours and materials from the map (2026-11-18, R261)

The extract keeps, beside a building's kind, what its tags say of its shape and look
(`orqwa_osm::Shape`): `building:levels`, `roof:shape`, `roof:height` (or `roof:levels`
× 2.8 m), `roof:orientation=across`, `roof:colour`, `building:colour`, `building:material`
and `roof:material`, for buildings and building parts that have any of them. Region records
go to content 3, so regions are prepared again.

The world lets them override the recipe (`buildings::mapped`): the roof becomes the shape
mapped, with recipes for flat roofs given a pitch, eaves and tiles where the map says the
roof is pitched; a mapped roof height becomes the rise (a steep pitch limited to it); levels
set the storeys through the existing wall heights. Pyramidal roofs, and domes as pyramids,
rise from every wall to one point over convex outlines; roofs of one slope (`skillion`) rise
across a rectangle over walls rising with them. Half-hipped and mansard roofs are built
hipped, gambrels gabled. A ridge mapped across a rectangle turns it. Fitted models stand
only where their roof is the one mapped and no roof height or orientation is given.

Mapped colours no longer snap to the nearest palette colour (ADR 0020's palette fit): they
keep their hue, with lightness 0.62–0.92 and saturation at most 0.4 on walls (dark walls
stay out, as before) and lightness 0.12–0.55 and saturation at most 0.6 on roofs. Materials
choose styles (timber boards, metal cladding, bands of glass; sheet or slate roofs) and,
where no colour is mapped, colours (brick, stone, concrete).

Generator 47; the stored format stays 3.

## Consequences

- No world, course or network change in this step: the census reads a download the rider
  already has, or one fetched by hand, offline.
