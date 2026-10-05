"""Extracts the stylized primitive bodies (female and male, every part unchanged) from Blender
Studio's Human Base Meshes bundle into primitive_stylized_bodies.blend, so the repository keeps
only what the riders use instead of the whole 49 MB bundle. See SOURCE.md.

Run in the art container on the unpacked official bundle:
    scripts/art.sh blender --background --factory-startup --python \
        art/sources/human-base-meshes/extract.py -- <path to human_base_meshes_bundle.blend>
"""

import os
import sys

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "primitive_stylized_bodies.blend")
MARKER = "_primitive_stylized"

bundle = sys.argv[sys.argv.index("--") + 1]
bpy.ops.wm.read_factory_settings(use_empty=True)
with bpy.data.libraries.load(bundle, link=False) as (source, target):
    target.objects = [name for name in source.objects if MARKER in name]
for obj in target.objects:
    bpy.context.scene.collection.objects.link(obj)
print(f"extracted {len(target.objects)} parts")
bpy.ops.wm.save_as_mainfile(filepath=OUT, compress=True)
