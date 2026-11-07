---
name: orqwa-art-pipeline
description: How ORQWA makes 3D models and textures — scripted Blender (5.2 LTS, headless, in the x86-64 art container), models exported as .glb into app/assets/models with a manifest the Rust world reads and material names the Godot side styles, tileable PBR textures baked from procedural materials, quick Cycles previews (ADR 0009, ADR 0020). Use when creating or changing anything under art/, app/assets/models/, app/assets/textures/, the model loaders (app/scenes/*_models.gd) or the model shaders, and when bringing in third-party (CC0) bases such as rider bodies.
---

# ORQWA's art pipeline

Models are **Blender Python scripts** (the source of truth) plus the **exported `.glb` files**
(committed, so the app builds without Blender). Look and colours follow the `orqwa-look` skill.
For bpy itself, the imported skills `blender-python-scripting`, `blender-modeling-modifiers`,
`blender-animation-rigging`, `blender-scene-rendering` and `blender-geometry-nodes` are the
reference — but ignore their "MCP-first" advice: here Blender always runs **headless**, no live
session, no MCP server.

## Running Blender

Never install Blender on the host. It runs in its own container (official Linux builds are
x86-64 only; Docker emulates it on Apple Silicon — fine for scripted work, a build of all
building models takes seconds):

```sh
scripts/art.sh blender --background --factory-startup --python art/<group>/build.py
scripts/art.sh blender --background --factory-startup --python art/<group>/build.py -- <name>…
```

The repository is mounted at `/workspaces/orqwa`. Write scratch files to `screenshots/`
(gitignored), never elsewhere in the tree.

## Quick previews (`art/preview.py`)

`build.py` of buildings, vegetation and clouds take `--preview DIR`: instead of exporting, they
render each model with Cycles from four views (`quarter`, `side`, `back`, `top`) into
`DIR/<name>_<view>.png`, on a meadow under a warm sun (a few seconds per view). Materials show
the script's preview colours or textures under Cycles, not the app's shaders, so new materials
can be tried before the app has them; check in the world after (`orqwa-render-review`).

```sh
scripts/art.sh blender --background --factory-startup --python art/buildings/build.py     -- --preview screenshots/preview chalet_2_m
```

A new group's build script calls `preview.options()` for the names and directory and
`preview.render(obj, directory, name)` instead of exporting (`ground=False` for things in the
air).

## Textures (`art/textures/build.py`)

Tileable PBR textures baked from procedural node trees: the plane's UV is wrapped onto a torus
in 4D and every noise and Voronoi node samples that point, so nothing has an edge. Each recipe
(`rock`, `grass`, `dirt`, `sand`, `bark`, `asphalt`, …) is a function building its tree with the
`Tree` helper (`noise`, `voronoi`, `ramp`, `math`, `mix`, `output(colour, roughness,
height)`); the bake writes `<name>_albedo.webp` (sRGB), `<name>_normal.webp` (OpenGL tangent
space, as Godot expects) and `<name>_rough.webp` into `app/assets/textures/`: colour and
roughness lossy (quality 90), normal maps lossless, written through the render output with a
view transform that keeps the baked values exactly (see the script).

```sh
scripts/art.sh blender --background --factory-startup --python art/textures/build.py     -- [name …] [--size 1024] [--out screenshots/textures]
```

- Iterate with `--out screenshots/textures --size 512`, and look at a 2 × 2 tiling of the
  albedo to check for seams and repeats.
- `noise(scale, …)`: scale counts whole periods per tile; `streak=True` stretches features
  along v (fibres, blades); `offset` decorrelates two noises at the same scale.
- Commit the textures only together with the shader or model that uses them, with their `.import`
  files set to `compress/mode=2` (VRAM), `mipmaps/generate=true`, `detect_3d/compress_to=0`, and
  `compress/normal_map=1` for normal maps (Godot writes other defaults on first import: fix
  them, then re-import).
- Keep recipes neutral in hue where the app tints per instance (walls, roofs, leaves).
- Spring's `foliage.py --blossoms-only` draws `blossoms_albedo.png`: transparent five-petal
  twig clusters on the existing leaf-card UVs. Keep this small mask lossless PNG (the
  emulated WebP encoder introduced line artifacts), with VRAM compression and mipmaps on
  import. Review petal silhouettes close up and the selected crowns on Low too.
- Where a recipe does not look real, a CC0 scan may replace it (ADR 0020): keep the original
  under `art/sources/<name>/` with `SOURCE.md`, convert it to the same names and formats in a
  script, and credit it in `docs/CREDITS.md`.
- `build.py -- rock` converts the archived CC0 Poly Haven **Rock Boulder Dry** scan using
  `rock_scan.py`, preserving sRGB albedo and lossless OpenGL normals. Originals and credits
  are under `art/sources/rock-boulder-dry/`. It replaced procedural noise after comparison
  on identical boulders in the same light. `rock.gdshaderinc` adds broad fractures at an 8 m
  scale for cliffs and 0.65 m for boulders. Review close and overview distances (R115).
- `build.py -- canopy` bakes the canopy distant forest ground turns into (R146): round crowns
  from Voronoi domes, rims and crevices in shade, tiled every 32 m. A 2 × 2 tile only shows
  seams; judge the look in the world, from the drone above forested hills.
- `build.py -- bridge_concrete` converts CC0 Poly Haven **Concrete Wall 007** using
  `concrete_scan.py`. Originals, licence and checksums live in `art/sources/concrete-wall-007/`.
  Keep its 2.16 m physical scale and dedicated albedo/normal/roughness maps: the urban
  concrete recipe's paving joints made bridge walls look tiled. Review pores and weathering
  below the deck and from the Oberalp close/side views, as well as the original chase (R122).

## bpy pitfalls

- Import `bpy` before `bmesh`.
- `bpy.ops.object.transform_apply` applies the location too by default; to rotate geometry of
  an object away from the origin, use `mesh.transform(matrix)` instead.
- Meshes the world reads bare (instanced by MultiMesh) lose their object's location, rotation
  and scale: bake transforms into the vertices before export.
- Never leave a file named like a standard Python module (`inspect.py`, `json.py`) beside a
  script Blender runs: it shadows the real one and Blender aborts with obscure errors.
- From Blender 5.0 new materials always have a node tree; the viewport colour
  (`diffuse_color`) and the Principled BSDF's Base Color are separate: set both, or let
  `preview.render` copy the first into the second.

## Layout of a model group (see art/buildings)

- `catalogue.json` — one entry per model: name, kind and parameters (sizes, storeys, pitch…).
- `kinds.py` — one builder per kind; returns the mesh and a dict describing its footprint.
- `kit.py` — reusable parts and the `Mesh` accumulator: faces with **material names**, texture
  coordinates in metres by default, counter-clockwise from the front (glTF convention).
- `build.py` — builds the catalogue (or the named models), exports `app/assets/models/<group>/
  <name>.glb` and writes `models.json` beside them (kind, dimensions, heights…) for the Rust
  world, which embeds it (`include_str!`) to choose and fit models.

Coordinates: metres, x along the model, y across, z up, origin at the footprint centre on the
ground; walls reach 3 m below ground for slopes. glTF/Godot turn Blender's y into −z.

## Contracts — change both sides together

- **Material names → looks.** Files carry no textures; every face has a material *name* and the
  app gives it its look — a texture from `app/assets/textures` and a palette tint
  (`app/scenes/building_models.gd` + `app/shaders/building_model.gdshader` for buildings,
  `app/scenes/vegetation_models.gd` + `vegetation.gdshader` / `foliage.gdshader` for plants).
  A new name needs an entry there.
- **Per-instance variation** comes from MultiMesh data: instance colour (e.g. walls), custom
  data (e.g. roof colour, variant 0–1). Keep it within the palette.
- **`models.json`** fields are read by `core/orqwa-world` (e.g. `buildings/models.rs`); a test
  there checks every listed model has its `.glb`.
- **Kept worlds** (R101) name models and fill instance buffers as the build that kept them did,
  and the app draws them with today's `.glb`: a model that only looks different needs nothing.
  Renaming or removing a model, or changing what an instance buffer holds, raises
  `orqwa_world::stored::FORMAT`; a `models.json` change that places models differently raises
  `orqwa_world::GENERATOR`, so courses offer to regenerate their worlds.

## Shading and budgets

Urban materials (R105, ADR 0027): `concrete` and `paving` in `art/textures/build.py` bake
1024 px albedo, normal and roughness maps. Concrete tiles over 3 m, paving over 2 m.
`app/shaders/urban.gdshader` shares them across Rust-built sidewalks, curbs, access and
front-garden walls; dense terrain also uses paving. Use VRAM compression and mipmaps on
all maps, normal-map import on normals, and keep these six textures with their settings.

- Realistic (ADR 0020, `orqwa-look`): smooth shading with sharp edges where the object
  has them (`use_smooth` on, shade auto smooth by angle or marked sharp edges); the exporter
  then splits normals only there. Vegetation writes its own normals per corner (smooth within a
  trunk or rock, pointing out of the crown on foliage cards); buildings are boxes and keep one
  normal per face.
- UVs in metres on every surface, so tileable textures keep their real scale.
- Stay within the triangle and texture budgets of `orqwa-look`; towns place thousands of
  buildings and forests thousands of trees. Print face counts when building (build.py does).
- Exports are deterministic: rebuilding unchanged scripts must give byte-identical `.glb`s.
  If a rebuild changes files you did not touch, find out why before committing.

## Regional buildings

Asian buildings (`art/buildings/japanese.py`, R164) use the existing named materials and
`japanese_house` / `japanese_castle` catalogue kinds. Keep the palette and gallery's regional
wall/roof colours aligned with Rust placement. Castle model tiers, first eave height and
footprint shrink must agree with `japanese_castle` shells in `buildings/mod.rs`; review the
nearby and distant silhouettes together. Nordic buildings (`art/buildings/nordic.py`) use
the `nordic_wood` instance-tinted plank material and their own country outlines, palette and
model kinds. Keep the Nordic castle's distant generic keep visually compatible. Asian and
Nordic outlines are independent of climate. French models (`art/buildings/french.py`) use
the French palette and a simplified metropolitan-France/Corsica outline; keep their pale
village houses, limestone city façades with zinc mansards, and slate-roofed château silhouettes
aligned with the French region selector. See
`docs/regional-buildings.md` before extending the regional sets.

## Tree LODs

`art/vegetation/lods.py` exports medium/far forms of every tree to
`app/assets/models/vegetation/lods/`, outside the world-placement manifest. Preserve crown
volumes, ground origins and material names; retain fan palms' fans and banana leaves.
Godot's `TreeLodMesh` combines these with the unchanged close geometry. Changing rendering
LODs needs no world regeneration. Keep their `.import` files and compare native GPU views
and frame rates; disconnected leaf cards need authored simplification beyond automatic LOD.
Conifers share `conifer_leader` between nearby and distant geometry: sparse upper whorls
meet a short terminal shoot whose needle planes taper to one apex on the trunk axis.
Retain that upper branching in LODs; long steep cards turn it into a spike. Keep the shoot
slender: a large dense spray above sparse branches reads as a second crown. Separate
splayed upright sprays create competing treetops.

## Woodland ferns

Woodland ferns (R104) are generated in `art/vegetation/build.py` as paired geometric
leaflets on arching fronds, using the existing `frond` material and `plants.ferns` palette.
Keep each below about 1,500 triangles, preserve both spreading and upright forms, and
refresh their model gallery and a woodland close-up after shape or placement changes.

## Props (`art/props`)

- What stands by the road (R92). Each model's +x points away from the road, its length along
  Blender +y (Godot −z, the way the road is ridden on the right), the origin on the ground;
  parts reach 0.4 m below it so they meet sloping ground. Rails are 4 m spans the world shears
  to the road's grade (posts stay upright), so a span's ends must line up with its
  neighbours'. Run ends are their own models (`guard_rail_start` / `guard_rail_end`): turning a
  model round flips which side faces the road.
- The rules live in `core/orqwa-world/src/props.rs`; each decision depends only on the road
  and the ground near its own point (stations at whole multiples of a spacing along the
  route), so a prop is the same whichever chunk places it. New props get a rule there, a
  test in `core/orqwa-world/src/tests.rs`, palette colours under `props` and a row in
  `PLACED` (the model check).

## Riders (`art/riders`)

- Realistic bodies (Human Base Meshes) skinned to a skeleton built from the bundle's joints.
  Heat weighting fails on the detailed mesh: weight a thinned copy and hand the weights on
  (Data Transfer, nearest face interpolated). Pose by aiming pose bones at target points
  (`aim`, `turn_bone` about a pivot), never by raw Euler angles: bone roll makes their signs
  unpredictable. Turn the pelvis about the hip joints, or the fit's hips move.
- The riding pose is baked as the rest (apply the armature modifier, `pose.armature_apply`,
  skin again); the app only turns the leg bones (two-bone IK, shortest arc from rest) and moves
  the pelvis bone. Every leg bone needs a child (the foot a `toe`) so the app can read where it
  ends. Keep bone and node names stable — the app finds them by name.
- Hands: finger bones from the bundle's finger joints, curled joint by joint about one axis
  per finger (`grip`; an axis per joint flips past 90° and the tip bends back out); the thumb
  aimed explicitly (`THUMB_AIM`). Weight the hands geometrically (`hand_weights`): heat
  weights on the thinned copy leave the fingers to the wrong bones, and a hard nearest-bone
  split tears the knuckles. The bundle's one skeleton has its hands and legs beside a broader
  body's (`centre_hand_joints`, `centre_leg_joints`), and the skin still sits off its bones
  posed: place the hand by where its skin lands (`hand_across`), not by the bones. Spare the
  hands when thinning, or the fingers flatten into strips; bending finger vertices without
  bones tears the mesh. Check hands with close-up Workbench renders from the inside, front
  and top before the Godot views.
- Kit edges: thin first, then cut planes across the limbs (`bmesh.ops.bisect_plane` on the
  faces near each limb) and choose materials by position; decimating after dressing makes
  the edges ragged. Unlinked appended objects have no computed world matrix: read `location`.
- Fit to a real rider on the hoods (a race photo, 1.76 m): sit bones 9 cm below the hip joints
  (the standing body's lowest point between the hips is the inner thighs, far lower), the back
  bending mostly at the pelvis. Choose road-frame dimensions first (`bike_fit.py`, published
  Aethos 49/54 proportions); pose the torso and arms to its fixed hoods. Solve the arm's reach
  to the wrist. Check the whole silhouette against a bike photo, seated and standing.
- The bike's shaped parts are `Faces.sweep` sections along `path` curves; chainrings and
  rotors are rings (`annulus`) — solid discs read as pale plates.
- Painted tube intersections are welded at 3 mm, smoothed and reduced to 1,800 triangles
  before joining the untouched hardware. Keep the frame's holes and fork clearance open.
  `sidewall`, `carbon`, `alloy` and `bottle` are styled by `RiderAvatar` from the `bike`
  palette; painted composite gets clear coat, only bare/anodized metal gets metallic shading.
  A complete bike is about 18k triangles. `VIEWS="bike-side bike-drive bike-front bike-cockpit"`
  in the rider runner shows the bike without a body under the app's sky and tone mapping.
- Male and female riders share node and material names; each has their own bike, sized to them.
- Keep stack, reach, head-tube length, fork offset and stem dimensions together. A round
  headset, modest rising stem, compact drop bar and upright shifter horns must read as one
  plausible steering assembly. Join the fork after welding the frame to retain its lower
  bearing joint. `bike-clearance` and `bike-saddle` are dedicated review views. Both saddles
  have 21 cm shells and matching rails; regenerate fit metadata when contact points change.
- The road-racing helmet (R225–R226, `art/riders/helmet.py`) is a signed distance field: a
  squarish ovoid fitted round the scalp, vents drawn as polygons on a chart round a long axis
  tilted from the fit dial down to below the brow (the rear exhausts on a plane behind the
  head), smooth booleans for rounded edges and recessed braces. Blender's bundled OpenVDB
  meshes it, a symmetric Decimate thins it, and the triangles are split along the skin
  contour into `helmet` and `vents`. Vertices and faces are put in a canonical order after
  meshing and thinning, so exports stay byte-identical. OpenVDB winds faces outwards; Cycles
  shades inverted faces without complaint, Godot culls them, so judge in Godot views. Keep the
  material names, head-bone attachment and strap anchors (`helmet.rim_height`). Compare
  front, side, top and rear with the Z1 product images linked in `art/README.md` (a scratch
  Cycles render of the helmet alone from the same angles iterates in a minute), then the
  `helmet-side`, `helmet-rear`, face and chase views on both riders. Keep it white and
  unbranded.

## Not a model: the remote's clip

`art/remote/keep_awake.py` renders the silent H.264 + AAC clip the phone remote's page plays to
keep the screen on (R88) into `core/orqwa-remote/src/keep-awake.mp4`, from the sequencer (no
GPU), with Blender's FFmpeg, as the dev container's cannot encode H.264. Keep its sound track
(Safari and Firefox need it), a length of at least 2 s (the page rewinds it a second before its
end instead of looping) and its stamps off (a render date in the metadata makes rebuilds
differ); `cargo test -p orqwa-remote` checks the tracks are there.

## Third-party bases (CC0 characters and the like)

- Allowed: own work, CC0, CC-BY, CC-BY-SA (ADR 0009). Not: NC, ND, engine-locked, Mixamo.
- Check the licence at the source (the page, not a re-upload) and record it before using.
- Keep the untouched original under `art/sources/<name>/` with `LICENSE` or `SOURCE.md` (URL,
  author, licence, date); the script derives the ORQWA model from it, so changes stay
  reviewable and repeatable.
- List every third-party asset in `docs/CREDITS.md`.

## Before committing

1. Build the group and check the printed face counts; iterate with `--preview DIR`.
2. Render for review in the world: `scripts/dev.sh scripts/render-models.sh` (`MODELS="a b"` or
   `GROUPS="vegetation"` for some; `scripts/render-riders.sh` for riders) — see
   `orqwa-render-review`; look at the images, compare with `orqwa-look`.
3. Refresh the gallery in `art/README.md`: the same scripts with `GALLERY=docs/images/models`
   (`scripts/dev.sh sh -c 'GALLERY=docs/images/models scripts/render-models.sh'`), and add or
   remove table rows for models added or taken out; update the group's section there too.
4. Commit the scripts, the `.glb`, their `.glb.import` files, `models.json` and the gallery;
   `scripts/check.sh` must pass (it runs the Rust test that every model exists).
5. Put a few review images in the PR (JPEG under `docs/images/`, linked by commit, see
   `orqwa-render-review`).
