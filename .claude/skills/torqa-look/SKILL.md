---
name: torqa-look
description: Torqa's art direction — stylized, faceted low-poly in a soft pastel palette, for the world and the riders alike (replaces the earlier photorealism target). Use for any visual work on the 3D world or riders — shaders in app/shaders, colours and geometry from core/torqa-world, Blender models in art/, lighting, sky, weather, quality presets — and whenever judging a screenshot or render.
---

# Torqa's look

Torqa's 3D world and riders are **stylized, faceted low-poly** with a **soft pastel palette**:
a toy-like diorama, not a photograph. The decision (user, 2026-10-05): replace realism
completely (no realistic option kept), **faceted everywhere** (world *and* riders), riders made
both from scratch in Blender and from CC0 stylized bases (male and female). The ADR recording it
is `docs/adr/0011-stylized-look.md` once written; it wins over this file if they disagree.

## The references

Two pictures the user chose (not in the repo; copyrighted):

1. **World** — a lighthouse island diorama: chunky faceted shapes (conifers as stacked faceted
   cones, a rock as a few big facets), flat colour per facet, no textures, mint sky, warm sand,
   coral roof, cream tower, sage and olive greens, teal shadows in the water. Soft warm key light
   from the upper left, soft long shadows, generous ambient occlusion in creases, everything
   bright and low in contrast.
2. **Rider** — a stylized character: exaggerated proportions (larger head and hands, long
   legs), saturated pastel clothes (pink, yellow, blue), plum darks, soft rim light. Torqa keeps
   the proportions and colours but makes the forms **faceted** like the world.

## Rules

- **Facets, not smoothness.** One normal per face. No smooth shading, no normal maps, no
  subdivision look. Fewer, bigger facets read better than many small ones.
- **Colour, not texture.** No photo textures, no grime, weathering, noise or fine patterns
  (roof tile rows, wood grain, window grids drawn per pixel). Variation comes from picking
  palette colours per object or instance and from light, at most a very soft large-scale tint
  gradient (e.g. darker at the foot of a tree).
- **Simple, chunky silhouettes.** Exaggerate what identifies a thing (a chalet's big roof and
  balcony, a church tower, a conifer's spikiness); leave out what does not show at riding
  distance (gutters, glazing bars, flower pots smaller than a hand).
- **Bright and soft.** High values, low contrast, no pure black or white; shadows tinted (teal
  or plum), never grey. Warm sun, cool sky fill.
- **No outlines, no cel bands.** Lighting stays smooth across a facet; facets do the stylizing.
- **Not metallic, not glossy.** Roughness high everywhere; water and windows may get a soft
  sheen, nothing mirror-like.

## Palette (sRGB, from the references)

World:

| Role | Colours |
|---|---|
| Sky, haze | mint `#b5dbce`, horizon `#d6ece4`* |
| Water | teal `#628388` deep, mint `#9fcfc4`* shallow, foam `#f2ebd9` |
| Ground | meadow sage `#b2c178`, light olive `#cbcd7e`, olive `#87895e`, forest `#7b7f44`, field sand `#efcaa8`, rock `#c3bbb4` |
| Road | blue-grey `#8e959b`*, markings `#f2ebd9` |
| Walls | cream `#f2ebd9`, sand `#efcaa8`, peach `#e8a788`, dusty rose `#bf9282` |
| Roofs, wood | coral `#e2705e`, terracotta `#a57369`, brick `#6b3231`, timber `#7a5755` |

\* proposed, not in the references; settle them in the look-dev scene.

Riders:

| Role | Colours |
|---|---|
| Kit | pink `#efb5cc`, magenta `#ce5aa5`, yellow `#f4c16e`, blue `#5b97c8`, deep blue `#3959a0`, navy `#2e3e7a` |
| Darks | plum `#614367`, aubergine `#4b2e44` |
| Skin | `#bd8877`, `#a4665a`, `#834a40` and lighter tones |

The palette lives in code in one place (planned: a shared palette file read by
`core/torqa-world` for vertex colours and by the shaders); do not scatter hex values. To take a
palette from a new reference image, run `scripts/palette.py` (k-means in Blender's Python, see
the file).

## How to get it

- **Faceted shading in Godot:** for meshes built in Rust, give every triangle its own vertices
  and its face normal; or in a spatial shader derive the normal per pixel:
  `NORMAL = normalize(cross(dFdy(VERTEX), dFdx(VERTEX)));` (view space; check the sign against a
  lit test face). Imported models: export flat (split normals per face), see `torqa-art-pipeline`.
- **Colour:** vertex colours or a per-instance colour (MultiMesh `INSTANCE_COLOR` /
  `INSTANCE_CUSTOM`) picked from the palette, sRGB → linear in the shader.
- **Light:** one warm `DirectionalLight3D` (≈ `#fff1dc`, proposed), soft shadows; sky as a smooth
  vertical gradient (mint to pale horizon); fog/haze tinted with the horizon colour; SSAO on from
  Medium up (creases make the diorama look); tonemapper AgX or Filmic with lifted blacks; glow
  subtle. Global illumination matters little for this look; prefer AO and shadows.
- **Budgets (triangles):** tree 40–300, bush 20–80, rock 20–100, house 300–1200, church
  800–2000, rider about 3–6k, bike about 1–2k. Fewer is better if the silhouette survives.

## Reviewing a render

Check, in order: silhouettes readable from the road; colours from the palette and harmonious
side by side; facets visible but not noisy; no texture, grime or fine pattern left; light warm,
shadows tinted, AO in creases; distant terrain fading into mint haze. Compare against the two
references described above. Rendering: see the `torqa-render-review` skill.
