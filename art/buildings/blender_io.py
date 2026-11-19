"""Turning a `kit.Mesh` into a Blender object and exporting it for Godot, shared by the
building models (`build.py`) and the parts buildings are assembled from (`parts.py`)."""

import bmesh
import bpy

# Colours of the materials in Blender's preview; the app replaces the materials by name.
PREVIEW = {
    "plaster": (0.86, 0.83, 0.77),
    "nordic_wood": (0.58, 0.19, 0.12),
    "stone": (0.6, 0.58, 0.55),
    "castle_stone": (0.56, 0.55, 0.51),
    "wood": (0.45, 0.3, 0.17),
    "wood_dark": (0.28, 0.18, 0.1),
    "frame": (0.93, 0.93, 0.91),
    "glass": (0.08, 0.1, 0.12),
    "glass_detail": (0.08, 0.1, 0.12),
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
    "accent": (0.89, 0.44, 0.37),
    "roof_flat": (0.71, 0.67, 0.64),
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
    if mesh.detailed:
        layer = data.uv_layers.new(name="WindowSeed")
        layer.data.foreach_set("uv", [c for uvs in mesh.detail_uvs for uv in uvs for c in uv])
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
