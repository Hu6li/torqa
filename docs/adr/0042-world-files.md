# ADR 0042 — World files (`.oqx`): authored worlds ridden as they are

- Status: proposed (stage 1 of 3: the format and its loading in ORQWA)
- Date: 2026-11-20
- Issue: #433

## Context

Every world ORQWA shows today is built by its own generator (`orqwa_world`). The rider brings
only inputs: a GPX file, or a course file (`.oqc`, [ADR 0007](0007-course-files.md)) packing
the GPX with the terrain and map tiles it was prepared from. A world a computer keeps
([ADR 0025](0025-kept-worlds.md)) is a cache: it is built again when it is missing, damaged or
of another format.

Some worlds should not be built by ORQWA at all. An author wants to correct what the generator
gets wrong, put real buildings where the map has outlines, model a landmark, or make a whole
place look as it really does, from data and with tools ORQWA has no business shipping. Such a
world is the product of the author's work. Rebuilding it from inputs, or replacing its models
with ORQWA's, destroys that work. These worlds are also to be sold one day, so they travel
as one file the buyer keeps, not as inputs every rider downloads again.

What a ride needs besides the 3D scene is small. Grade, distance, climbs, the HUD, the trainer's
slope and the FIT samples all come from the route's points (`orqwa_routes::Route`): position,
elevation and whether a point lies on the ground, a bridge or in a tunnel. The rider stands at
the route's elevation, not on the road mesh. The rider's position across the road comes from
the road's width and the side traffic keeps to. Everything else ORQWA computes when a course
opens (map, countries, minimap, masks for snow and leaves) belongs to the generator's world.

Options considered:

- **ORQWA's stored world format** (`orqwa_world::stored`, ADR 0025) with custom models added.
  Little new code, but the format is ORQWA's internal one: its meshes carry ORQWA's meanings in
  vertex colours, its models are names of models built into the app, and it changes with every
  `stored::FORMAT`. External tools would have to write it, and authored worlds would break with
  updates.
- **A hybrid**: generated parts in the stored format, authored parts as glTF. Two loaders, and
  the stored format's problems remain for half of the world.
- **glTF 2.0 for all geometry**, with a small ORQWA description for what glTF cannot say (the
  routes, the light, where each part of the world lies). Every modelling tool writes glTF;
  Godot loads it at runtime.

## Decision

An **ORQWA World** is a zip with the extension **`.oqx`**. (`.oqw` is already the shared
workout format of group rides, [ADR 0037](0037-group-ride-activities.md).) Its full
specification is [world-files.md](../world-files.md); in short:

```
world.json                 format, id, name, publisher, licence, attribution, origin, the side
                           traffic keeps to, the light, sections, assets, horizon, routes
routes/<id>.json           each route's points: east, north, elevation, surface, road width
sections/<n>/lod<k>.glb    the world in sections, each in levels of detail (glTF 2.0 binary)
sections/<n>/instances.bin many copies of the world's assets in that section
assets/<name>/lod<k>.glb   models placed many times (trees, vines, posts, …)
horizon.glb                the far landscape, loaded whole
preview.png                the library's picture
```

- **The world is authoritative.** ORQWA rides it as stored: it builds nothing, downloads
  nothing and never regenerates, repairs, rewrites or deletes the file. Only the rider
  deletes it. Updates of ORQWA and cleared caches leave it as it is. What ORQWA derives from it
  for speed (textures compressed for the GPU) lives in the cache and can always be made again.
- **Strictly versioned.** `world.json` carries `format` (1). A newer format is refused with a
  message that a newer ORQWA is needed, never read in part or rebuilt.
- **glTF 2.0 binary, self-contained.** Geometry and materials are standard glTF: metallic-
  roughness PBR, textures embedded as PNG, JPEG or WebP. No external or `data:` URIs. A short
  list of glTF extensions is allowed. Static scenes only: no animations, skins, cameras or lights.
- **Instances in ORQWA's own buffers.** Godot's runtime glTF loader ignores
  `EXT_mesh_gpu_instancing`, so a section lists the copies of each asset in
  `instances.bin` (a 3×4 transform each, optionally a tint), as ORQWA's own worlds already
  hand their trees to Godot's `MultiMesh`.
- **One ORQWA material extension, `ORQWA_terrain_layers`.** Ground textured from aerial
  photos is blurred at a rider's eye height. The extension adds up to eight tiling detail layers
  (albedo, normal, roughness), weighted by masks, that fade in near the camera over the base
  colour. Other glTF viewers show the base colour.
- **Routes.** A world holds one or more named routes, each listed in the library as a ride of
  its own. Its points are authoritative for riding: physics, trainer slope, HUD, climbs and
  statistics come from them as they do from a course's route. The author makes the road
  surface lie on them. ORQWA turns the points into latitude and longitude from the world's
  origin (the projection ORQWA's worlds use), so rides, records and ghosts work as on courses.
  A world not placed on Earth (`located: false`) saves its rides without positions, like a
  video course without GPS.
- **The world's own light.** `world.json` gives the sun, the sky (ORQWA's atmosphere lit by
  that sun, with the author's clouds, or the author's panorama), ambient light and fog. ORQWA's time of day,
  weather and seasons do not apply, and none of its live effects reach an authored world:
  no snow, wet roads, puddles, wind or autumn leaves. The rider's graphics quality still sets
  view distance, levels of detail, shadows and screen effects.
- **Sections stream like ORQWA's chunks.** A section has a centre and a radius. ORQWA loads the
  sections near the rider first, in the order and frame budget its own chunks use, and switches
  a section's levels of detail by distance.
- **Kept apart from courses.** World files live in `worlds/` in the data directory, beside
  `courses/`. Importing checks the whole file before copying it in. The library lists one
  entry per route; deleting one deletes the world with all its routes, after naming them.
- **Not shared from ORQWA.** Worlds are to be sold. ORQWA offers no export, and world routes
  cannot be chosen for group rides. A file is a file, so ORQWA cannot stop copies: no DRM,
  which a GPL program could not enforce anyway. `publisher` and `licence` say whose a world
  is. Signed packages that prove where a world comes from could come with a later format.
- **Assets of worlds are not ORQWA's assets.** R47 (assets redistributable with the GPL
  program) covers what ORQWA ships. A world file carries its own licence and attribution, which
  the course page shows.

## Stages

1. **This decision**: the format, its validation, and riding world files in ORQWA, with a small
   test world made by a script.
2. **A realistic world** of Twann – Chasseral around Lake Biel, built from open Swiss geodata
   (swisstopo) and CC0 or own models, in a separate, private repository: it is to be sold.
3. **An editor** to correct and design worlds (#433), and converting `.oqc` courses into world
   files as a starting point.

## Consequences

**Amendment (2026-11-22, R270).** World routes have a distinct **World** library type.
Each route may carry its countries and translated names, using the course metadata shape,
so cards, country search, filters and groups work offline without modifying the world file.
Omitted countries stay unknown; worlds not placed on Earth cannot supply them.

- Authors work in any tool that writes glTF; ORQWA needs no exporter for stage 1.
- ORQWA gains a runtime glTF loader. It keeps the author's materials, unlike the loaders of
  ORQWA's own models, which replace them with ORQWA's shaders.
- Textures loaded at runtime are not compressed for the GPU: Godot's compressors exist in its
  editor only (`Image.compress` is unavailable in exported programs). PNG, JPEG and WebP
  textures take their full size in video memory, with mipmaps ORQWA makes on loading, so
  authors keep them as small as the view allows. GPU-compressed textures (KTX2 with Basis
  Universal, `KHR_texture_basisu`, which Godot transcodes to the GPU's own format) may follow
  once they are tested.
- World files are large: a realistic 20 km world is expected at several gigabytes. They use
  ZIP64 and higher limits than course files; [world-files.md](../world-files.md) sets them.
- Authored worlds look the same in every season and weather. Opt-in material tags that let
  ORQWA's effects reach chosen parts may follow.
- A rider cannot ride a world route in a group ride until sharing of bought worlds is decided.
