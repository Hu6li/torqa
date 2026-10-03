# ADR 0009 — 3D asset pipeline and asset licensing

- Status: accepted
- Date: 2026-10-03

## Context

The 3D world should reach MyWhoosh-like realism for terrain and rider (R44–R46). That needs real
models (rider, bike, trees, rocks) and PBR textures. The project is GPL-3.0 with zero budget
(R2), so every asset must be free and redistributable in a public repository (R47). Realistic
humans are hard to model from scratch; mechanical parts like a bike are easy to describe in code.

## Decision

- **Scripted models.** Models are produced by **Blender Python scripts** run headless in the
  devcontainer (Blender and the MPFB2 add-on are container packages). The scripts are the source
  of truth; they export glTF (`.glb`) for Godot.
- **Both are committed**: the scripts and the exported `.glb` files, so the app builds without
  Blender and changes to models are reviewable. Each asset is documented (what it is, how to
  regenerate it, its sources and license).
- **Riders** (female, male) start from **MakeHuman** base meshes via **MPFB2** (CC0 output,
  rigged); kit materials and the cadence-driven pedaling animation (pedal IK) are scripted.
- **Bike**: a parametric model built by script, so other frames and wheels can follow (R46).
- **Vegetation, rocks**: generated (geometry nodes / Sapling) or taken from free asset libraries.
- **Textures**: CC0 PBR sets (Poly Haven, ambientCG).
- **Allowed licenses**: own work, CC0, CC-BY, CC-BY-SA (one-way compatible with GPLv3). Every
  third-party asset is listed with author, source and license in a credits file.
- **Not allowed**: NC or ND licenses, engine-locked assets (Quixel Megascans / Fab — Unreal-only),
  Mixamo (raw files may not be redistributed).

## Consequences

- Anyone can regenerate or change the models with free tools; the repository stays fully open.
- The container image grows by Blender (~hundreds of MB).
- Art direction needs human review of rendered previews; scripts make iterations cheap.
- Committed `.glb` files grow the repository; large binaries may move to Git LFS if needed.
