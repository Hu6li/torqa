# Art

Scripted 3D models for ORQWA ([ADR 0009](../docs/adr/0009-asset-pipeline.md)), and the phone
remote's [silent clip](#the-remotes-clip-remote). The scripts are the source; what they make
(the `.glb` files, the clip) is committed so the app builds without Blender.

Seasonal presentation reuses these models, including their authored tree LODs; Summer is
the green baseline. See [seasons](../docs/seasons.md) for the rendering contract and review.
[Winter materials](../docs/seasons-winter.md) reuse vegetation and rock geometry for
slope-dependent snow and dormant broadleaf foliage. Disposable road-edge banks derive from
the kept road mesh, deepen with elevation and leave junctions and structures open; street
snow and paired tyre grooves share the climate treatment. Natural-ground blankets and
boulder caps gain depth with altitude too; a derived clearance mask protects ways and paving.
Greater snow load increases inclined-surface coverage. No additional source texture is needed.

[Spring](../docs/seasons-spring.md) adds blossom to a subset of broadleaf crowns and opens
the procedural five-petal meadow flowers; placement and Blender exports stay the same.

The [autumn materials](../docs/seasons-autumn.md) share the palette across nearby foliage,
authored tree LODs and distant crowns; evergreen needles keep their own colours.
Orange/russet distant collections stand among greener forest areas. Woodland streets gain
procedural fallen leaves from a retained-map mask, with no new asset or generator change.
Meadow flower clumps become grass, and flower-box blooms are hidden for Autumn.

## Running Blender

Blender runs headless in its own container (official Linux builds exist for x86-64 only, so on
Apple Silicon Docker emulates it):

```sh
scripts/art.sh blender --background --factory-startup --python art/buildings/build.py
scripts/art.sh blender --background --factory-startup --python art/buildings/build.py -- chalet_2_m
```

The second form rebuilds only the named models. Then review them as the world draws them
(runs in the dev container; images land in `screenshots/models/<group>/`):

```sh
scripts/dev.sh scripts/render-models.sh
scripts/dev.sh sh -c 'MODELS="chalet_2_m" scripts/render-models.sh'
scripts/dev.sh sh -c 'GROUPS="vegetation" scripts/render-models.sh'
```

After a change to the models, refresh the [gallery](#gallery) below as well.
The app and route captures are collected in the [screenshot gallery](../docs/screenshots.md);
the [refresh workflow](../docs/development.md#refreshing-screenshots) includes reviewing every
model and rider capture before replacing these gallery images.

For quick looks while working, the build scripts of buildings and vegetation render
Cycles previews of their own materials instead of exporting (`art/preview.py`; four views per
model, no Godot needed):

```sh
scripts/art.sh blender --background --factory-startup --python art/buildings/build.py -- --preview screenshots/preview chalet_2_m
```

## Look

Since 2026-10-18 the target is as realistic as possible ([ADR 0020](../docs/adr/0020-realistic-look.md)):
smooth shading, real proportions, physically based materials and light. Every group below has
left the faceted look of ADR 0011; riders are smooth but still stylized figures (`docs/PLAN.md`,
Phase 9).

Rock surfaces (R115) combine the CC0 Poly Haven **Rock Boulder Dry** scan with metre-scale
weathering in `app/shaders/rock.gdshaderinc`, shared by cliffs and boulders. Larger fractured
faces remain visible after grain is mipmapped away; joints are irregular and interrupted,
without masonry rows or paving cells. Weathering scales are 8 m on cliffs and 0.65 m on
boulders. Relief changes shading, preserving terrain contact; exposure patches break up the
grass-to-rock transition.

## Textures (`textures/`)

Ridden and surrounding roads share `app/shaders/road.gdshader` (R119, ADR 0031). Rust encodes
their surface, width, marking style and lane/track flags in mesh colours; reuse the existing
asphalt, gravel, dirt, grass, concrete and paving textures. Widths and lane layouts are visual
estimates from road hierarchy, direction and ramps; current tiles normally retain only paved
versus unpaved. No new textures or per-course asset downloads are needed.

`build.py` bakes tileable PBR textures (albedo, normal, roughness; WebP, normal maps lossless)
from procedural Blender materials into `app/assets/textures/`: for the ground and roads `grass`,
`dirt`, `gravel`, `rock` and `asphalt` (`sand` too, unused so far), for trees `bark`, for
buildings `plaster`, `roof_tiles`, `slate`, `planks` and `masonry`. Only the maps the app reads
are committed (most materials need no roughness map). Rock is converted by `rock_scan.py`
from the untouched 1K CC0 originals in `sources/rock-boulder-dry/`, with source and credits
recorded there and in [Credits](../docs/CREDITS.md). `build.py -- rock` runs that conversion. Pass names after `--` for some, `--size`
for another resolution (1024 by default) and `--out` to bake elsewhere, such as
`screenshots/textures` while trying things out.

Urban ground (R105, [ADR 0027](../docs/adr/0027-urban-surfaces.md)) adds `concrete` slabs
and staggered rectangular `paving`, each with 1024 px albedo, normal and roughness maps.
Concrete spans 3 m per tile (0.75 × 0.5 m slabs); paving spans 2 m (0.25 × 0.5 m blocks).
The Rust generator drapes sidewalks, forecourts and building access over the terrain and
builds curb and low-wall faces, using these shared materials in `app/shaders/urban.gdshader`.
Sidewalks share their joins around bends, slope into lowered entrances and have solid
perimeter faces extending down to the terrain, including outer edges and exposed ends.
At junctions they share the asphalt's rounded corner outline. An 18 cm curb-stone strip
follows the road edge, with a beveled lip and joints; outside bends curve around the road.
Terrain uses paving in dense districts and shared road asphalt for commercial/industrial
land. Mapped green areas and low-density residential lawns retain their vegetation.

Water geometry uses shared longitudinal elevations rather than the bank terrain
(R118, [ADR 0030](../docs/adr/0030-river-levels.md)). Keep river cross-sections level while
the course descends; the bed follows the same datum and the gravel band follows the bank.
Tall banks blend out over a wider reach; exposed steep channel sides use triplanar rock.
Review steep valley rivers from road height and above with `orqwa-render-review`.

Non-ridden road bridges have solid 0.7 m decks (R121), with separate downward-facing
underside vertices to keep the slab edges sharp. Undersides use edge UVs so unpaved bridges'
grass strips stay on top. Review the deck from road height underneath as well as from above;
the generator adds this geometry when a kept world is regenerated.

Bridge structures (R122) use the CC0 Poly Haven **Concrete Wall 007** photograph-based
albedo, normal and roughness maps for cast concrete, with pores, worn patches and formboard
marks at their original 2.16 m scale. `concrete_scan.py` converts the archived originals in
`art/sources/concrete-wall-007/`; rebuild with `build.py -- bridge_concrete`. These dedicated
maps avoid the sidewalk recipe's tiled joints. Stone arches use the existing masonry maps.
Both use metre-based projection on horizontal and vertical faces; steel rails use the palette's
metal tint. Single-track decks are 4.8 m wide, with room around the 3.2 m ballast bed for
maintenance access. Low kerbs and open railings replace concrete walls on viaducts, narrower
columns carry transverse caps, and bearing walls tie the ends to the ground. Stone arch
bridges keep their solid masonry parapets. Beam ends, slab ends and exposed cap undersides
are closed. Generator 12 adds this geometry; the material scan adds no dependency.
Ridden bridges keep the wearing course inside their kerbs, without the rural road's
gravel and grass skirts hanging off the deck.

Road, street, path and railway ribbons also draw their exposed backs with plain underside
materials, so markings, grass strips and rail heads stay on top. This rendering change
applies to kept worlds without regeneration. Review the Oberalp railway crossing, bridge
side views and the four materials' above/below opacity test before changing this group.

```sh
scripts/art.sh blender --background --factory-startup --python art/textures/build.py -- --out screenshots/textures --size 512
```

## Buildings (`buildings/`)

- `catalogue.json` lists the models: kind, size, storeys, roof pitch and cover.
- `kinds.py` builds each kind: `house`, `chalet`, `farmhouse` (Bernese, with the Ründi arch),
  `church` (nave, choir, tower with clocks and a needle spire or saddle roof), `chapel`
  (with a roof turret), `shed` (also garages), `office` (glazed ground floor, a band of glass
  along every storey), `hotel` (balconies in every column, an entrance canopy), `public`
  (schools, town halls, hospitals: an entrance bay under a canopy on columns and a flag;
  classic with a stone ground floor and a hipped roof, or modern, flat-roofed with a band of
  colour at every floor), `castle` (#137: a keep with crenellated walls round a steep hipped
  roof, eight-sided corner towers under pointed roofs, small arched windows and an arched
  gate), `lighthouse` (#137: a round tower tapering in bands of white and `accent` colour from a
  stone plinth, a gallery with a solid railing, a glazed lantern under a pointed cap) and
  `tropical` (#136: houses of the subtropics such as Okinawa and Ishigaki, plastered concrete
  with sun slabs over the windows, under a flat roof with a water tank or a low hipped roof of
  red tiles with white ridges and wide eaves). Castles' and lighthouses' walls reach 8 m below
  the ground, as they stand on hilltops and rocks.
- `kit.py` holds the pieces: walls with recessed openings, windows with frames, sills and
  shutters, doors, balconies with solid balustrades, flower boxes, gable, hipped and
  half-hipped roofs with thick edges, flat roofs behind a parapet with a cornice, bands round
  the walls, canopies, roof machinery, flags, spires and clocks. Parts are chunky and few (ADR 0011):
  no gutters, downpipes, rafters or glazing bars, nothing that does not show at riding
  distance.

Models use metres with x along the building, y across it and z up; the origin is the centre of
the footprint at ground level, and walls reach 3 m below it for slopes. Texture coordinates
are metres on each surface (walls: along and up; roofs: along the eaves and up the slope).

The files carry no textures: each face has a material **name**, and the app gives every name
a texture (from `textures/`) tinted by a palette colour (`buildings.*` in
`app/assets/palette.json`) in
`app/scenes/building_models.gd` and `app/shaders/building_model.gdshader`. New names need an
entry there. Keep the models lean — a house 300–1,200 faces, a chalet or farmhouse up to about
2,000, the rarer offices, hotels and public buildings up to about 3,000 — since towns place
thousands of them; `build.py` prints the counts.

| Name | Used for |
|---|---|
| `plaster` | rendered walls; the colour varies per building |
| `nordic_wood` | painted plank siding on Nordic houses; the colour varies per building |
| `stone` | plinths, sills, quoins |
| `castle_stone` | Regional keeps' stone platforms, using the existing natural-rock scan |
| `wood`, `wood_dark`, `wood_light` | boarded walls, balconies; beams and log ends; the Ründi |
| `frame`, `glass`, `leaded` | window frames; glass, and stained glass in churches |
| `shutter`, `door`, `garage` | shutters (colour varies per building, some have none), doors |
| `tiles`, `slate`, `sheet` | roofs: tiles in the building's roof colour, slate, sheet metal |
| `roof_flat` | flat roofs (gravel) |
| `accent` | cornices, bands, canopies, balconies and flags of offices, hotels and public buildings; the colour varies per building (their roof colour where they have a pitched roof) |
| `metal`, `copper` | caps, finials; spires |
| `flowers`, `leaves` | geraniums in boxes and on balconies (one colour per box) |
| `clock` | clock dials |

The build also writes `models.json` next to the models: each model's kind, roof, the footprint
its walls stand on (`length` along x, `width` along y), `eaves` and total `height`. The world
reads it to fit models to the outlines on the map.

Every building is in the [gallery](#buildings).

The Asian starter set (`japanese.py`, R164) uses six Japanese-inspired `japanese_house`
models with pale or coloured plaster, dark timber lattice, deep swept tiled eaves and ridge
caps, plus three `japanese_castle` keeps with battered stone bases and three or five
receding roofs. Eight wall colours and six roof colours give neighbouring buildings more
variety. The Nordic set (`nordic.py`) adds four painted-plank houses with bright trim and
steep gables, plus three stone keeps with four corner towers. Seven wall colours and five
roof colours span red, ochre, cream, green and blue. The French set (`french.py`) adds four
plastered village houses with shutters, two Haussmann-inspired city houses with iron balconies
and zinc mansard roofs, and three château-inspired stone keeps with roofed corner pavilions.
Six pale or muted village wall colours and five slate or terracotta roof colours sit alongside
five limestone city wall colours and four zinc roof colours. Regional selectors preserve
tropical lowland houses; castles remain mapped points of interest. See
[regional buildings](../docs/regional-buildings.md) for placement, references, rebuild
commands and validation. These are own-work archetypes, not replicas.

## Vegetation (`vegetation/`)

`build.py` holds the catalogue and the kinds in one file: `conifer` (a spruce: a trunk under
whorls of needle sprays drooping out, long at the bottom and short at the top, and one
centered, tapered leader shared by its nearby, medium and far forms),
`broadleaf` (a trunk forking into limbs that reach into crowns of leaf clusters), `bush` (clumps
of leaf clusters) and `rock` (asymmetric masses cut along seven weathered fracture planes,
shaded smooth); for the subtropics (#136) `palm` (coconut palms with a curving trunk under
drooping fronds and a few coconuts, and a fan palm), `banana` (a stem under big paddle leaves)
and `tropical_bush` (leaf clusters with long leaves fanning out). Crowns are foliage cards:
quads textured with a cluster of leaves or a spray of needles cut out by its alpha, their
normals pointing out of the crown so it is lit as one soft volume. Trees are 300–550
triangles, rocks 320–640, palms up to about 1,000; random shapes use fixed seeds, so rebuilds
give the same files.

The conifer crown envelope tapers through its upper whorls into a short, slender terminal
shoot, lighter than the branches below it. The stem supports those upper whorls.
[Front, side, rear and elevated model renders](../docs/images/models/conifer-single-peak.jpg)
show both the tall and broad forms; the same leader is retained in their authored LODs.

`fern` has spreading and upright crowns of arching fronds, each with paired, tapered
leaflets and raised midribs. These are solid geometry seen from both sides, about
1,400–1,500 triangles per plant, with no extra texture. The world varies their size, heading
and colour (`plants.ferns`) and places them in patches under mapped woods within 60 m of the
road. They share the existing undergrowth budget and use the short small-plant draw distance
(R104), with the same exclusions as other undergrowth.

`foliage.py` draws leaf, needle and five-petal blossom cards (`leaves_albedo.webp`,
`needles_albedo.webp`, `blossoms_albedo.png`) on transparent ground into
`app/assets/textures/`. The app tints them per plant; Spring's selected broadleaf crowns
use the blossom atlas, including authored LODs. `--blossoms-only` rebuilds that atlas alone.
The small blossom alpha mask stays lossless PNG because the emulated Blender WebP encoder
introduced line artifacts; Godot still imports it into the same mipmapped VRAM format.

```sh
scripts/art.sh blender --background --factory-startup --python art/vegetation/foliage.py
scripts/art.sh blender --background --factory-startup --python art/vegetation/build.py
scripts/art.sh blender --background --factory-startup --python art/vegetation/lods.py
```

Trees also have authored medium and far forms in `app/assets/models/vegetation/lods/`:
fewer, broader sprays and leaf clusters, simpler trunks and fronds, with the same species,
crown volumes and ground origins. They are rendering assets outside `models.json`.
`TreeLodMesh` combines them with the original nearby geometry for Godot's automatic,
screen-size-based LOD selection in the visible scene and shadows. No trees are removed by
LOD. Medium–Ultra split tree instances into 160 m batches; Low keeps 480 m batches to avoid
extra CPU and draw calls. Kept world files need no changes or regeneration.

| Name | Used for |
|---|---|
| `leaves`, `needles` | foliage cards of broadleaf trees and bushes, and of conifers; tinted per plant (palette `plants.conifers`, `plants.broadleaves`, `plants.bushes`, `plants.tropical`) |
| `frond` | solid leaves of palms, banana plants and ferns, tinted per plant (`plants.palms`, `plants.tropical`, `plants.ferns`) |
| `trunk` | trunks and limbs, the bark texture by coordinates in metres around and up |
| `rock` | rocks, the rock texture laid on from three sides, tinted per rock (`plants.rocks`) |

The app gives each name its look in `app/scenes/vegetation_models.gd`,
`app/shaders/vegetation.gdshader` and `app/shaders/foliage.gdshader` (cards seen from both
sides, lit through from behind; trees sway in the wind, leaves flutter). `models.json` lists
each model's `kind` and `height`; the world (`core/orqwa-world/src/vegetation.rs`) picks one
model of the kind it wants per plant. Every plant is in the [gallery](#vegetation).

Mapped forests use jittered 4 m candidate spacing within 300 m of the road and thin to
an equivalent 10 m spacing beyond it (R103). Wooded hillsides keep their trees even on steep
ground; open land keeps sparse plants on its separate 7 m grid. Roads, railways, water,
buildings and mapped clearings stay clear. Ultra draws the full tree population; Low,
Medium and High upload stable subsets of about 12.5 %, 25 % and 50 % (R108). Tree ranges
rise from 600 m on Low to 3,240 m on Ultra; View distance overrides range independently.
The separate Tree density detail overrides the population independently of the preset;
Auto follows it again (R109). Sampling happens before GPU upload and leaves kept worlds
intact, without regeneration.

## Props (`props/`)

`build.py` makes what stands by the road ridden (R92) into `app/assets/models/props/`:
`guard_rail` (a 4 m span of W-beam on a post, with the spacer block; 88 triangles),
`guard_rail_end` and `guard_rail_start` (a span twisting down into the ground ahead or behind,
flaring away from the road, as a run of rail begins and ends), `delineator` (a 1 m white post,
its sloping top and black band with a reflector facing the traffic) and `snow_pole` (2.5 m,
red and white in half-metre bands); in towns `street_lamp` (6.5 m, its arm bending over the
road to a flat luminaire), `bench` (wooden slats on two cast frames), `bin` (a drum on a post)
and `bus_shelter` (glass back and ends in thin frames, a flat roof, a bench inside); in the
countryside `fence` (a 3 m span of two split rails between weathered posts), `hay_bale` and
`hay_bale_wrapped` (round bales lying on their side, straw or white silage wrap) and
`wayside_cross` (a wooden cross on a stone foot under a little roof); and road signs on a
galvanised pole: `sign_place` (a blue panel with a white rim) and `sign_place_end` (the same
with a red bar across), whose names are the app's text on their faces, and the warning
triangles `sign_bend_left`, `sign_bend_right` (a curving arrow) and `sign_grade` (a slope,
the grade as text); and `power_pole` (a tapered wooden pole with a crossarm and three
insulators). Each model's +x points away from the road, its length
runs along it, its origin on the ground where it stands; parts reach 0.4 m below it to meet
sloping ground. The world shears the rails to the road's grade, so their posts stay upright.

```sh
scripts/art.sh blender --background --factory-startup --python art/props/build.py
```

| Name | Used for |
|---|---|
| `galvanised` | the rails' beams, posts and blocks (`props.galvanised`, metallic) |
| `post`, `band`, `reflector` | the delineator's white post, black band and orange reflector |
| `pole_red`, `pole_white` | the snow pole's bands |
| `metal_dark` | lamps, bench frames, bins, the shelter's frame and roof |
| `wood` | bench and shelter seats |
| `lamp` | the luminaire's glass, faintly lit |
| `glass` | the shelter's panes, see-through |
| `wood_weathered` | fences and wayside crosses |
| `hay`, `wrap` | the bales: straw, or white plastic |
| `stone` | the wayside cross's foot |
| `sign_blue`, `sign_white`, `sign_red`, `sign_black` | the signs' faces and symbols |
| `insulator` | the power poles' insulators |

The app colours each name from the palette's `props` section (`app/scenes/prop_models.gd`).
The world (`core/orqwa-world/src/props.rs`) places them by rules from the road's shape and
the ground beside it: guard rails, span by span, on the side where the ground 4.5 m beyond
the edge lies 2 m or more below the road, with a start and an end span at each run and none
where a street joins; delineator posts every 50 m on both sides of country roads; snow poles
every 25 m above 1,300 m, behind the rails where there are any. Bridges and tunnels get
none. Towns — built-up land on the map, or four buildings within 60 m where it maps none —
get no posts but street lamps every 60 m on each side, staggered; bench candidates every
120 m on alternate sidewalks, facing the road with a bin beside each; and a bus shelter
every 400 m on the right. Benches use the raised sidewalk height and need a continuous,
nearly level surface outside entrances; a blocked candidate tries the opposite side and
nearby spots within 20 m along the block.
Their backs sit near the outer sidewalk edge, leaving the inner walking strip open. Each stands only
where it has room: clear of the buildings' walls, side streets and where streets join. In the
country, about a third of the 120 m stretches beside pasture or field (or open land the map
leaves unmarked) are fenced, span by span, except where a rail stands; round bales lie in
fields 12–90 m from the road, one to three together; and a wayside cross stands just past
about one in five of the lanes joining the road outside towns. Road signs stand on the right of
travel: where the road enters a town (by majority over 200 m, so a gap between houses ends
none) the nearest place's name from the map, and its end sign where it leaves; outside towns
a bend sign 80 m before a hairpin (more than 100° within 60 m) and a grade sign before 300 m
climbing or falling at 9 % or more. Power lines follow about two in five 600 m stretches of
country road, on one side, 9 m from the centre: a pole every 50 m and three wires sagging
from pole to pole, built by the world as geometry (`wire` in the palette) since no fixed model
bends with the road. Every prop is in the [gallery](#props).

## Riders (`riders/`)

`build.py` makes a female and a male rider, each on their own bike, into
`app/assets/models/riders/rider_<sex>.glb`:

```sh
scripts/art.sh blender --background --factory-startup --python art/riders/build.py
scripts/dev.sh scripts/render-riders.sh   # side, front, chase and face views in screenshots/riders/
```

- **Bodies** are the realistic female and male bodies of Blender Studio's CC0 *Human Base
  Meshes*, extracted into `sources/human-base-meshes/realistic_bodies.blend` with the bundle's
  skeleton joints (see its `SOURCE.md` and `extract_realistic.py`). A skeleton is built from
  those joints, scaled to each body, and the body skinned to it (heat weights on a thinned
  copy, handed on by nearest surface). He is scaled to 1.76 m; she keeps the body's 1.64 m.
- **Head:** fitted to the head's own shape. Hair is a shell over the scalp (to the nape and
  over the ears for her, short for him). The road-racing helmet is fitted round the whole
  skull, its brow just above the sunglasses. Its white, unbranded shell follows the
  [Lazer Z1 KinetiCore reference and 360° view](https://lazersport.us/products/z1kineticore):
  ribs that meet at the fit dial at the back, a spine inside a ring of vents, long side
  channels down to a low vent at the temple, recessed cross braces, exhausts beside the spine
  and solid bands above the rim. `riders/helmet.py` carves it from a signed distance field, so
  the vent edges come out rounded, meshes it with Blender's own OpenVDB and thins it to about
  9,500 triangles; a couple of millimetres into each vent the white skin gives way to dark
  foam. The lower rim clears the ears and carries the chin straps.
  Sunglasses are two lenses on a shield wrapped round the face in front of the eyes
  (found from the nose's tip), a bridge and arms back to the ears; she has a ponytail with
  a hair tie. The headgear moves with the head bone. The reference photos are not bundled;
  the model is generated by the project's own script.
- **Seat:** the saddle and hip fit have a 20 mm forward rail adjustment. Height is solved
  again for a 30° knee bend with the pedal at the bottom, with the sit bones on the saddle
  9 cm below the hip joints. `bike_fit.py` sets the frame
  stack, reach, steering geometry, stem and bar width before the rider is posed, using the
  published Aethos 49/54 geometry as a dimensional reference. The back bends forward, mostly
  at the pelvis, until slightly bent arms reach the fixed hoods (about 44° above horizontal
  for her, 41° for him). Neck and head are aimed so the rider looks up the road; hands rest on the
  hoods, knuckles over the horns, each finger curled joint by joint round the horn's front and
  the thumb forward along the hood's inner side; feet on the pedals with the knees forward,
  the soles on them. The riding pose becomes the skeleton's rest.
- **Kit:** after thinning the posed body (the hands and helmet spared, so fingers stay round
  and vent edges stay smooth), the kit's edges are cut into
  it as planes across the limbs and the neck, so they run straight: shorts to above the knee,
  a jersey with a collar and sleeves to half way down the upper arm, socks and white shoes;
  the hands are bare, their fingers curled round the hoods. Each face takes the region of the
  bone that moves it most. The shoes are the convex hulls of the feet.
- **Bike:** reference-sized road frames, with the saddle fitted to the legs and the torso and
  arms posed to the frame's hoods; see below.

Each file holds `skeleton` with `body` skinned to it (bones `pelvis`, `spine`, `chest`, `neck`,
`head`, `shoulder`/`upper_arm`/`forearm`/`hand`, three a finger (`thumb_1`…`little_3`) and
`thigh`/`shin`/`foot`/`toe`, each `.L` and `.R`), `frame`, `wheel_front`, `wheel_rear`, `crankset` (right crank forward) and `pedal_l`,
`pedal_r`. `riders.json` holds each rider's fit: hips, thigh and shin lengths, where the ankle
and the ball of the foot sit over the pedal, the hands on the hoods, hubs, bottom bracket and
cranks.
`app/scenes/rider_avatar.gd` turns wheels and cranks, turns the leg bones to the pedals
(inverse kinematics) and moves the pelvis bone, and all above it, as the rider sways and stands.
Exports are deterministic. `PREVIEW=<dir>` also renders Cycles previews of the riders there.

| Name | Used for |
|---|---|
| `skin`, `hair` | skin; hair and ponytail |
| `jersey`, `sleeve`, `shorts`, `socks`, `shoes` | the kit |
| `helmet`, `vents`, `glasses` | helmet and its vent slots, sunglasses |
| `frame` | the frame and fork |
| `tyre`, `sidewall` | rubber tread and the darker brown casing |
| `rim`, `carbon` | satin composite rims, cockpit, seat post and bottle cage |
| `metal`, `alloy` | bright steel drivetrain/rotors and dark anodized hardware |
| `saddle`, `bar`, `bottle` | saddle cover, rubber grips and the bidon |

Colours: the rider's own palette section (`rider_female`, `rider_male`; `frame` there too) and
`bike` for the bike's other parts. The frame has a non-metallic clear coat; only `metal` and
`alloy` use metallic reflection. Rubber stays rough and composite parts have a satin finish.
The bike is a modern race bike: a compact frame of shaped,
squared-off tubes (a deep down tube, tall head tube, aero seat tube, dropped seat stays), a
straight tapered fork, a black aero seat post on the seat tube's line and a clamped cockpit
with round headset bearings, a 6° rising stem, compact round drops,
upright shifter horns and separate brake levers with pivots and end plugs; a compact 21 cm saddle
on shortened rails; a shaped bottle in an open cage;
35 mm carbon rims with 24 thinner, crossed spokes, valve stems and vented disc rotors;
an open-centred 11-speed cassette, dark toothed chainrings on a spider, tapered cranks, a rear
derailleur and a continuous chain with spaced side plates. Painted tube intersections are
voxel-welded at 3 mm, rounded and reduced to 1,800 triangles. The separate arched fork crown
joins the final mesh after that step, preserving its headset joint.
A rider has about 41,000–43,000 triangles, a bike 18,036 (previously about 9,800); the whole
avatar grows by about 16%, with the same six bike objects and no per-link animation.
`riders.json` is rebuilt with the updated hips, hoods and hubs. Node/bone names and metadata
fields stay stable, and kept worlds need no regeneration.
Both riders, from every side, are in the [gallery](#riders).

The rider renderer uses the app's atmospheric sky, AgX tone mapping and midday light energy.
For bike-only close-ups, run `VIEWS="bike-side bike-drive bike-front bike-cockpit"` with
`scripts/render-riders.sh`; `bike-clearance` and `bike-saddle` focus on tyre/frame separation
and saddle proportions. The small/medium frames have approximately 975/979 mm wheelbases,
80/100 mm stems and 380/420 mm bars; their hoods follow that layout instead of setting it.
See the [bike review](../docs/bike-appearance.md) for matched
before/after images and validation.

## The remote's clip (`remote/`)

Not a model: `keep_awake.py` renders the silent clip the phone remote's page plays, unseen, to
keep the phone's screen on (R88, ADR 0019), into `core/orqwa-remote/src/keep-awake.mp4`. Four
seconds of black, 64 px, with a silent sound track, in H.264 and AAC, about 3.6 kB, from the
sequencer, so no GPU is needed. Blender's own FFmpeg encodes the H.264 the dev container's
cannot. Keep the sound track: Safari and Firefox keep the screen on only for a video that has
one.

```sh
scripts/art.sh blender --background --factory-startup --python art/remote/keep_awake.py
```

## Gallery

Every model as the world draws it, in its palette colours and light: buildings, plants and
props in three variants side by side (colours vary per building and plant), the riders from the views
`scripts/render-riders.sh` takes. After changing a model, render the pictures again in the dev
container (a run replaces its group's pictures, so a model taken out leaves none behind) and
update the tables if models were added or removed:

```sh
scripts/dev.sh sh -c 'GALLERY=docs/images/models scripts/render-models.sh'
scripts/dev.sh sh -c 'GALLERY=docs/images/models scripts/render-riders.sh'
```

### Buildings

| | | |
|---|---|---|
| ![house_gable_1_m](../docs/images/models/buildings/house_gable_1_m.jpg)<br>`house_gable_1_m` | ![house_gable_1_s](../docs/images/models/buildings/house_gable_1_s.jpg)<br>`house_gable_1_s` | ![house_gable_2_l](../docs/images/models/buildings/house_gable_2_l.jpg)<br>`house_gable_2_l` |
| ![house_gable_2_m](../docs/images/models/buildings/house_gable_2_m.jpg)<br>`house_gable_2_m` | ![house_gable_2_s](../docs/images/models/buildings/house_gable_2_s.jpg)<br>`house_gable_2_s` | ![house_gable_3_l](../docs/images/models/buildings/house_gable_3_l.jpg)<br>`house_gable_3_l` |
| ![house_gable_3_m](../docs/images/models/buildings/house_gable_3_m.jpg)<br>`house_gable_3_m` | ![house_hipped_2_l](../docs/images/models/buildings/house_hipped_2_l.jpg)<br>`house_hipped_2_l` | ![house_hipped_2_m](../docs/images/models/buildings/house_hipped_2_m.jpg)<br>`house_hipped_2_m` |
| ![house_hipped_3_l](../docs/images/models/buildings/house_hipped_3_l.jpg)<br>`house_hipped_3_l` | ![chalet_2_m](../docs/images/models/buildings/chalet_2_m.jpg)<br>`chalet_2_m` | ![chalet_2_s](../docs/images/models/buildings/chalet_2_s.jpg)<br>`chalet_2_s` |
| ![chalet_3_l](../docs/images/models/buildings/chalet_3_l.jpg)<br>`chalet_3_l` | ![chalet_3_m](../docs/images/models/buildings/chalet_3_m.jpg)<br>`chalet_3_m` | ![farmhouse_l](../docs/images/models/buildings/farmhouse_l.jpg)<br>`farmhouse_l` |
| ![farmhouse_m](../docs/images/models/buildings/farmhouse_m.jpg)<br>`farmhouse_m` | ![farmhouse_xl](../docs/images/models/buildings/farmhouse_xl.jpg)<br>`farmhouse_xl` | ![church_needle_l](../docs/images/models/buildings/church_needle_l.jpg)<br>`church_needle_l` |
| ![church_needle_m](../docs/images/models/buildings/church_needle_m.jpg)<br>`church_needle_m` | ![church_saddle_m](../docs/images/models/buildings/church_saddle_m.jpg)<br>`church_saddle_m` | ![chapel_m](../docs/images/models/buildings/chapel_m.jpg)<br>`chapel_m` |
| ![chapel_s](../docs/images/models/buildings/chapel_s.jpg)<br>`chapel_s` | ![garage_m](../docs/images/models/buildings/garage_m.jpg)<br>`garage_m` | ![shed_m](../docs/images/models/buildings/shed_m.jpg)<br>`shed_m` |
| ![shed_s](../docs/images/models/buildings/shed_s.jpg)<br>`shed_s` | ![office_2_xl](../docs/images/models/buildings/office_2_xl.jpg)<br>`office_2_xl` | ![office_3_m](../docs/images/models/buildings/office_3_m.jpg)<br>`office_3_m` |
| ![office_4_l](../docs/images/models/buildings/office_4_l.jpg)<br>`office_4_l` | ![office_6_l](../docs/images/models/buildings/office_6_l.jpg)<br>`office_6_l` | ![hotel_3_m](../docs/images/models/buildings/hotel_3_m.jpg)<br>`hotel_3_m` |
| ![hotel_4_m](../docs/images/models/buildings/hotel_4_m.jpg)<br>`hotel_4_m` | ![hotel_6_l](../docs/images/models/buildings/hotel_6_l.jpg)<br>`hotel_6_l` | ![public_flat_2_l](../docs/images/models/buildings/public_flat_2_l.jpg)<br>`public_flat_2_l` |
| ![public_flat_3_xl](../docs/images/models/buildings/public_flat_3_xl.jpg)<br>`public_flat_3_xl` | ![public_flat_4_l](../docs/images/models/buildings/public_flat_4_l.jpg)<br>`public_flat_4_l` | ![public_hipped_2_m](../docs/images/models/buildings/public_hipped_2_m.jpg)<br>`public_hipped_2_m` |
| ![public_hipped_3_l](../docs/images/models/buildings/public_hipped_3_l.jpg)<br>`public_hipped_3_l` | ![castle_l](../docs/images/models/buildings/castle_l.jpg)<br>`castle_l` | ![castle_m](../docs/images/models/buildings/castle_m.jpg)<br>`castle_m` |
| ![castle_s](../docs/images/models/buildings/castle_s.jpg)<br>`castle_s` | ![lighthouse_l](../docs/images/models/buildings/lighthouse_l.jpg)<br>`lighthouse_l` | ![lighthouse_s](../docs/images/models/buildings/lighthouse_s.jpg)<br>`lighthouse_s` |
| ![tropical_flat_1_m](../docs/images/models/buildings/tropical_flat_1_m.jpg)<br>`tropical_flat_1_m` | ![tropical_flat_2_l](../docs/images/models/buildings/tropical_flat_2_l.jpg)<br>`tropical_flat_2_l` | ![tropical_flat_2_m](../docs/images/models/buildings/tropical_flat_2_m.jpg)<br>`tropical_flat_2_m` |
| ![tropical_hipped_1_m](../docs/images/models/buildings/tropical_hipped_1_m.jpg)<br>`tropical_hipped_1_m` | ![tropical_hipped_1_s](../docs/images/models/buildings/tropical_hipped_1_s.jpg)<br>`tropical_hipped_1_s` |  |

Asian starter set (Japanese-inspired models):

| | | |
|---|---|---|
| ![Japanese one-storey house, small](../docs/images/models/buildings/japanese_house_1_s.jpg)<br>`japanese_house_1_s` | ![Japanese one-storey house](../docs/images/models/buildings/japanese_house_1_m.jpg)<br>`japanese_house_1_m` | ![Japanese hipped house](../docs/images/models/buildings/japanese_house_hipped_1_m.jpg)<br>`japanese_house_hipped_1_m` |
| ![Japanese two-storey house, small](../docs/images/models/buildings/japanese_house_2_s.jpg)<br>`japanese_house_2_s` | ![Japanese two-storey house](../docs/images/models/buildings/japanese_house_2_m.jpg)<br>`japanese_house_2_m` | ![Japanese two-storey house, large](../docs/images/models/buildings/japanese_house_2_l.jpg)<br>`japanese_house_2_l` |
| ![Japanese three-tier keep, small](../docs/images/models/buildings/japanese_castle_3_s.jpg)<br>`japanese_castle_3_s` | ![Japanese three-tier keep](../docs/images/models/buildings/japanese_castle_3_m.jpg)<br>`japanese_castle_3_m` | ![Japanese five-tier keep](../docs/images/models/buildings/japanese_castle_5_l.jpg)<br>`japanese_castle_5_l` |

Nordic starter set:

| | | |
|---|---|---|
| ![Nordic one-storey house](../docs/images/models/buildings/nordic_house_1_s.jpg)<br>`nordic_house_1_s` | ![Nordic one-storey house](../docs/images/models/buildings/nordic_house_1_m.jpg)<br>`nordic_house_1_m` | ![Nordic two-storey house](../docs/images/models/buildings/nordic_house_2_m.jpg)<br>`nordic_house_2_m` |
| ![Nordic two-storey house, large](../docs/images/models/buildings/nordic_house_2_l.jpg)<br>`nordic_house_2_l` | ![Nordic three-storey keep](../docs/images/models/buildings/nordic_castle_3_s.jpg)<br>`nordic_castle_3_s` | ![Nordic three-storey keep](../docs/images/models/buildings/nordic_castle_3_m.jpg)<br>`nordic_castle_3_m` |
| ![Nordic four-storey keep](../docs/images/models/buildings/nordic_castle_4_l.jpg)<br>`nordic_castle_4_l` | | |

French starter set:

| | | |
|---|---|---|
| ![French one-storey house](../docs/images/models/buildings/french_house_1_s.jpg)<br>`french_house_1_s` | ![French one-storey house, medium](../docs/images/models/buildings/french_house_1_m.jpg)<br>`french_house_1_m` | ![French two-storey house](../docs/images/models/buildings/french_house_2_m.jpg)<br>`french_house_2_m` |
| ![French two-storey house, large](../docs/images/models/buildings/french_house_2_l.jpg)<br>`french_house_2_l` | ![French three-storey château keep](../docs/images/models/buildings/french_castle_3_s.jpg)<br>`french_castle_3_s` | ![French three-storey château keep, medium](../docs/images/models/buildings/french_castle_3_m.jpg)<br>`french_castle_3_m` |
| ![French four-storey château keep](../docs/images/models/buildings/french_castle_4_l.jpg)<br>`french_castle_4_l` | | |

French city houses:

| | |
|---|---|
| ![French five-storey city apartment](../docs/images/models/buildings/french_city_5_m.jpg)<br>`french_city_5_m` | ![French six-storey city apartment](../docs/images/models/buildings/french_city_6_l.jpg)<br>`french_city_6_l` |

### Tree detail levels

Each comparison shows the unchanged nearby model, then its authored medium and far forms.
The app switches by projected size, so the simpler forms are normally much smaller on screen.
These are geometry references; [native forest measurements](../docs/benchmarks/tree-lods.md)
and route screenshots check their cost and coverage in context.

| Conifers | Broadleaf trees |
|---|---|
| ![Tall conifer detail levels](../docs/images/models/vegetation-lods/conifer_tall.jpg) | ![Round broadleaf detail levels](../docs/images/models/vegetation-lods/broadleaf_round.jpg) |
| ![Broad conifer detail levels](../docs/images/models/vegetation-lods/conifer_broad.jpg) | ![Clustered broadleaf detail levels](../docs/images/models/vegetation-lods/broadleaf_cluster.jpg) |

| Tropical trees | Tropical trees |
|---|---|
| ![Coconut palm detail levels](../docs/images/models/vegetation-lods/palm_coconut.jpg) | ![Short palm detail levels](../docs/images/models/vegetation-lods/palm_short.jpg) |
| ![Fan palm detail levels](../docs/images/models/vegetation-lods/palm_fan.jpg) | ![Banana detail levels](../docs/images/models/vegetation-lods/banana.jpg) |

### Vegetation

| | | |
|---|---|---|
| ![conifer_broad](../docs/images/models/vegetation/conifer_broad.jpg)<br>`conifer_broad` | ![conifer_tall](../docs/images/models/vegetation/conifer_tall.jpg)<br>`conifer_tall` | ![broadleaf_cluster](../docs/images/models/vegetation/broadleaf_cluster.jpg)<br>`broadleaf_cluster` |
| ![broadleaf_round](../docs/images/models/vegetation/broadleaf_round.jpg)<br>`broadleaf_round` | ![bush](../docs/images/models/vegetation/bush.jpg)<br>`bush` | ![rock_block](../docs/images/models/vegetation/rock_block.jpg)<br>`rock_block` |
| ![rock_pair](../docs/images/models/vegetation/rock_pair.jpg)<br>`rock_pair` | ![palm_coconut](../docs/images/models/vegetation/palm_coconut.jpg)<br>`palm_coconut` | ![palm_fan](../docs/images/models/vegetation/palm_fan.jpg)<br>`palm_fan` |
| ![palm_short](../docs/images/models/vegetation/palm_short.jpg)<br>`palm_short` | ![banana](../docs/images/models/vegetation/banana.jpg)<br>`banana` | ![tropical_bush](../docs/images/models/vegetation/tropical_bush.jpg)<br>`tropical_bush` |
| ![fern_spreading](../docs/images/models/vegetation/fern_spreading.jpg)<br>`fern_spreading` | ![fern_upright](../docs/images/models/vegetation/fern_upright.jpg)<br>`fern_upright` | |

### Props

| | | |
|---|---|---|
| ![guard_rail](../docs/images/models/props/guard_rail.jpg)<br>`guard_rail` | ![guard_rail_start](../docs/images/models/props/guard_rail_start.jpg)<br>`guard_rail_start` | ![guard_rail_end](../docs/images/models/props/guard_rail_end.jpg)<br>`guard_rail_end` |
| ![delineator](../docs/images/models/props/delineator.jpg)<br>`delineator` | ![snow_pole](../docs/images/models/props/snow_pole.jpg)<br>`snow_pole` | ![street_lamp](../docs/images/models/props/street_lamp.jpg)<br>`street_lamp` |
| ![bench](../docs/images/models/props/bench.jpg)<br>`bench` | ![bin](../docs/images/models/props/bin.jpg)<br>`bin` | ![bus_shelter](../docs/images/models/props/bus_shelter.jpg)<br>`bus_shelter` |
| ![fence](../docs/images/models/props/fence.jpg)<br>`fence` | ![hay_bale](../docs/images/models/props/hay_bale.jpg)<br>`hay_bale` | ![hay_bale_wrapped](../docs/images/models/props/hay_bale_wrapped.jpg)<br>`hay_bale_wrapped` |
| ![wayside_cross](../docs/images/models/props/wayside_cross.jpg)<br>`wayside_cross` | ![sign_place](../docs/images/models/props/sign_place.jpg)<br>`sign_place` | ![sign_place_end](../docs/images/models/props/sign_place_end.jpg)<br>`sign_place_end` |
| ![sign_bend_left](../docs/images/models/props/sign_bend_left.jpg)<br>`sign_bend_left` | ![sign_bend_right](../docs/images/models/props/sign_bend_right.jpg)<br>`sign_bend_right` | ![sign_grade](../docs/images/models/props/sign_grade.jpg)<br>`sign_grade` |
| ![power_pole](../docs/images/models/props/power_pole.jpg)<br>`power_pole` | | |

### Riders

| | Female | Male |
|---|---|---|
| side | ![female side](../docs/images/models/riders/female-side.jpg) | ![male side](../docs/images/models/riders/male-side.jpg) |
| side-down | ![female side-down](../docs/images/models/riders/female-side-down.jpg) | ![male side-down](../docs/images/models/riders/male-side-down.jpg) |
| standing | ![female standing](../docs/images/models/riders/female-standing.jpg) | ![male standing](../docs/images/models/riders/male-standing.jpg) |
| standing-chase | ![female standing-chase](../docs/images/models/riders/female-standing-chase.jpg) | ![male standing-chase](../docs/images/models/riders/male-standing-chase.jpg) |
| front | ![female front](../docs/images/models/riders/female-front.jpg) | ![male front](../docs/images/models/riders/male-front.jpg) |
| chase | ![female chase](../docs/images/models/riders/female-chase.jpg) | ![male chase](../docs/images/models/riders/male-chase.jpg) |
| face | ![female face](../docs/images/models/riders/female-face.jpg) | ![male face](../docs/images/models/riders/male-face.jpg) |
| head | ![female head](../docs/images/models/riders/female-head.jpg) | ![male head](../docs/images/models/riders/male-head.jpg) |
| helmet side | ![female helmet side](../docs/images/models/riders/female-helmet-side.jpg) | ![male helmet side](../docs/images/models/riders/male-helmet-side.jpg) |
| helmet rear | ![female helmet rear](../docs/images/models/riders/female-helmet-rear.jpg) | ![male helmet rear](../docs/images/models/riders/male-helmet-rear.jpg) |
| lean | ![female lean](../docs/images/models/riders/female-lean.jpg) | ![male lean](../docs/images/models/riders/male-lean.jpg) |
| bike side | ![female bike side](../docs/images/models/riders/female-bike-side.jpg) | ![male bike side](../docs/images/models/riders/male-bike-side.jpg) |
| drivetrain | ![female drivetrain](../docs/images/models/riders/female-bike-drive.jpg) | ![male drivetrain](../docs/images/models/riders/male-bike-drive.jpg) |
| bike front | ![female bike front](../docs/images/models/riders/female-bike-front.jpg) | ![male bike front](../docs/images/models/riders/male-bike-front.jpg) |
| cockpit | ![female cockpit](../docs/images/models/riders/female-bike-cockpit.jpg) | ![male cockpit](../docs/images/models/riders/male-bike-cockpit.jpg) |
| wheel clearance | ![female wheel clearance](../docs/images/models/riders/female-bike-clearance.jpg) | ![male wheel clearance](../docs/images/models/riders/male-bike-clearance.jpg) |
| saddle | ![female saddle](../docs/images/models/riders/female-bike-saddle.jpg) | ![male saddle](../docs/images/models/riders/male-bike-saddle.jpg) |
