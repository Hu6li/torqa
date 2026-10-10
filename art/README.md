# Art

Scripted 3D models for Torqa ([ADR 0009](../docs/adr/0009-asset-pipeline.md)). The scripts are
the source; the exported `.glb` files are committed so the app builds without Blender.

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
scripts/dev.sh sh -c 'GROUPS="vegetation clouds" scripts/render-models.sh'
```

After a change to the models, refresh the [gallery](#gallery) below as well.

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
a flat palette colour (`buildings.*` in `app/assets/palette.json`) in
`app/scenes/building_models.gd` and `app/shaders/building_model.gdshader`. New names need an
entry there. Keep the models lean — a house 300–1,200 faces, a chalet or farmhouse up to about
2,000, the rarer offices, hotels and public buildings up to about 3,000 — since towns place
thousands of them; `build.py` prints the counts.

| Name | Used for |
|---|---|
| `plaster` | rendered walls; the colour varies per building |
| `stone` | plinths, sills, quoins |
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

## Vegetation (`vegetation/`)

`build.py` holds the catalogue and the kinds in one file: `conifer` (a trunk under stacked
cones, each turned a little), `broadleaf` (a trunk under one or more chunky 20-facet blobs),
`bush` and `rock`; for the subtropics (#136) `palm` (coconut palms with a curving trunk under
drooping fronds and a few coconuts, and a fan palm), `banana` (a stem under big paddle leaves)
and `tropical_bush` (a clump with long leaves fanning out). Shapes are faceted (one normal per
face) and lean — 20 to 70 triangles, palms and other plants with leaves seen from both sides up
to about 300 — since forests place thousands; random shapes use fixed seeds, so rebuilds give
the same files.

```sh
scripts/art.sh blender --background --factory-startup --python art/vegetation/build.py
```

| Name | Used for |
|---|---|
| `leaves` | crowns, fronds, leaves and bushes; the colour varies per plant (palette `plants.conifers`, `plants.broadleaves`, `plants.bushes`, `plants.palms`, `plants.tropical`) |
| `trunk` | trunks (palette `plants.trunk`) |
| `rock` | rocks; the colour varies per rock (palette `plants.rocks`) |

The app gives each name its look in `app/scenes/vegetation_models.gd` and
`app/shaders/vegetation.gdshader` (trees sway a little in the wind). `models.json` lists each
model's `kind` and `height`; the world (`core/torqa-world/src/vegetation.rs`) picks one model
of the kind it wants per plant. Every plant is in the [gallery](#vegetation).

## Riders (`riders/`)

`build.py` makes a female and a male rider, each on their own bike, into
`app/assets/models/riders/rider_<sex>.glb`:

```sh
scripts/art.sh blender --background --factory-startup --python art/riders/build.py
scripts/dev.sh scripts/render-riders.sh   # side, front, chase and face views in screenshots/riders/
```

- **Bodies** come from Blender Studio's CC0 *Human Base Meshes*: the stylized "primitive"
  bodies, extracted into `sources/human-base-meshes/` (see its `SOURCE.md` and `extract.py`).
  They are parts with their origins at the joints. The script keeps them at their base level
  (faceted, no subdivision), drops fingers and toes, thins nose and ears, lengthens arms,
  hands and feet, makes the heads and shoulder caps smaller (natural proportions, ADR 0011) and
  dresses the parts: shorts to above the knee, short sleeves, gloves, shoes.
- **Head:** fitted to the head's own shape. Hair is a shell over the scalp (to the nape and
  over the ears for her, short for him), the helmet a shell over the hair, drawn out to the
  back, with vent slots laid on top; sunglasses are two lenses following the face over the
  eyes, a bridge and arms back to the ears; she has a ponytail with a hair tie.
- **Seat:** the hips go on the seat tube's line where the knee bends 30° with the pedal at the
  bottom (a usual bike fit); the torso bends forward until the hands, elbows a little bent,
  reach hoods at least 0.5 m ahead of the bottom bracket; the shoulders turn half the way with
  the arms.
- **Bike:** built around those contact points (saddle under the pelvis, hoods under the hands):
  frame of six-sided tubes, fork, stem, drop bar with hoods, saddle, 18-sided wheels with eight
  spokes, chainring and cranks.

Each file holds `body` (the rider's posed upper body), the legs as `thigh_l`, `shin_l`,
`foot_l`, `thigh_r`, `shin_r`, `foot_r` (origins at hip, knee and ankle), `frame`,
`wheel_front`, `wheel_rear`, `crankset` (right crank forward) and `pedal_l`, `pedal_r`.
`riders.json` holds each rider's fit: hips, thigh and shin lengths, where the ankle sits over
the pedal, hubs, bottom bracket and cranks. `app/scenes/rider_avatar.gd` turns wheels and cranks
and aims the legs at the pedals (inverse kinematics).

| Name | Used for |
|---|---|
| `skin`, `hair` | skin; hair and ponytail |
| `jersey`, `sleeve`, `shorts`, `gloves`, `shoes` | the kit |
| `helmet`, `vents`, `glasses` | helmet and its vent slots, sunglasses |
| `frame` | the frame and fork |
| `tyre`, `rim`, `metal`, `saddle`, `bar` | the bike's other parts |

Colours: the rider's own palette section (`rider_female`, `rider_male`; `frame` there too) and
`bike` for the bike's other parts. A rider about 4,400–5,300 triangles, a bike about 1,400.
Both riders, from every side, are in the [gallery](#riders).

## Clouds (`clouds/`)

`build.py` makes four low-poly clouds into `app/assets/models/clouds/`: small, puffy, long and
a towering one, each a few rough balls of 20 facets flattened underneath, 60–120 faces, about
10 m long (the app scales them up). One material, `cloud`; `app/shaders/cloud.gdshader` lights
them by the sun and the weather and fades them into the horizon, and `app/scenes/cloud_layer.gd`
spreads them round the camera, more, bigger and lower as the weather clouds over. They are
in the [gallery](#clouds) too.

```sh
scripts/art.sh blender --background --factory-startup --python art/clouds/build.py
```

## Gallery

Every model as the world draws it, in its palette colours and light: buildings and plants in
three variants side by side (colours vary per building and plant), the riders from the views
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

### Vegetation

| | | |
|---|---|---|
| ![conifer_broad](../docs/images/models/vegetation/conifer_broad.jpg)<br>`conifer_broad` | ![conifer_tall](../docs/images/models/vegetation/conifer_tall.jpg)<br>`conifer_tall` | ![broadleaf_cluster](../docs/images/models/vegetation/broadleaf_cluster.jpg)<br>`broadleaf_cluster` |
| ![broadleaf_round](../docs/images/models/vegetation/broadleaf_round.jpg)<br>`broadleaf_round` | ![bush](../docs/images/models/vegetation/bush.jpg)<br>`bush` | ![rock_block](../docs/images/models/vegetation/rock_block.jpg)<br>`rock_block` |
| ![rock_pair](../docs/images/models/vegetation/rock_pair.jpg)<br>`rock_pair` | ![palm_coconut](../docs/images/models/vegetation/palm_coconut.jpg)<br>`palm_coconut` | ![palm_fan](../docs/images/models/vegetation/palm_fan.jpg)<br>`palm_fan` |
| ![palm_short](../docs/images/models/vegetation/palm_short.jpg)<br>`palm_short` | ![banana](../docs/images/models/vegetation/banana.jpg)<br>`banana` | ![tropical_bush](../docs/images/models/vegetation/tropical_bush.jpg)<br>`tropical_bush` |

### Clouds

![The four clouds: small, puffy, long and towering](../docs/images/models/clouds/clouds.jpg)

### Riders

| | Female | Male |
|---|---|---|
| side | ![female side](../docs/images/models/riders/female-side.jpg) | ![male side](../docs/images/models/riders/male-side.jpg) |
| side-down | ![female side-down](../docs/images/models/riders/female-side-down.jpg) | ![male side-down](../docs/images/models/riders/male-side-down.jpg) |
| front | ![female front](../docs/images/models/riders/female-front.jpg) | ![male front](../docs/images/models/riders/male-front.jpg) |
| chase | ![female chase](../docs/images/models/riders/female-chase.jpg) | ![male chase](../docs/images/models/riders/male-chase.jpg) |
| face | ![female face](../docs/images/models/riders/female-face.jpg) | ![male face](../docs/images/models/riders/male-face.jpg) |
| head | ![female head](../docs/images/models/riders/female-head.jpg) | ![male head](../docs/images/models/riders/male-head.jpg) |
| lean | ![female lean](../docs/images/models/riders/female-lean.jpg) | ![male lean](../docs/images/models/riders/male-lean.jpg) |
