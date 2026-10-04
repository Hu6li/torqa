"""Builds the building models of the catalogue and exports them for Godot (ADR 0009).

Run in the art container:
    scripts/art.sh blender --background --factory-startup --python art/buildings/build.py

Each model is one mesh with named materials (`plaster`, `tiles`, `glass`, …); the app gives
them their look (textures, tint per building), so the files carry only shape, texture
coordinates and material names. `models.json` beside them lists each model's kind, the
footprint its walls stand on and its heights, for fitting models to map outlines. Pass model
names after `--` to build only those.
"""

import json
import os
import sys

import bmesh
import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import kinds  # noqa: E402  (needs the path above)

OUT = os.path.normpath(os.path.join(HERE, "..", "..", "app", "assets", "models", "buildings"))

# Colours of the materials in Blender's preview; the app replaces the materials by name.
PREVIEW = {
    "plaster": (0.86, 0.83, 0.77),
    "stone": (0.6, 0.58, 0.55),
    "wood": (0.45, 0.3, 0.17),
    "wood_dark": (0.28, 0.18, 0.1),
    "frame": (0.93, 0.93, 0.91),
    "glass": (0.08, 0.1, 0.12),
    "shutter": (0.22, 0.38, 0.26),
    "door": (0.38, 0.24, 0.13),
    "tiles": (0.55, 0.26, 0.17),
    "slate": (0.24, 0.25, 0.27),
    "metal": (0.55, 0.56, 0.58),
    "copper": (0.35, 0.55, 0.47),
    "flowers": (0.82, 0.08, 0.12),
    "leaves": (0.14, 0.33, 0.1),
    "clock": (0.92, 0.9, 0.85),
    "sheet": (0.45, 0.46, 0.48),
    "garage": (0.8, 0.8, 0.78),
    "wood_light": (0.72, 0.58, 0.4),
}


def material(name):
    found = bpy.data.materials.get(name)
    if found:
        return found
    created = bpy.data.materials.new(name)
    rgb = PREVIEW.get(name, (0.8, 0.8, 0.8))
    created.diffuse_color = (*rgb, 1.0)
    if created.node_tree is None and hasattr(created, "use_nodes"):
        created.use_nodes = True
    if created.node_tree is not None:
        principled = created.node_tree.nodes.get("Principled BSDF")
        if principled is not None:
            principled.inputs["Base Color"].default_value = (*rgb, 1.0)
    return created


def to_object(mesh, name):
    data = bpy.data.meshes.new(name)
    data.from_pydata([tuple(v) for v in mesh.verts], [], mesh.faces)
    names = sorted(set(mesh.materials))
    for n in names:
        data.materials.append(material(n))
    index = {n: i for i, n in enumerate(names)}
    data.polygons.foreach_set("material_index", [index[m] for m in mesh.materials])
    data.polygons.foreach_set("use_smooth", mesh.smooth)
    layer = data.uv_layers.new(name="UVMap")
    layer.data.foreach_set("uv", [c for uvs in mesh.uvs for uv in uvs for c in uv])
    # Faces were built separately; sharing their corners makes the files much smaller (the
    # exporter still splits them where normals or texture coordinates differ).
    shared = bmesh.new()
    shared.from_mesh(data)
    bmesh.ops.remove_doubles(shared, verts=shared.verts, dist=1e-4)
    shared.to_mesh(data)
    shared.free()
    data.update()
    obj = bpy.data.objects.new(name, data)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def export(obj, path):
    for other in bpy.context.scene.objects:
        other.select_set(False)
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        use_selection=True,
        export_apply=True,
        export_materials="EXPORT",
        export_yup=True,
    )


def main():
    wanted = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    with open(os.path.join(HERE, "catalogue.json"), encoding="utf-8") as file:
        catalogue = json.load(file)
    os.makedirs(OUT, exist_ok=True)
    manifest = {}
    for spec in catalogue["models"]:
        if wanted and spec["name"] not in wanted:
            continue
        bpy.ops.wm.read_factory_settings(use_empty=True)
        mesh, shape = getattr(kinds, spec["kind"])(spec)
        obj = to_object(mesh, spec["name"])
        export(obj, os.path.join(OUT, spec["name"] + ".glb"))
        manifest[spec["name"]] = {"kind": spec["kind"], "roof": spec.get("roof", "gable"),
                                  **{k: round(v, 3) if isinstance(v, float) else v
                                     for k, v in shape.items()}}
        print(f"built {spec['name']}: {len(mesh.faces)} faces")
    path = os.path.join(OUT, "models.json")
    if wanted and os.path.exists(path):
        with open(path, encoding="utf-8") as file:
            manifest = {**json.load(file)["models"], **manifest}
    with open(path, "w", encoding="utf-8") as file:
        json.dump({"models": dict(sorted(manifest.items()))}, file, indent=1)
        file.write("\n")


main()
