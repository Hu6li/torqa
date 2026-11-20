# ORQWA World files (`.oqx`), format 1

The specification of world files: complete worlds an author builds outside ORQWA and riders
ride as they are ([ADR 0042](adr/0042-world-files.md), R266–R269). It is written for authors
and for tools that write world files. ORQWA refuses a file that breaks a rule below and says
which rule.

## The file

A world file is a ZIP archive (ZIP64 allowed, one disk) with the extension `.oqx`. Entries are
stored or deflated. Names use `/`, are relative, and contain no `..`, backslashes, drive
letters, control characters or names Windows reserves. There are no directories, links or
duplicate names, and **no entry that `world.json` does not name**.

```
world.json                 required: the description below
preview.png                optional: the library's picture, PNG, at most 8 MiB
routes/<id>.json           one per route
sections/<…>.glb           the world's parts, each in levels of detail
sections/<…>.bin           copies of assets placed in a section
assets/<…>.glb             models placed many times
horizon.glb                optional: the far landscape
sky/<…>.png|jpg|webp       optional: a sky panorama
```

| Limit | |
|---|---:|
| Archive and all entries unpacked | 64 GiB |
| Entries | 200,000 |
| `world.json` | 16 MiB |
| A route | 64 MiB |
| A `.glb` or `.bin` | 1 GiB |
| A sky panorama | 64 MiB |

## Coordinates

The world is laid out in metres: **east** and **north** of the world's `origin`, and
**elevation** above sea level. ORQWA places the origin on Earth with the projection its own
worlds use: an equirectangular projection at the origin's latitude on a sphere of 6,371 km
(`east = Δlon · π/180 · 6,371,000 · cos(lat₀)`, `north = Δlat · π/180 · 6,371,000`).

glTF content uses glTF's own axes, which are Godot's: **+X east, +Y up, −Z north**, one unit a
metre, relative to the centre of the section or the instance it belongs to. A point (x, y, z)
of a section centred on [e, n, h] lies at east `e + x`, north `n − z` and elevation `h + y`.

## `world.json`

```json
{
  "format": 1,
  "id": "6f1c1d84-6a1e-4d55-9a53-0d9e2f1f7a10",
  "name": "Lake Biel",
  "version": "1.0",
  "publisher": "ORQWA",
  "licence": "All rights reserved",
  "attribution": ["Geodata © swisstopo (modified)"],
  "created_unix_s": 1795000000,
  "origin": { "lat": 47.0937, "lon": 7.1563, "located": true },
  "drives_on": "right",
  "environment": {
    "sun": { "elevation_deg": 32, "azimuth_deg": 235, "colour": [1.0, 0.95, 0.86], "energy": 1.5 },
    "ambient": { "colour": [0.62, 0.72, 0.86], "energy": 0.8 },
    "sky": { "kind": "atmosphere", "clouds": 0.3, "overcast": 0.0 },
    "fog": { "colour": [0.74, 0.8, 0.88], "density": 0.00012 }
  },
  "sections": [
    {
      "center": [1200.0, -350.0, 430.0],
      "radius": 400.0,
      "lods": [
        { "file": "sections/0012/lod0.glb", "until_m": 700 },
        { "file": "sections/0012/lod1.glb", "until_m": 3000 },
        { "file": "sections/0012/lod2.glb" }
      ],
      "instances": "sections/0012/instances.bin"
    }
  ],
  "assets": [
    { "name": "beech-a", "lods": [{ "file": "assets/beech-a/lod0.glb", "until_m": 150 }, { "file": "assets/beech-a/lod1.glb" }] }
  ],
  "horizon": "horizon.glb",
  "preview": "preview.png",
  "routes": [
    { "id": "twann-chasseral", "name": "Twann – Chasseral", "file": "routes/twann-chasseral.json" }
  ]
}
```

| Field | |
|---|---|
| `format` | `1`. A file of a newer format is refused with a message that a newer ORQWA is needed. |
| `id` | A UUID that stays the same across versions of the world. |
| `name` | The world's name, 1–1,024 characters without control characters. |
| `version` | Optional: the author's version of the world, shown with its name. |
| `publisher` | Optional: who made or sells the world. |
| `licence` | Required: the world's licence, shown on its page (an SPDX identifier or a sentence). |
| `attribution` | Credits its data and models require, shown wherever the world is used. |
| `created_unix_s` | When the file was made, seconds since 1970. |
| `origin` | Where the world's (0, 0) lies. `located: false` for a place not on Earth: its rides are saved without positions. |
| `drives_on` | `"right"` or `"left"`: the side of the road riders keep to. |
| `environment` | The world's light ([below](#light)). |
| `sections` | At least one. |
| `assets` | The models `instances` files place, by their index in this list. Names are unique, 1–64 of `a–z 0–9 - _`. |
| `horizon` | Optional: one `.glb` in world coordinates around the origin, always drawn behind everything. |
| `preview` | Optional: `preview.png`. |
| `routes` | At least one. Ids are unique, 1–64 of `a–z 0–9 -`. |

Every number is finite. Colours are sRGB, from 0 to 1.

### Sections and levels of detail

A section is a part of the world within `radius` metres of its `center` [east, north,
elevation], horizontally. ORQWA loads the sections nearest the rider first and lets go of those
far behind. Each level of detail is drawn while the camera is less than `until_m` metres from
the section's centre and further than the level before; the last level may leave `until_m`
out and is drawn as far as the rider's view distance reaches. Levels are ordered nearest first.

Sections should be a few hundred metres across: small enough to load in a moment, large enough
not to need thousands of them.

### Assets and `instances` files

An asset is a model placed many times: each of its `.glb` files holds one mesh (of any number
of primitives and materials), placed at the origin. Its levels of detail switch by the distance
from the camera to the section the copies are in. An `instances` file places assets within its
section, in little-endian binary:

```
"OQXI"              4 bytes
version             u32 = 1
groups              u32
then per group:
  asset             u32, index into world.json "assets"
  count             u32
  flags             u32, bit 0: each copy carries a tint
  count × copy      12 f32: the 3 × 4 matrix [r00 r01 r02 x  r10 r11 r12 y  r20 r21 r22 z],
                    row by row, in the section's glTF frame;
                    then, with the tint flag, 4 f32: an sRGB multiplier and alpha,
                    applied to every material of the asset
```

Matrices may rotate and scale but not shear. A file ends after its last group.

## Routes

```json
{
  "points": [
    { "east": 0.0, "north": 0.0, "elevation": 434.2, "surface": "ground", "width": 6.0 },
    { "east": 4.8, "north": 8.7, "elevation": 434.5, "surface": "ground", "width": 6.0 }
  ]
}
```

A route is the line riders ride, and **its points are authoritative**: distance, elevation and
grade for the physics, the trainer's slope, the HUD, the climbs and the course statistics come
from them, as they are, without smoothing. The line runs down the middle of the road. The rider
rides at the line's elevation, a quarter of the road's `width` to the `drives_on` side of it.

- At least two points, and at least 100 m long. Consecutive points are distinct.
- `surface` is `ground`, `bridge` or `tunnel`.
- `width` is optional: the width of the road the rider is on, in metres (default 5).
- ORQWA samples the line every 10 m, with straight lines between the samples, measuring
  distance horizontally. **The road surface must lie on that line**: build the road from the
  route's own points, sampled the same way, or riders float above or sink into it on crests.

## Light

The world's light is the author's. ORQWA's time of day, weather and seasons do not apply.

| Field | |
|---|---|
| `sun` | `elevation_deg` (−10 to 90), `azimuth_deg` (clockwise from north), `colour`, `energy` (0–16). |
| `ambient` | `colour`, `energy` (0–16): light from the sky, before the sky's own. |
| `sky` | Either `{"kind": "atmosphere", "clouds", "overcast"}`: ORQWA's sky, the atmosphere lit by the world's sun, `clouds` its cover from 0 (none) to 1 (closed) and `overcast` how grey it is, from 0 (fair, the default) to 1; or `{"kind": "panorama", "image": "sky/<name>.png", "energy"}`: an equirectangular image the sky and its reflections come from. |
| `fog` | Optional: `colour` and `density` (per metre, 0 to 0.01). |

## glTF content

Every `.glb` is glTF 2.0 in its binary form, self-contained:

- One JSON chunk, then at most one BIN chunk; the header's length is the entry's size.
- At most one buffer, without `uri`, no longer than the BIN chunk. Buffer views and accessors
  lie within it.
- Images come from buffer views, as `image/png`, `image/jpeg` or, with `EXT_texture_webp`,
  `image/webp`. No `uri` anywhere.
- Static scenes: no `animations`, `skins` or `cameras`. Nodes form trees.
- No sparse accessors.
- Extensions used are among `KHR_texture_transform`, `KHR_materials_emissive_strength`,
  `KHR_materials_unlit`, `KHR_mesh_quantization`, `EXT_texture_webp` and
  `ORQWA_terrain_layers`. Only the first five may be required.

Materials are glTF's metallic-roughness PBR, drawn as they are, with Godot's standard
material. Alpha modes work as glTF defines them; use `MASK` for leaves and fences.

### `ORQWA_terrain_layers`

Ground textured from aerial photos is soft at a rider's eye height. This material extension
lays up to eight tiling detail layers over the base colour near the camera:

```json
"extensions": {
  "ORQWA_terrain_layers": {
    "weights": [{ "index": 3 }, { "index": 4 }],
    "layers": [
      { "albedo": { "index": 5 }, "normal": { "index": 6 }, "roughness": 0.92, "size_m": 2.0 },
      { "albedo": { "index": 7 }, "normal": { "index": 8 }, "roughness": 0.75, "size_m": 3.0 }
    ],
    "fade_m": [40, 160],
    "strength": 1.0
  }
}
```

- `weights`: one or two textures over the base colour's texture coordinates; their red, green,
  blue and alpha channels weigh layers 0–3 and 4–7.
- `layers`: 1–8 layers, each an `albedo` texture, an optional `normal` texture, a `roughness`
  (0–1) and the size in metres one repeat of its textures covers. Layers repeat in the
  horizontal plane of the world, and on steep ground also from the side, so the ground needs no
  texture coordinates for them and cliffs do not stretch them.
- Near the camera, the albedo is the base colour times each layer's albedo divided by its
  average colour, blended by the weights and by `strength` (0–1). From `fade_m[0]` to
  `fade_m[1]` metres the layers fade out, leaving the base colour.
- List it in `extensionsUsed`, not `extensionsRequired`: viewers that do not know it show the
  base colour.

## Checking a file

`orqwa-cli world-check <file.oqx>` checks a file as importing it does and shows what it holds,
for tools that write world files. `orqwa-cli world-pack <folder> <file.oqx>` packs a folder
holding `world.json` and the parts it names into a world file and checks it: glTF and images
are stored, routes and `instances` files deflated.

## What ORQWA does with a world file

- Importing checks the whole file against this specification and copies it into `worlds/` in
  the data directory. Each route is a ride in the library.
- Riding reads the file as it is. ORQWA builds nothing, downloads nothing and never changes or
  deletes the file; only the rider deletes it. Textures are drawn uncompressed with mipmaps
  ORQWA makes as it loads them: keep them as small as the view allows.
- Rides on a world's route are saved and compared like rides on courses. Records and ghosts
  belong to the route's line: a new version of the world with the same line keeps them.
- ORQWA offers no export of world files, and their routes cannot be chosen for group rides.
