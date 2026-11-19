"""Builds the building models of the catalogue and exports them for Godot (ADR 0009).

Run in the art container:
    scripts/art.sh blender --background --factory-startup --python art/buildings/build.py

Each model is one mesh with named materials (`plaster`, `tiles`, `glass`, …); the app gives
them their look (textures, tint per building), so the files carry only shape, texture
coordinates and material names. `models.json` beside them lists each model's kind, the
footprint its walls stand on and its heights, for fitting models to map outlines. Pass model
names after `--` to build only those, and `--preview DIR` to render them there instead of
exporting (art/preview.py).
Pass `--detail` after `--` to export nearby Ultra variants separately, without changing the
ordinary GLBs or placement manifest.
"""

import json
import os
import sys

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.dirname(HERE))

import kinds  # noqa: E402  (needs the path above)
import preview  # noqa: E402
import details  # noqa: E402

OUT = os.path.normpath(os.path.join(HERE, "..", "..", "app", "assets", "models", "buildings"))

from blender_io import export, to_object  # noqa: E402


def main():
    argv = list(sys.argv)
    detailed = "--detail" in argv
    if detailed:
        argv.remove("--detail")
    wanted, preview_dir = preview.options(argv)
    with open(os.path.join(HERE, "catalogue.json"), encoding="utf-8") as file:
        catalogue = json.load(file)
    directory = os.path.join(OUT, "detail") if detailed else OUT
    os.makedirs(directory, exist_ok=True)
    manifest = {}
    for spec in catalogue["models"]:
        if wanted and spec["name"] not in wanted:
            continue
        bpy.ops.wm.read_factory_settings(use_empty=True)
        details.ENABLED = detailed
        # The tall French blocks already spend their geometry budget on iron balconies/dormers.
        details.GLAZING_BARS = spec["kind"] != "french_city"
        details.FOOTPRINT = (spec["length"], spec["width"]) if "length" in spec else None
        mesh, shape = getattr(kinds, spec["kind"])(spec)
        obj = to_object(mesh, spec["name"])
        if preview_dir:
            preview.render(obj, preview_dir, spec["name"])
            continue
        export(obj, os.path.join(directory, spec["name"] + ".glb"))
        manifest[spec["name"]] = {"kind": spec["kind"], "roof": spec.get("roof", "gable"),
                                  **{k: round(v, 3) if isinstance(v, float) else v
                                     for k, v in shape.items()}}
        if "pitch" in spec:
            manifest[spec["name"]]["pitch"] = spec["pitch"]
        triangles = sum(len(face) - 2 for face in mesh.faces)
        print(f"built {spec['name']}: {len(mesh.faces)} faces, {triangles} triangles")
    if preview_dir or detailed:
        return
    path = os.path.join(OUT, "models.json")
    if wanted and os.path.exists(path):
        with open(path, encoding="utf-8") as file:
            manifest = {**json.load(file)["models"], **manifest}
    with open(path, "w", encoding="utf-8") as file:
        json.dump({"models": dict(sorted(manifest.items()))}, file, indent=1)
        file.write("\n")


main()
