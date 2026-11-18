# ADR 0041 — Building shape from map data

- Status: proposed (step 1 of 5: the measurement)
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

The plan, in five steps:

1. Measure how much of this OpenStreetMap has.
2. Place buildings on slopes as they are built (#554), in every world.
3. Use the heights the tiles already carry, for models as well as shells.
4. Keep building tags in the map detail extract.
5. Build roofs, parts, colours and materials from those tags.

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

Steps 2 and 3 go ahead as planned. Step 4 keeps `building=*` for every building, and the
shape tags, colours and materials for those that have them. Steps 4 and 5 decide the
format of the extract and are described when they are made.

## Open

- A national overlay (swissBUILDINGS3D first) behind a `BuildingSource` trait: worth it in
  Switzerland by these numbers, at the cost of a new server and a 3D format to read. This
  needs its own ADR.

## Consequences

- No world, course or network change in this step: the census reads a download the rider
  already has, or one fetched by hand, offline.
