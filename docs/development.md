# Development

How to build, check and change ORQWA. Contributor conventions are in [CLAUDE.md](../CLAUDE.md),
architecture decisions in [adr/](adr/).

## Setup

All tooling runs in a container — nothing is installed on your machine.

1. Install [Docker](https://www.docker.com/) and [VS Code](https://code.visualstudio.com/) with the
   [Dev Containers](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers) extension.
2. Open this folder in VS Code and choose **Reopen in Container**.

Without VS Code, `scripts/dev.sh <command>` runs any command in the same container; on Windows,
from Git Bash. The checkout uses LF line endings everywhere (`.gitattributes`): the container
runs the scripts and the Dockerfile from the checkout, and `sh` rejects CRLF. A clone made
before that setting may hold CRLF files; `git ls-files --eol | grep w/crlf` lists them, and
`git add --renormalize .` after a clean `git checkout` puts them right.

The containers run as root. With rootful Docker on Linux, a WSL clone included, what they write
into the checkout would stay owned by root, so `scripts/dev.sh` and `scripts/art.sh` hand those
files back to you afterwards (Docker Desktop and rootless setups need nothing and get nothing).
The VS Code devcontainer does not; after working in it on such a host, one `scripts/dev.sh true`
does the same.

## Tasks

| Task | Command (inside the container) |
|---|---|
| All checks (fmt, clippy, tests, cargo-deny, licence notices, translations, gdlint, gdformat, Godot smoke tests), as CI runs them | `scripts/check.sh` |
| Only the checks the branch's changes since main can affect (committed or not), for quick turns while working; the full run before a PR | `scripts/check.sh --quick` |
| Build the GDExtension into `app/bin/` | `scripts/build-gdext.sh [debug\|release]` |
| Render screenshots of the start page, a ride, the summary and the analysis (software Vulkan, with 16 weeks of made-up rides) into `screenshots/` | `scripts/screenshots.sh` |
| Render only the English/German error-details views, without preparing a route | `SCREENSHOT_ERRORS_ONLY=1 scripts/screenshots.sh` |
| Render only Graphics settings, without preparing a route or kept world | `SCREENSHOT_GRAPHICS_ONLY=1 scripts/screenshots.sh` |
| Render English/German group setup and connection popups, activity previews, late entry, shared workout controls and organizer consent, with local-only hosting and presentation fixtures | `SCREENSHOT_GROUP_ONLY=1 scripts/screenshots.sh` |
| Render seven group avatars, distance gaps and map/profile markers on an offline course using presentation fixtures | `SCREENSHOT_GROUP_RIDERS_ONLY=1 scripts/screenshots.sh` |
| Render the screenshots in another window size, e.g. the smallest the layouts are made for (R94) | `SCREENSHOT_SIZE=1024x640 scripts/screenshots.sh` |
| Render the standard views of the 3D world (before and after a visual change) into `screenshots/views/` | `scripts/render-views.sh` |
| Count what a course's world draws at chosen places (draw calls, primitives), with each kind of geometry, effect or panel taken away in turn; frame times need a Mac ([Measuring frame rate](#measuring-frame-rate)) | `scripts/perf/software.sh` |
| Export the resource pack that measures frame rate in a copy of the native app on a Mac (`scripts/perf/mac.sh`, on the Mac) | `scripts/perf/pack.sh` |
| Compare measuring runs as a Markdown table for a pull request | `python3 scripts/perf/compare.py screenshots/perf/frames.csv` |
| Render the 3D models (buildings, plants, clouds) and the riders for review; with `GALLERY=docs/images/models`, also the galleries in `art/README.md` | `scripts/render-models.sh`, `scripts/render-riders.sh` |
| Redraw the wordmark and the lockups in `docs/brand/` after changing their geometry ([ADR 0026](adr/0026-the-name-orqwa.md)) | `python3 scripts/brand/wordmark.py` |
| Regenerate the boot splash PNG after a logo change | `scripts/render-splash.sh` |
| Refresh the translation template after text changes | `python3 scripts/i18n/extract.py` |
| Fuzz the decoders that read group riders' input (codes, controls, voice, state, workouts, STUN, shared courses), each target for N seconds; a finding leaves its input in `core/fuzz/artifacts/` | `scripts/fuzz.sh [N]` |
| Refresh the app's About notices and the CLI's packaged dependency/Opus licence file, after changing dependencies ([ADR 0018](adr/0018-about-and-notices.md)) | `scripts/notices.sh` |
| Make the map detail of an OpenStreetMap region download and measure it ([ADR 0033](adr/0033-map-detail.md)) | `cargo run --release --manifest-path core/Cargo.toml -p orqwa-cli -- detail <region>-latest.osm.pbf <out>` |
| Run the CLI with the fake trainer | `cargo run --manifest-path core/Cargo.toml -p orqwa-cli -- ride --fake` |

For window-mode changes, also run the window smoke test on a virtual display in the container:
`timeout 60 xvfb-run -a godot --path app --audio-driver Dummy -s res://tests/window_smoke.gd -- --fullscreen-only`.
The headless display has no window mode. The display run checks entering and leaving full
screen from the ride bar, synchronization with the keyboard, graphics setting and system
window controls, and restoring full screen after the overlay (#376, R150).

The 3D models are Blender scripts in `art/` (see [art/README.md](../art/README.md) for them and a
gallery of every model); Blender runs in its own container on the host:
`scripts/art.sh blender --background --factory-startup --python art/<group>/build.py`.

Worlds can be kept across updates (R101, [ADR 0025](adr/0025-kept-worlds.md)): a change that
makes courses come out differently raises `orqwa_world::GENERATOR`, so their pages offer to
regenerate them; one that kept worlds could no longer be read or drawn with raises
`orqwa_world::stored::FORMAT` and writes the stored sample again
(`ORQWA_WRITE_STORED_WORLD=1 cargo test -p orqwa-world stored`). With **Keep courses ready to
ride** on, a library course rides the world kept before your change: turn it off while working
on the generator. Renders and screenshots prepare GPX routes, which always build.

Map detail ([ADR 0033](adr/0033-map-detail.md)) changes no generator: the surfaces come with
a course's inputs, so a course prepared with them has its own fingerprint and one without
comes out as before. To try it in the container, put a region's download at
`~/.cache/orqwa/osm/geofabrik/<id>.osm.pbf`, run
`orqwa-cli detail ~/.cache/orqwa/osm/geofabrik/<id>.osm.pbf ~/.cache/orqwa/osm/detail-v1 --region <id>`
and write its record `~/.cache/orqwa/osm/geofabrik/<id>.json`
(`{"id": "<id>", "name": "…", "extract": "<id>-<date the command printed>"}`); GPX routes
prepared afterwards use it. With a region ready, `ORQWA_COURSE=<course.tqc> cargo test -p
orqwa-app -- --ignored --nocapture roundabouts_of_a_course` tells how far a course is ridden
against the traffic round each ring the detail maps (R143).

Urban surfaces and benches (R105–R106, [ADR 0027](adr/0027-urban-surfaces.md)) use generator 7
and stored world format 2: chunks include an `urban` mesh and its material codes. The format-2
sample is committed alongside format 1. Cached format-1 worlds rebuild on their next ride; course
files keep their existing format. Format-2 worlds from generator 4 offer regeneration to
add the more regular city benches. Generator 6 joins sidewalk edges around bends, closes
their sides down to the drawn terrain and slopes neighbouring pieces into lowered entrances.
Existing format-2 worlds offer regeneration for the corrected sidewalks; the layout is unchanged.
Generator 7 shares the carriageway's tangent arcs with the sidewalk at junctions, rounds outer
bends and clips footway polygons against the road instead of discarding complete sections.
The road-facing border uses separate 18 cm curbstones with a 2 cm beveled lip and 75 cm joints.
Bench and vegetation queries use the same clipped footprints as the rendered meshes.

Generator 27 keeps the matched original map feature alongside each ridden road segment's
appearance, plus its projected position on the matched local map section. Copies of that
section share one sidewalk and pavement owner, so coarse GPX bends and chained tile pieces
cannot create a second footway that cuts gaps in the ridden strip. Superseded map clearance
does not cut the new walk. Independent streets, joining arms and unridden parallel arms of
the same map feature keep their walks. This matching data is temporary; stored format 3
is unchanged.
Its walking tops follow the owning street profile, rather than a nearby lower road's bank.
Ridden endpoints use road elevations, mapped walks use centre-line terrain samples, and
junction curves use their inner edge. Both sides share the same grade and cross-section
datum; access ramps and bench/bin height queries agree with that surface. Solid sides
extend down to terrain, with buried edges kept correctly wound. Stored format remains 3.
Generator 28 carries both endpoint widths through walking strips, joins and access offsets.
Actual ridden carriageway triangles replace the wider terrain-shaping envelope in sidewalk
clearance, avoiding false gaps where road width changes. Partial overlaps with drawn building
outlines are clipped, preserving the clear walking width and closing the new outer edges.
Submillimetre GPU clipping slivers are discarded before normals are computed. Footprints
and height queries remain shared with vegetation and furniture; stored format stays 3.
Generator 29 limits a rounded bend's replacement triangle to the original outer sidewalk
miter, plus a 2 cm allowance. Its previous three-radius reach could cut a nearby parallel
street's walk while the curved replacement stayed beside its own road. The full saved
Andermatt comparison includes both streets; stored format remains 3.
Generator 30 shares the straight walking surfaces' grade fields with curved joins, across
the full tangent cross-section. Each arc section blends the same two parents using its
fraction along the whole corner; street-centre anchors supply fallback profiles. Review
the joints from road height too: matching footprints can still conceal a vertical step.
Generator 31 clips overlapping walking footprints to one top and fits a common grade through
each connected overlap region, easing into adjoining profiles over one walking width.
Folded inner offsets become bounded footprints before road clipping. Profiles at overlaps
and chunk boundaries share globally sampled endpoint heights instead of extrapolating a
different grade from each chunk.
Corner pieces require walking approaches at both ends; a paved junction alone cannot add
isolated sidewalk tiles beside a service road. Junction fans are reserved for distinct
street arms; a continuous street already owns its bend geometry, including inside a loop.
Walking overlap components are determined before ownership trimming and include touching
overlap groups, so clipping cannot split their grade. Stored format remains 3.
Generator 32 clips side extrusions to the exposed walking perimeter and ignores repeated
mask vertices, preventing internal side walls and duplicate boolean pieces. Stored format
remains 3.
Generator 33 requires at least 12 m of connected inferred raised sidewalk after clipping
and overlap ownership, before chunking. Surviving longitudinal intervals accumulate across
straight and curved pieces, with clipping partitions counted once. Isolated remnants do
not inherit missing source length. Access paths and forecourts retain their own rules;
stored format remains 3.

Generator 34 keeps sidewalk segment intersections inside each cut polygon's finite bounds.
Nearly collinear remnants with degenerate end edges cannot act as infinite bands, connect
walks across gaps or hide distant exposed edges. The 12 m minimum and stored format 3 remain.

Generator 35 goes round a roundabout once per visit (R145). A run of a file's points matched
on a ring, or at it on a street leading off it (within 3 m), is left out where going through
them would take the line more than a lap and 5 m round between the points before and after
the run (`Network::once_round` in `orqwa-routes/src/snap.rs`): they lie beside the ring past
the way out, as where a file follows a path beside it or rests there. The line then leaves
the ring the first time it passes its way out. A turn back at a roundabout, once round, stays.
Rings the map detail marks (R143) and those R126 finds by their shape are treated alike, so
courses without detail can come out differently too. Stored format 3 remains.

River surfaces (R118, [ADR 0030](adr/0030-river-levels.md)) use generator 8 with the same
stored format 2. `water_levels.rs` samples waterway centre lines before chunk generation;
both water meshes and channel beds use those shared elevations. Keep these profiles
independent of road shaping. Review a steep valley from the road and above, plus a lake,
after changing them. Older kept worlds offer **Regenerate world** for the corrected rivers.

Bridge geometry (R121–R122) uses generator 12 / stored format 3. `drape::deck` adds a
downward-facing underside at the same depth as the side walls, including at chunk joins;
the ridden road and railway decks are built separately in `structures.rs`. Older kept
worlds offer regeneration. Run `cargo test -p orqwa-world non_ridden_bridges` for coverage
of all five road classes, grades, bends and chunk boundaries, and check a render from below.

Generator 13 prevents a mapped bridge from being built again underneath the ridden bridge
when their aligned centre lines differ slightly. Bridge ownership uses their shared centre
corridor; ground junctions still require full cross-section coverage. Separate parallel and
crossing bridges remain visible. Stored format stays at 3.

Generator 14 keeps mapped approach paving out of ridden tunnels. Ground ribbons, rounded
ends and junction paving use the terrain's portal cut, at their own surface height, so a
height-grid ramp cannot close the opening after the terrain itself has been hollowed out.
Independent surface roads above tunnels remain visible. Review the full saved Alpenbrevet
route: cropping it changes the projection origin and terrain grid, which can hide this fault.

Generator 15 assigns a complete aligned mapped bridge to the ridden road when the feature
follows that centre line and overlaps its bridge. A slightly different tunnel/bridge tag
boundary must not leave a short mapped deck, railing or end cap across the approach. Bridges
that leave the ridden corridor still use per-section coverage and retain their visible parts.

Generator 17 keeps one railway crossing clearance across adjacent ground/bridge tag samples.
An overpass approach must not be pinned to road level immediately beside its elevated deck;
an underpass must stay below the road across its bridge end. Parallel tracks only share an
elevation when their independently estimated levels are within 6 m, so vertically separated
railways cannot pull one another into abrupt ramps. True level crossings retain their pins.

Generator 18 brings mapped bridge endpoints within 3 m of a connecting ridden ground road
to that road's elevation. Kerbs and railings open where a ground street meets a deck within
0.5 m of its height; exact short street segments keep these gaps off the opposite edge.
Separate overpasses retain their height and protection. Slab faces stay closed below the
wearing surface, including at junctions and true ends. Endpoint profiles are resolved before
chunk generation, and the lowered approach banks follow them so they cannot bury the deck.

Generator 19 extends complete-feature bridge ownership to aligned stretches retained as
Ground by route import. Their pavement already draws the surface; a second mapped deck at
raw DEM heights created the raised steps and skewed slabs in the Alpenbrevet follow-up images.
An entirely underground route cannot own a separate surface bridge above it. Crossing,
parallel and beyond-route bridges remain independent.

Generator 20 subtracts the complete tunnel volume from terrain and draped paving, using the
same road sections and bundled railway widths as the tube geometry. Short overlapping cuts
follow bends and grades; the original mouth cuts still extend beyond the portal rings.
This prevents shaped banks or nearby lower ways from leaving terrain faces inside a tunnel
farther than the old six-metre mouth cut. Complete aligned mapped ground approaches that
overlap the ridden tunnel share its pavement owner, including rounded ends, so duplicate
ribbons cannot climb onto the roof. Interior cuts reach below the tube floor so thin terrain
strips cannot show through the road. Railway sections compressed below a ridden-road
embankment retain that cover rather than cutting the uncompressed ellipse out of it.
Distinct surface roads crossing above remain visible.

Generator 21 keeps the ridden ground road's width-adjusted verge at its own datum before
railway terrain shaping. On Alpenbrevet's Furka climb, a lower parallel track's cutting
previously pulled down that shoulder, taking guardrails, posts and draped pavement with it.
The railway cutting blends smoothly between the protected road verge and the separate track
bed, retaining both elevations. Railway shaping still applies below road bridges. Protection remains
on the generated ground; its transform is not lifted independently of its footing. Protection
whose footing crosses its road station's chunk boundary is grounded against the neighbour's
completed terrain triangles, rather than the owner's coarse border interpolation.
Drop detection looks beyond that shaped verge so a flat shoulder does not create short
gaps and sloping rail ends beside a continuous cliff.

Generator 22 places each guardrail span between shared points on the actual offset road edge.
The points use the wearing surface's cross-section directions; each model's longitudinal
basis follows the edge chord and its endpoint heights, while posts remain upright. Inner
bends no longer overlap and outer bends no longer leave gaps. Endpoints crossing a chunk
boundary use the final neighbouring terrain so blade heights agree there as well.
Bridge barriers and slabs also share the pavement's averaged cross-section directions,
including the transition to ground road; posts follow the interpolated bend rather than the
preceding section's tangent. This prevents intrusion into curved carriageways and concrete
showing through the asphalt on Alpenbrevet's start/finish loop.

Generator 23 gives a retraced bridge one structure owner, matching the pavement. The return
GPX's different curve can no longer generate a second deck and crossing fences at the shared
roundabout approach. It also excludes bridge barriers from the full mapped roundabout area, including its
interior and carriageway. Boundary mismatches between Ground and Bridge share the earlier
pavement owner at the same level. Short different ends within the six-metre pavement overlap
margin belong to the complete earlier bridge, preventing residual fence stubs.
Following a curved GPX with persistent bridge tags is not enough
to infer a fence boundary there. This uses the ring's outer road footprint rather than the
GPX offset or an angle test, so parallel portions of the junction also stay open. Outside
protection and distinct higher overpasses remain. Tunnel service loops do not create a
surface junction exclusion.

Generator 24 applies a general road-clearance pass after all carriageways and junction fans
have been generated. It indexes their actual triangles, excluding verges, shore gravel and
the narrow mapped-deck kerbs. Barrier footprints are clipped against that pavement in plan
and elevation, checking complete spans and thickness rather than just ends or street angles.
Crossing rail/post pieces and concrete kerbs/parapets are removed together; slab sides,
end edges and slab shoulders recede below same-level joining pavement while undersides
and supports remain closed. Roadside guardrail and fence instances share the
same clearance check. Temporary barrier ranges are discarded before storing, so format 3
is unchanged. Protection on genuine overpasses and beside clear road edges remains.

Generator 25 retains the original map feature selected for each ridden segment's appearance.
Close, aligned mapped copies of that feature share the ridden pavement owner even when their
cross-sections are slightly offset. This removes duplicate ground ribbons, rounded caps and
false shoulder openings that can paint asphalt and markings down steep terrain. A shallow
joining street has a different source, as do independent parallel ways and a wider highway
beside a narrow ridden cycleway; they retain their pavement. Source IDs are used only during
generation, including after mapped features are chained. Stored format remains 3.

Generator 26 fits guardrail endpoint heights to the owning carriageway, 15 cm below its
surface. Posts retain their lateral clearance and embed their buried feet in the road edge
and skirt. A lower hairpin arm can cut the bank without pulling the upper road's blade below
the asphalt. Shared curved endpoints agree independently of terrain chunk ownership; only
ground-mounted snow poles and delineators use the final-ground correction. The terrain and
lower road remain clear. Stored format remains 3.

Generator 27 rides roundabouts the way traffic goes round them. OpenStreetMap draws a
roundabout in the direction of traffic, counter-clockwise where it keeps right and clockwise
where it keeps left; the tiles keep that order but rarely mark the ring one-way. Route matching
(`orqwa-routes/src/snap.rs`) finds rings of road 6–45 m in radius and round within 12 %, made
of one closed way or of up to eight ways each starting where the last ends (the map splits
some, tiles cut others at their borders), which at least two other roads join. Between two
matched points it goes round them only in drawing order, the long way round if need be, and
judges detours as if either way round were allowed. A point matched at most 5 m back round
the ring is GPS wander, not a lap; a loop only one street leads to stays two-way. Dense
recorded tracks come out as before; files whose points skip the ring no longer take the short
way against the traffic. Kept worlds built with the old route offer regeneration because their
ridden road follows it. Stored format remains 3.

Generator 36 gives surrounding vehicle roads, unpaved service roads, tracks and cycleways
shared road-bed profiles before terrain chunks are built (R141, ADR 0031). Heights use a cached
global 16 m terrain lattice, with 24 m local smoothing and a 20 % intermediate-grade envelope;
chain ends and branch/tag-change endpoints keep their common datum. Small paths, including paved
footpaths, keep following the terrain. Bridges and tunnels keep their structure rules.

The terrain at a point is composed in one place, `Shapers::shape_at` in
`core/orqwa-world/src/lib.rs`, from the priority rule: ridden road, then railways, then
surrounding ways. `shape` is the ridden road's and railways' ground as before;
`level_across` (`earthworks::shape`) is a way's bed and banks. `Shapers::keep` says how firmly
the way holds a point (its own reach, the partition towards the ridden verge, and the shadow
behind it seen from the ridden road); `Shapers::structure_keep` gives tunnel cover and portal
surroundings back to the ridden road, and `hard_verge` keeps the ridden verge at its usual
width unless a close way needs room. Every factor fades to zero at its own edge: a change that
switches one on or off at a hard boundary leaves a terrain step. `drawn_ground` uses the same
composition, so portals, bridges and props stand in the ground the chunks draw.

`Levels` forms beds within `FORMED_REACH_M` (120 m) of the ridden road; farther tracks and
cycleways stay draped and paved roads keep a plain shelf without walls. It refines terrain
(`bed_detail`) only beside a close bed at another height than a ridden carriageway or railway
(gaps under 5.5 m: 24 pieces a side, under 3 m: 48; `CLOSE_RISE_M`), and for paved roads
across hillsides steeper than 0.6, where walls need clean triangles, and tracks and cycleways
steeper than 0.35, whose level shelf (`Level::verge`) narrows to the finer pieces' margin (12).
Compare world triangle counts and build time before and after a change here: refining every
close or steep piece at 24 cost 79 % more ground triangles on Alpenbrevet Gold, this rule
17 %, with a build about 30 % longer (per-point shaping in the grid and its normals). Their earth banks
(`earthworks::shape` without `facing`) meet the hillside instead of fading into it as a scar,
and boulders beside formed ways follow the natural slope (`vegetation`). Pieces the ridden road owns, by alignment whatever their structure
tags (`RoadIndex::owns_alignment`), form no bed. Concrete faces (`earthworks::retaining_faces`)
dress only steep banks a paved vehicle road's earthworks moved and holds. Dense-city paving
(`Urban::paint`) stops at engineered banks steeper than 20 % and natural ground steeper than
about 35 %.

For controlled visual review, run `cargo test --manifest-path core/Cargo.toml -p orqwa-world
earthworks_render_fixture -- --ignored`, then render `app/tools/render_earthworks.gd` with
`OUT_DIR` set to an existing absolute directory. `FIXTURE` selects an alternate exported JSON
for an identical-camera baseline. Pair it with the saved full Alpenbrevet Gold course at 32.40,
35.75, 36.40, 40.07 and 54.60 km, rendered at the same cameras before and after.

Single-track bridges fit the ballast plus a maintenance walkway. Concrete viaducts use
low kerbs, open steel railings, capped columns and bearing walls at their ends; stone arches
retain masonry parapets. Generator 16 builds road and railway structures after terrain chunks
so piers and bearing walls meet the final ground triangles, including shaped banks and carved
channels. Foundations clip terrain triangles to their complete rectangular footprint and
extend below its lowest point; a centre sample or four corners can miss a downhill face or
channel. Mapped-road piers use the same footprint calculation against their chunk's terrain.
`BridgeMaterials` shares dedicated scanned cast-concrete maps and
world-space projection across structure faces and exposed road backs; arches use masonry.
Rebuild the bridge maps with `scripts/art.sh blender --background --factory-startup --python
art/textures/build.py -- bridge_concrete`; originals, licence and checksums are archived under
`art/sources/concrete-wall-007/`. Wearing surfaces retain road-profile metadata. New bridge
details use sRGB tints with zero
alpha, outside the positive road-style flag range; steel tints shade the rails.
Older worlds keep working; shaders draw road, street, path and railway ribbons from both
sides immediately. `surface_smoke.gd` renders all four materials above and below and checks
opacity. `scripts/check.sh` runs it with software Vulkan/Xvfb after the headless smoke tests.

## Trainer protocols and hardware checks

`core/orqwa-devices/src/trainer.rs` is the control-encoding boundary for FTMS and Tacx
FE-C over Bluetooth (R110, [ADR 0028](adr/0028-tacx-bluetooth.md)). Bluetooth discovery,
subscriptions, device handles and reconnection stay shared in `ble.rs`. Tacx packet and
command-queue tests run with `cargo test -p orqwa-devices`; they do not require a radio.

The NEO 2 Smart has been checked on hardware. Check another Tacx model the same way, with a
native CI-built app/CLI (Docker on macOS has no Bluetooth): close other trainer apps, run
`orqwa-cli scan`, then connect in the app's Trainer row. With `RUST_LOG=orqwa_devices=debug`,
record firmware, advertised services, power/cadence/speed, SIM on flat/climbs/descents, ERG
targets, fixed resistance, release on pause, virtual gears and reconnection. A model that
passes joins the wiki's tested devices. Existing device screenshots still apply; the
adapter adds no controls or layout changes.

## Refreshing screenshots

Tree rendering uses the original close models plus authored medium/far forms from
`art/vegetation/lods.py`; rebuild those in the art container after a tree shape change.
They preserve placement and stored worlds. Compare the same native GPU, camera and pixel
dimensions before and after, with Low included; see the [tree LOD measurements](benchmarks/tree-lods.md).

The current [screenshot gallery](screenshots.md) links the app screens and standard route
views; [Art](../art/README.md#gallery) contains the model galleries. Run captures separately
from checks. For a documentation refresh, isolate the generated sample rides from your own
profile by setting `XDG_DATA_HOME` inside the container:

```sh
scripts/dev.sh sh -c 'XDG_DATA_HOME=$PWD/screenshots/refresh-data SCREENSHOT_DIR=$PWD/screenshots/refresh/app scripts/screenshots.sh'
scripts/dev.sh sh -c 'XDG_DATA_HOME=$PWD/screenshots/refresh-data OUT_DIR=$PWD/screenshots/refresh/views scripts/render-views.sh'
scripts/dev.sh sh -c 'OUT_DIR=$PWD/screenshots/refresh/models GALLERY=$PWD/screenshots/refresh/gallery scripts/render-models.sh'
scripts/dev.sh sh -c 'OUT_DIR=$PWD/screenshots/refresh/riders GALLERY=$PWD/screenshots/refresh/gallery scripts/render-riders.sh'
```

The software-rendering runners select FXAA instead of MSAA and disable glow, which otherwise
produces bright halos under lavapipe. They wait for route preparation and streamed chunks;
the route views pause the rider at fixed positions. The graphics settings in the app are
unchanged. Both app and route runners check their capture script compiles before rendering.

Additional UI fixtures use the same real controls without hardware or account credentials:

`SCREENSHOT_SUMMARY_ONLY=1 scripts/screenshots.sh` captures just the quick finish dialog over
a finished fake-trainer workout as `summary-quick.png`, without rendering a 3D world.
Use a fresh `XDG_DATA_HOME` to show the workout's first power PR in that capture; this mode
does not generate sample ride history.
The normal run also captures its full details as `summary.png`.
The automatic-finish, manual-finish and discard flows are checked by `summary_smoke.gd`;
`window_smoke.gd` checks the dialog at 1024 × 640 in English and German.

For a graphics/settings change, capture only the settings with
`SCREENSHOT_SETTINGS_ONLY=1 scripts/screenshots.sh` in the container; this avoids building
a course or populating sample rides. `SCREENSHOT_LOCALE=de` selects German for the same run.
The Courses tab turns map detail on for its picture and lists the regions in the container's
cache: copy a region's download to `~/.cache/orqwa/osm/geofabrik/<id>.osm.pbf` first (e.g.
Switzerland's), and it is prepared before the capture; the setting is restored afterwards,
which deletes the region if map detail was off.

```sh
scripts/dev.sh sh -c 'XDG_DATA_HOME=$PWD/screenshots/refresh-data SCREENSHOT_DIR=$PWD/screenshots/refresh/app timeout 240 xvfb-run -a godot --path app --resolution 1600x900 -s res://tools/capture_details.gd'
```

`capture_details.gd` captures device readings, connection failures, planned workouts and
expanded error details in English and German. Capture `app/video-course.png` from a real
ride using locally supplied licensed footage and its Incyclist XML/GPX. Keep that source
outside app assets and preserve its attribution in [Credits](CREDITS.md) and the displayed
documentation. The exporter expects this video image alongside the standard app captures.

Review every capture before copying it into documentation. `contact_sheets.gd` recursively
builds sheets and a filename index using image I/O only:

```sh
scripts/dev.sh sh -c 'INPUT_DIR=$PWD/screenshots/refresh/app CONTACT_DIR=$PWD/screenshots/review/app godot --headless --path app -s res://tools/contact_sheets.gd'
scripts/dev.sh sh -c 'REFRESH_ROOT=$PWD/screenshots/refresh godot --headless --path app -s res://tools/export_screenshots.gd'
```

Repeat the review for `views`, `models` and `riders`; open full-size images where a thumbnail
cannot establish quality. The exporter replaces the stable app/wiki image paths and writes
the route gallery to `docs/images/views/`. Copy the reviewed model JPEGs from the scratch
gallery into `docs/images/models/`. Historical comparison baselines stay with their original
reviews. Phone-remote browser screenshots require a separate browser capture; these helpers
do not replace them.

## Measuring frame rate

The rider's **Benchmark graphics** setting (R151, [ADR 0034](adr/0034-graphics-benchmark.md))
calibrates presets against a bounded offline workload. It does not replace the course-specific
measurements below. Its integration test runs headless with the checks; to also exercise
preview rendering and cancellation, run in the container:
`xvfb-run -a godot --path app --rendering-driver vulkan --resolution 1024x640 -s res://tests/benchmark_smoke.gd`.
`BENCHMARK_SCREENSHOT` saves a review image before cancellation. Do not run this alongside checks.
With `BENCHMARK_COMPLETE=1`, it also runs all four presets to completion and checks the saved
recommendation and restored rendering settings; software FPS still says nothing about a GPU.

Frame rate needs a real GPU: software Vulkan in the container draws the same draw calls and
primitives, but its timings say nothing about a GPU. `app/tools/measure_frames.gd` measures a
course's world either way. It opens a course (a `.tqc`, built offline from its saved inputs, or
a GPX route), waits for every chunk, pauses the rider at each distance in `PLACES` and samples
the frames there: everything shown (`all`), then with one thing taken away at a time, as `TAKE`
asks. Each sample appends a row to `frames.csv`: frame rate, median and 95th-percentile frame
time, and the draw calls and primitives in view and in the shadow maps. Each place gets a
screenshot, the run a log with the world's build and upload times and its geometry by kind.

| `TAKE` | Samples |
|---|---|
| `kinds` | Each kind of chunk geometry hidden: terrain, streets and tracks, sidewalks (`urban`), building models and shells, trees, grass, water, props |
| `effects` | Each of the preset's effects off: reflections, ambient occlusion, bloom, soft shadows, multisampling, the sun's shadows; two shadow splits; shadows to 300 m |
| `interface` | The minimap, the whole ride screen, all chunks hidden |
| `distance` | The ground, ways, sidewalks and water of chunks beyond `FAR` metres (600) hidden, each and together: the most that drawing them more cheaply far off could save |

`QUALITY` picks the preset (Ultra unless set), `CAMERA` the camera (0 chase, 1 first person,
2 drone), `MINIMAP` its view (`close`, `route` or `hidden`).

**On a Mac.** Release builds take neither `-s` nor `--main-pack`. So `scripts/perf/pack.sh`
exports the project's resource pack in the container with the tool as its main loop, and
`scripts/perf/mac.sh` runs a copy of the installed app in `/tmp` with that pack, an ad-hoc
signature and a throwaway `HOME`: the rider's own profile, rides, settings and kept worlds stay
untouched, and the copy does not look for updates. The copy brings its own Rust extension, so
the installed app measures GDScript changes; for a Rust change set `APP` to the macOS app CI
built from the branch (the run's `ORQWA-macos-arm64.zip`).

```sh
scripts/dev.sh scripts/perf/pack.sh               # screenshots/perf/measure.pck
COURSE=core/orqwa-gd/courses/twann-chasseral.tqc PLACES="2750 9000" TAKE=kinds,effects \
    scripts/perf/mac.sh screenshots/perf/measure.pck branch
```

To compare with another commit, pack its project (with today's tool) and measure both alike:

```sh
mkdir -p screenshots/perf/main && git archive origin/main app | tar -x -C screenshots/perf/main
scripts/dev.sh scripts/perf/pack.sh screenshots/perf/main.pck screenshots/perf/main/app
COURSE=... PLACES=... scripts/perf/mac.sh screenshots/perf/main.pck main
scripts/dev.sh python3 scripts/perf/compare.py screenshots/perf/frames.csv main branch
```

`compare.py` prints a Markdown table of median frame times with the change from the first
label to the last, ready for a pull request. Keep the window in front and run nothing else
meanwhile (checks, renders, builds), on the same power source for every run; follow a run
with `tail -f screenshots/perf/<label>.log`. A city's world takes about a minute to build, then
each sample about four seconds. The window is 1600 × 900 points, twice that in pixels each way
on a Retina display; the log notes the viewport. Write lasting results into
`docs/benchmarks/` with their conditions, as in [city courses](benchmarks/city-courses.md).

**In the container.** `scripts/perf/software.sh` runs the tool under software Vulkan, for the
counts alone: what a change does to draw calls and primitives when no Mac is at hand. Set the
tool's settings inside `sh -c` so they reach the container:

```sh
scripts/dev.sh sh -c 'COURSE=core/orqwa-gd/courses/twann-chasseral.tqc PLACES=2750 TAKE=distance scripts/perf/software.sh'
```

On Windows the portable Godot editor runs the tool directly with the extension cross-built
for it (`scripts/build-gdext.sh release windows`):
`godot --path app -s res://tools/measure_frames.gd`, with `TIMED=1`, an absolute `OUT_DIR` and
the settings above in the environment.

## Checking music controls

Music control (R26, #260) uses `osascript` on macOS, Windows `SendInput` through a
PowerShell helper, and `playerctl` on Linux. No new library dependency is needed. macOS
checks bundle IDs and compiles only the running player's command: compiling a `tell`
statement for an absent Spotify can otherwise fail before the Apple Music fallback runs.
Even a literal bundle ID can fail compilation when its app is absent, so running checks
resolve IDs through a variable and treat a missing application as stopped.
Windows sends a virtual media key down and up; `SendKeys([char]179)` would type a character.

The macOS and Windows CI jobs compile and run the tests in
`core/orqwa-app/src/media/commands.rs` directly with `rustc --test` before building the app.
This is the production module, which needs only Rust's standard library; native script
checks need not build the rest of the app or FFmpeg first. The macOS test executes all three
commands with nonexistent player IDs and expects the no-player
runtime error, catching premature dictionary lookup. Windows compiles the real helper and
checks the `INPUT` size and the key-down/key-up events without injecting keys into the
runner's desktop. Container tests check command mappings; none of these confirms playback.

For native playback checks, use a downloaded app build, start a playlist in the player,
then focus ORQWA during a ride and press **M** twice, **.**, and **,**. Confirm pause/resume,
next and previous in the player. On macOS try Apple Music with Spotify absent, Spotify
alone, both running (Spotify takes priority), and neither running; verify the first
permission prompt and the error if permission is denied. On Windows use Spotify or a
browser tab that registers a system media session; successful input injection does not
tell whether a player handled it. On Linux use an MPRIS player with `playerctl` installed,
then try no player and a missing `playerctl`. For a regression report, record the app build,
OS, player and results. Run these manual checks separately from container checks.

For #260, the rider reported working music control on macOS with Music/Spotify open, on
Windows, and on Ubuntu with VLC after installing `playerctl` and loading media
(2026-10-21). These are manual playback reports; exact app/OS versions and every
permission/error-path case were not specified. The rider guide now explains installation
and how to distinguish a missing helper, no visible player and an empty VLC playlist.

## Builds

Tagged builds of the app and CLI are available on the
[Releases page](https://github.com/ORQWA/ORQWA/releases), without signing in.
Between releases, download artifacts from the
[CI runs on main](https://github.com/ORQWA/ORQWA/actions/workflows/ci.yml?query=branch%3Amain)
while signed in to GitHub. Both use the same archives:

The `macos` CI job builds `ORQWA-macos-arm64.zip` and `orqwa-cli-macos-arm64.tar.gz`. To test
real trainers on macOS, use the CLI built by CI: see the [command-line guide](https://github.com/ORQWA/ORQWA/wiki/Command-line-tool).

Windows builds come from the `windows` CI job: `ORQWA-windows-x86_64.zip` (`ORQWA.exe` with
`orqwa_gd.dll` beside it; keep both in one folder), the installer `ORQWA-windows-x86_64.msi`
and `orqwa-cli-windows-x86_64.zip`, all under the run's artifacts. The installer is built from
`scripts/release/ORQWA.wxs` with the WiX Toolset ([ADR 0023](adr/0023-windows-installer.md)),
and the job installs and uninstalls it once to check it. Like the macOS app it is built natively on the runner (MSVC, with MSYS2's
`sh` and `make` for FFmpeg).
FFmpeg's libraries come from `scripts/build-ffmpeg.sh` (a pinned, verified release; ADR 0010):
built into the dev image, and once per platform in CI.

For a quick look on a Windows PC's GPU while developing, the dev container cross-builds the
extension too ([ADR 0021](adr/0021-windows-extension-in-the-container.md)):

```sh
scripts/dev.sh scripts/build-gdext.sh release windows
```

It puts `orqwa_gd.dll` (MinGW, linked statically) into `app/bin/`, where the portable Godot
editor on the host loads it to render the views in seconds instead of lavapipe's minutes. It is
for development only; the released Windows app is CI's.

Linux builds come from the `linux` CI job, for x86_64 and arm64: `ORQWA-linux-<arch>.tar.gz`
(a `ORQWA` folder with `ORQWA.<arch>` and `liborqwa_gd.so`; keep both together) and
`orqwa-cli-linux-<arch>.tar.gz`. They are built on Ubuntu 22.04 so they run on any distribution
with glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+, Fedora 36+), not only on ones as new as
the dev container ([ADR 0012](adr/0012-linux-builds.md)). CI starts the exported app
headless to check that it loads.

Docker on macOS cannot access Bluetooth or the GPU, so macOS builds are produced by GitHub Actions.
To test 3D rendering or a real trainer, run the built app (or the portable Godot editor) natively.

Every build names the branch and commit it comes from on the settings' About tab and in its
first log line (`ORQWA 0.2.1 (main@0ad9af1)`), as Git reports them while building, or the
`ORQWA_BRANCH` and `ORQWA_COMMIT` variables when set ([ADR 0018](adr/0018-about-and-notices.md)).
`scripts/dev.sh` sets them from the host: a worktree's Git directory lies outside the checkout
the container sees. CI sets them to a pull request's own branch and last commit, as it builds
the merge with main; a release, built from its tag, names its commit alone.

### Riding audio

For group voice (#413, #428, #441), run the `orqwa-voice` conversation tests in the container.
They use pinned loopback QUIC, real Opus and simulated devices, including delayed old-worker
cleanup and recoverable device failures. Native Bluetooth acceptance remains manual: switch
speakers during listening, enable the headset microphone, then repeat mute/deafen and device
changes during a ride. Check that a rate transition resumes sound, a manual choice starts
microphone-off, and permission/disconnect errors leave the group and local ride intact. Record
the OS, headset, chosen input/output and Technical details for failures. Test separately from
container checks; no real microphone or Bluetooth device is available inside Docker on macOS.

For R177, simulated-device tests check that local microphone metering never calls playback,
releases capture on close and after ten seconds, and retains permission diagnostics. Encrypted
conversation tests cover the speaker gate and full-duplex headphones independently. On native
devices, test before joining voice and while speaking, close the settings dialog mid-test,
switch input during testing, and leave the group. Verify that only the level meter moves,
capture ends and opted-in listening returns microphone-off. With speakers, take turns; with
headphones on every rider, disable **Prevent speaker echo** and verify simultaneous speech.
Issue #258 still requires listening during a real video ride; group voice changes do not
validate WSOLA's perceptual quality or settle its fade thresholds.

The Rust ambient mixer, its CC0 texture sources, regeneration command and a WAV listening
preview are documented in [the audio README](../core/orqwa-app/audio/README.md) and
[ADR 0029](adr/0029-ride-audio.md). Run the preview and asset preparation in the devcontainer.
The live mixer needs no audio downloads or additional dependency.
The `--rain` preview switches Clear → Rain → Clear at a standstill, isolating the rainfall
and its weather transitions from bicycle sound; Wind volume controls rain too (R153).

### Publishing a release

A tag does not set the version. The About tab, the first log line, the CLI and the User-Agent
read `version` in `core/Cargo.toml`; the macOS bundle reads `application/short_version` and
`application/version` in `app/export_presets.cfg`, the Windows `.exe` `application/file_version`
and `application/product_version` there, with a fourth number 0 (`0.3.1.0`). First set the new
version in all those places and update `core/Cargo.lock` to match:
`make release-version VERSION=0.3.1` does both in the container, refusing anything but three
numbers (the macOS bundle's version allows no more). Commit that, re-render the About
screenshot with `scripts/screenshots.sh` (it shows the version and the commit) and merge both.
`scripts/check.sh` fails while the versions disagree (`scripts/release/version.py`).

Then push the tag `v<version>` on the merged commit, for example `git tag v0.2.1` then
`git push origin v0.2.1`. Every tag triggers CI, but only a tag naming its commit's version is
released: the `version` job fails any other, such as `v0.2.0` on a commit saying 0.2.1, and
the release needs it. Branch and PR builds only produce workflow artifacts.
The tag's commit must contain this workflow. A tag push runs the container checks and the
native macOS, Windows and both Linux builds, and the Android build. After all succeed, the
`release` job verifies every nonempty app/CLI archive (the Android APK among them), creates a
draft with generated release notes, uploads the archives and publishes it. Each archive goes up
with the release's version in its name (`ORQWA-0.5.2-windows-x86_64.msi`,
`orqwa-cli-0.5.2-linux-arm64.tar.gz`; #322), and only under that name; the builds and CI
artifacts keep plain names. Only that job has
`contents: write`, using the automatic `GITHUB_TOKEN`.

The APK of a tag is signed with ORQWA's release key, from the repository secrets
`ANDROID_KEYSTORE` (the keystore, base64) and `ANDROID_KEYSTORE_PASSWORD` (alias `torqa`); the
build fails without them. Every Android update must carry the same key, so its owner keeps a
copy outside GitHub. Other builds are signed with a debug key.

Build jobs upload each archive as a single file (`archive: false`). The release download
uses `skip-decompress: true` to keep those files intact: without it the download action
extracts ZIPs, leaving the publisher without the macOS and Windows release archives.

If a build or check fails, fix it, set a new version and tag the corrected commit with it.
If publication fails, rerun the failed job from the tag's CI run: an unfinished draft is
resumed and its assets replaced before publication. A published release is left unchanged on
reruns. Tag runs are not cancelled by a later push. Check the CI run and the release's assets
before announcing it; merging the workflow PR alone does not create a release.

A workflow fix does not change an existing tag's CI run. For a failure caused by the workflow
itself, merge the fix with a new version and tag that rather than rerunning the old
workflow.

### Android

The APK comes from a container of its own ([ADR 0024](adr/0024-android.md)), built on the dev
image only when needed. `app/` is never changed: the build works on a copy.

```sh
scripts/android.sh scripts/build-android.sh release   # → dist/ORQWA-android-arm64.apk
```

Install it on a tablet with adb over USB or wireless debugging (`adb install -r …`) and read
ORQWA's log with `adb logcat -s ORQWA godot`. Local builds are signed with a debug key made once
in the `orqwa-gradle` volume; `ORQWA_RELEASE_KEYSTORE` and `ORQWA_RELEASE_KEYSTORE_PASSWORD`
sign with a release key instead, as CI does for releases. The APK's version is ORQWA's
(`version/code` 500 for 0.5.0). [android/README.md](../android/README.md) lists
the parts and how to remove Android support again.

## More

- Contributor conventions: [CLAUDE.md](../CLAUDE.md)
- Architecture decisions: [adr/](adr/)
- Roadmap: [PLAN.md](PLAN.md); requirements: [REQUIREMENTS.md](REQUIREMENTS.md)
- Third-party assets and their licences: [CREDITS.md](CREDITS.md)
- Translating the app: [translating.md](translating.md)

## Usage wiki

Keep the wiki focused on usage guides: setup, controls, settings, rider workflows and
troubleshooting. Update it when those tasks change. Pure visual or internal fixes need no
wiki edit; implementation details, bug-fix notes and review images belong in repository
documentation and the PR.

Keep the welcome and naming story on Home, and visual examples in the screenshot gallery.
Lead guides with the steps a rider needs; explain settings and limitations only as needed
to make a choice or solve a problem.

Rider guides live in the [GitHub wiki](https://github.com/ORQWA/ORQWA/wiki), checked out as
the `wiki/` submodule ([ADR 0015](adr/0015-usage-wiki.md)). Clone with
`git clone --recurse-submodules`, or run `make wiki-init`
in an existing checkout. Initialization leaves an existing wiki checkout untouched; after
pulling application changes, run `git submodule update --init -- wiki` only when the wiki is
clean and its local work has been saved.

Git operations use your host's credential helper. Documentation validation and tests run in
the dev container; inside an already running container, use `python3 scripts/wiki/check.py`
and `python3 -m unittest discover -s scripts/wiki -p 'test_*.py'` directly.
For linked worktrees, `scripts/dev.sh` also mounts the shared Git metadata read-only, so
validation can read the checkout's index without changing the original workspace.

1. Initialize the wiki and create a branch before editing:
   `make wiki-init`, then `git -C wiki switch -c docs/my-change`.
   Resume an existing branch with `git -C wiki switch docs/my-change`.
2. Update affected guides, with navigation and screenshots only where needed. Wiki filenames are
   GitHub page names. Use absolute `https://github.com/ORQWA/ORQWA/wiki/<page>` links, and
   `https://raw.githubusercontent.com/ORQWA/ORQWA/main/docs/images/<file>` for shared images.
   Check wiki references before deleting an image from the application repository.
3. Run `make wiki-prepare BRANCH=docs/my-change`. It validates, commits wiki changes and
   pushes only the preparation branch; it never updates published pages. Name it `docs/<change>`
   and keep it until publication: the publisher then deletes it.
4. Explicitly stage `git add wiki` in the application PR and include the comparison link
   printed by preparation. Wiki commits are separate from application commits; review both.
5. After merge into application `main`, the **Wiki** workflow publishes the pinned wiki
   revision. Existing published edits are preserved by merging. If this creates a different
   commit, automation opens or updates `docs: sync published wiki revision`, changing only
   the submodule pointer. Review and merge that PR normally; it does not republish changes.

### GitHub setup

The wiki must be enabled and have an initial page before its Git repository can be cloned.
Publishing uses GitHub's automatic token with `contents: write` and `pull-requests: write`;
no custom secret is required. In repository **Settings → Actions → General → Workflow
permissions**, enable **Allow GitHub Actions to create and approve pull requests**. Automation
never approves or merges its own PRs. PR checks created with the automatic token may require
a maintainer to approve their workflow runs.

Optionally set the repository Actions secret `WIKI_AUTOMATION_TOKEN` to a token that can push
the wiki and application automation branch and create PRs, if checks should start without
that approval step. Enter it directly in GitHub's settings; never put credentials in files,
commit messages, URLs, logs or chat. The workflow uses this secret if present, otherwise its
automatic token.

### Recovery

The publisher validates documentation before pushing and never force-pushes the published
wiki branch. On conflicts, preserve both histories: fetch the wiki, merge its published branch
into the preparation branch, resolve the pages, prepare again and submit the new pointer in
an application PR. The published branch is currently `master`; tooling discovers its default
branch rather than assuming it is named `main`.

For transient failures or a failed PR creation after successful publication, rerun **Wiki**
using **Run workflow**. Each attempt reads the latest application `main`, publishes only its
pinned history, and reuses one pointer PR. A retry creates no changes when both repositories
are synchronized. The only branch automation may replace with a lease is its own
`automation/wiki-sync` branch; it never replaces user preparation branches. After publishing,
it deletes `docs/*` wiki branches whose commits are all published, so their commits stay
reachable from the published branch; a branch that gained commits meanwhile is kept.
