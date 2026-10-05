# ADR 0011 — Stylized, faceted look instead of realism

- Status: accepted
- Date: 2026-10-05
- Supersedes: the realism target of R44–R47 and the MPFB2 part of [ADR 0009](0009-asset-pipeline.md)

## Context

Torqa's world aimed at MyWhoosh-level realism (R44): photo textures, PBR ground, realistic
trees, a MakeHuman/MPFB2 rider. Realism in an open, zero-budget project needs a lot of assets of
high quality, and every gap shows; the user's verdict after the first realistic buildings and
textures was to change direction. They chose two references:

1. a **low-poly diorama** (a lighthouse on an island): chunky faceted shapes, flat colours in a
   soft pastel palette, warm soft light, soft shadows and ambient occlusion;
2. a **stylized character**: exaggerated proportions, saturated pastel clothes.

Decisions (user, 2026-10-05):

1. **Replace** realism completely; no realistic option is kept (two looks would double every
   art task).
2. **Faceted everywhere**: world and riders alike, one normal per face.
3. **Riders** both ways: scripted from scratch in Blender *and* built from CC0 stylized bases —
   a female and a male rider.

## Decision

- **Look.** Stylized, faceted low-poly in a soft pastel palette, bright and low in contrast:
  colour instead of texture, chunky silhouettes, warm sun with tinted shadows, ambient
  occlusion, a pastel gradient sky and haze. No outlines, no cel bands, nothing glossy or
  metallic. The working guide, with the palette taken from the references, is the
  `torqa-look` skill (`.claude/skills/torqa-look/SKILL.md`).
- **One palette** for the whole world, kept in one file that both the Rust core (vertex and
  instance colours) and the shaders read, so colours change in one place.
- **Retired:** the ambientCG photo textures, normal maps, triplanar rock, textured asphalt and
  per-pixel patterns (tile rows, wood grain, window grids). Models keep material *names*; the
  app gives them flat palette colours.
- **Pipeline unchanged otherwise** (ADR 0009): Blender scripts are the source, `.glb` files are
  committed, Blender runs headless in the art container. MPFB2 is dropped: its humans are
  realistic. Riders come from Torqa's own scripts and from CC0 stylized bases kept untouched
  under `art/sources/`, with the scripts deriving the faceted riders from them.
- **Performance.** Flat shading and few, simple shaders free GPU time. Quality presets keep
  their names; they mostly vary shadows, ambient occlusion, view distance and the amount of
  vegetation. Global illumination is no longer a goal.
- **Review.** Every visual step is checked in renders (`torqa-render-review`) against the
  references, starting with a small look-dev scene.

### Settled with previews (2026-10-05)

Mock-ups of the look (chase and drone views, the kit, riders, facet sizes, weather) were reviewed
with the user:

- **Riders** keep natural ("heroic") proportions: long legs, a head of normal size. The toy-like
  variant with a large head and short limbs was dropped.
- **Facet size**: the ground keeps the terrain grid it has (finer near the road, coarser further
  out); no extra-large facets, so the real terrain still reads.
- **Weather and time of day** stay pastel: a lilac-to-peach evening with plum shadows, rain as soft
  grey-blue light and closer haze.
- The palette is `app/assets/palette.json`, read by `core/torqa-world` (`palette.rs`) and by the app
  (`app/scenes/palette.gd`).
- **Riders' bodies** come from Blender Studio's CC0 *Human Base Meshes* (the stylized
  "primitive" bodies, faceted at their base level, arms lengthened and heads made smaller for
  natural proportions); kit, helmet, sunglasses and hair are scripted on top, so "both ways"
  meets in one rider. The **bike** is scripted in Blender and sized to each rider. The rider
  is chosen per profile (R46).

## Consequences

- Most of the realistic work of Phase 9 is restyled rather than extended: ground, roads,
  vegetation, buildings, sky and rider. Logic stays: route snapping, the shaped road, building
  kinds and placement, streets, quality presets.
- The asset count and size drop (no texture sets); `docs/CREDITS.md` loses the texture credits
  once they are removed.
- The look is easier to keep consistent and cheaper to render, which helps the 60 fps target on
  an M1 (R43).
- A believable place still matters: the world keeps the real terrain, roads and buildings of
  the map; only their rendering is stylized.
