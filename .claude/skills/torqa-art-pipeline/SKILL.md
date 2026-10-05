---
name: torqa-art-pipeline
description: How Torqa makes 3D models — scripted Blender (5.2 LTS, headless, in the x86-64 art container), exported as .glb into app/assets/models, with a manifest the Rust world reads and material names the Godot side styles (ADR 0009). Use when creating or changing anything under art/, app/assets/models/, the model loaders (app/scenes/*_models.gd) or the model shaders, and when bringing in third-party (CC0) bases such as rider bodies.
---

# Torqa's art pipeline

Models are **Blender Python scripts** (the source of truth) plus the **exported `.glb` files**
(committed, so the app builds without Blender). Look and colours follow the `torqa-look` skill.
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

The repository is mounted at `/workspaces/torqa`. Write scratch files to `screenshots/`
(gitignored), never elsewhere in the tree.

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
  app gives it its look (`app/scenes/building_models.gd` + `app/shaders/building_model.gdshader`
  for buildings). A new name needs an entry there.
- **Per-instance variation** comes from MultiMesh data: instance colour (e.g. walls), custom
  data (e.g. roof colour, variant 0–1). Keep it within the palette.
- **`models.json`** fields are read by `core/torqa-world` (e.g. `buildings/models.rs`); a test
  there checks every listed model has its `.glb`.

## Making models faceted and lean

- Flat shading: leave `use_smooth` off; the exporter then writes one normal per face. Avoid
  modifiers that smooth (SubSurf) unless applied and then flattened.
- Stay within the triangle budgets of `torqa-look`; towns place thousands of buildings and
  forests thousands of trees. Print face counts when building (build.py does).
- Exports are deterministic: rebuilding unchanged scripts must give byte-identical `.glb`s.
  If a rebuild changes files you did not touch, find out why before committing.

## Riders (`art/riders`)

- No armature: the bodies are rigid parts (Human Base Meshes' primitive bodies), so the build
  poses them by turning parts about their joints and exports the posed upper body as one node
  (`body`) and each leg part as its own node with its origin at its joint. Godot aims the leg
  nodes at the pedals by two-bone IK (`app/scenes/rider_avatar.gd`); `riders.json` carries the
  fit. Keep node names stable — the app finds them by name.
- Faceted characters: base-level bodies (no SubSurf) with flat shading, kit by material name
  coloured from the rider's palette section.
- Male and female riders share node and material names; each has their own bike, sized to them.

## Third-party bases (CC0 characters and the like)

- Allowed: own work, CC0, CC-BY, CC-BY-SA (ADR 0009). Not: NC, ND, engine-locked, Mixamo.
- Check the licence at the source (the page, not a re-upload) and record it before using.
- Keep the untouched original under `art/sources/<name>/` with `LICENSE` or `SOURCE.md` (URL,
  author, licence, date); the script derives the Torqa model from it, so changes stay
  reviewable and repeatable.
- List every third-party asset in `docs/CREDITS.md`.

## Before committing

1. Build the group and check the printed face counts.
2. Render for review: `scripts/dev.sh scripts/render-models.sh` (`MODELS="a b"` for some) —
   see `torqa-render-review`; look at the images, compare with `torqa-look`.
3. Commit the scripts, the `.glb`, their `.glb.import` files and `models.json`;
   `scripts/check.sh` must pass (it runs the Rust test that every model exists).
4. Put a few review images in the PR (JPEG under `docs/images/`, see `torqa-render-review`).
