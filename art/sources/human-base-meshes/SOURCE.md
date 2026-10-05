# Human Base Meshes — stylized primitive bodies

- **What:** the stylized "primitive" female and male bodies (100 parts: pelvis, torso, limbs,
  hands, feet, head and face parts, each with its origin at its joint), unchanged.
- **From:** *Human Base Meshes* asset bundle v1.4.1 by Blender Studio and contributors,
  <https://www.blender.org/download/demo-files/> (file
  `human-base-meshes-bundle-v1.4.1.zip`, 50,643,039 bytes, SHA-256
  `811f43accbb31a88266d932f8f5563b2d13586fca0ba2693aad1f5fe582b3515`, downloaded
  2026-10-05 from <https://download.blender.org/demo/asset-bundles/human-base-meshes/>).
- **Licence:** CC0 1.0 (public domain dedication), as stated on the Blender demo files page.
- **How:** `extract.py` copies the objects named `*_primitive_stylized*` from the bundle's
  `human_base_meshes_bundle.blend` into `primitive_stylized_bodies.blend`; nothing else is kept.

The riders (`art/riders/build.py`) are derived from this file: posed on the bike, faceted at
their base level, dressed and fitted with a helmet by script (ADR 0011).
