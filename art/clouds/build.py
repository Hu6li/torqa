"""Builds the cloud models and exports them for Godot (ADR 0009, ADR 0011).

Run in the art container:
    scripts/art.sh blender --background --factory-startup --python art/clouds/build.py

Low-poly clouds as in the references: a few chunky faceted puffs, flat underneath, made of
rough balls of 20 facets each (as the broadleaf crowns in art/vegetation). One material,
`cloud`; the app colours and lights them (`app/shaders/cloud.gdshader`) and scales them to
size. Models are about 10 m long; random shapes use fixed seeds, so rebuilds give the same
files.
"""

import math
import os
import random

import bmesh
import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.normpath(os.path.join(HERE, "..", "..", "app", "assets", "models", "clouds"))

# Puffs: (x, y, z, radius) in metres, x along the cloud.
CATALOGUE = [
    {"name": "cloud_small", "seed": 3,
     "puffs": [(-1.6, 0.0, 0.0, 1.8), (0.4, 0.3, 0.4, 2.3), (2.1, -0.2, 0.0, 1.5)]},
    {"name": "cloud_puffy", "seed": 11,
     "puffs": [(-3.2, 0.2, 0.0, 1.7), (-1.4, -0.3, 0.6, 2.4), (0.8, 0.4, 1.0, 2.8),
               (2.9, -0.2, 0.3, 2.0), (4.4, 0.3, -0.1, 1.3)]},
    {"name": "cloud_long", "seed": 23,
     "puffs": [(-5.0, 0.0, 0.0, 1.4), (-3.3, 0.4, 0.3, 1.9), (-1.2, -0.3, 0.5, 2.2),
               (1.0, 0.2, 0.4, 2.0), (3.1, -0.3, 0.2, 1.8), (4.9, 0.2, -0.1, 1.3)]},
    {"name": "cloud_tower", "seed": 37,
     "puffs": [(-2.4, 0.0, 0.0, 2.0), (0.0, 0.3, 0.2, 2.6), (2.3, -0.2, 0.0, 1.9),
               (-0.6, 0.0, 2.4, 2.1), (0.7, -0.2, 4.2, 1.6)]},
]
# Undersides are flat this far below the middle of the lowest puffs.
FLOOR = -0.6
PREVIEW = (0.98, 0.97, 0.93)

ICO_FACES = [(0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11), (1, 5, 9), (5, 11, 4),
             (11, 10, 2), (10, 7, 6), (7, 1, 8), (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8),
             (3, 8, 9), (4, 9, 5), (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1)]


def puff(faces, centre, radius, seed):
    """A rough ball of 20 facets, flattened underneath at `FLOOR`, a little wider than high."""
    t = (1 + 5 ** 0.5) / 2
    corners = [(-1, t, 0), (1, t, 0), (-1, -t, 0), (1, -t, 0), (0, -1, t), (0, 1, t), (0, -1, -t),
               (0, 1, -t), (t, 0, -1), (t, 0, 1), (-t, 0, -1), (-t, 0, 1)]
    rng = random.Random(seed)
    turn = rng.uniform(0, 2 * math.pi)
    points = []
    for x, y, z in corners:
        length = math.sqrt(x * x + y * y + z * z)
        k = radius * (1 + rng.uniform(-0.15, 0.15)) / length
        x, y, z = x * k * 1.15, y * k, z * k * 0.85
        x, y = x * math.cos(turn) - y * math.sin(turn), x * math.sin(turn) + y * math.cos(turn)
        points.append((centre[0] + x, centre[1] + y, max(centre[2] + z, FLOOR)))
    for a, b, c in ICO_FACES:
        face = [points[a], points[b], points[c]]
        if not outward(face, centre):
            face.reverse()
        faces.append(face)


def outward(face, centre):
    """Whether the face's corners run counter-clockwise seen from outside, away from centre."""
    (ax, ay, az), (bx, by, bz), (cx, cy, cz) = face
    ux, uy, uz = bx - ax, by - ay, bz - az
    vx, vy, vz = cx - ax, cy - ay, cz - az
    normal = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx)
    middle = [(ax + bx + cx) / 3 - centre[0], (ay + by + cy) / 3 - centre[1],
              (az + bz + cz) / 3 - centre[2]]
    return sum(n * m for n, m in zip(normal, middle)) > 0


def build(spec):
    faces = []
    for k, (x, y, z, radius) in enumerate(spec["puffs"]):
        puff(faces, (x, y, z), radius, spec["seed"] + k)
    data = bpy.data.meshes.new(spec["name"])
    verts = [p for face in faces for p in face]
    data.from_pydata(verts, [], [list(range(3 * i, 3 * i + 3)) for i in range(len(faces))])
    material = bpy.data.materials.get("cloud") or bpy.data.materials.new("cloud")
    material.diffuse_color = (*PREVIEW, 1.0)
    data.materials.append(material)
    data.polygons.foreach_set("use_smooth", [False] * len(faces))
    shared = bmesh.new()
    shared.from_mesh(data)
    bmesh.ops.remove_doubles(shared, verts=shared.verts, dist=1e-4)
    # Facets squashed flat against the floor add nothing.
    bmesh.ops.dissolve_degenerate(shared, edges=shared.edges, dist=1e-4)
    shared.to_mesh(data)
    shared.free()
    obj = bpy.data.objects.new(spec["name"], data)
    bpy.context.scene.collection.objects.link(obj)
    return obj, len(data.polygons)


def main():
    os.makedirs(OUT, exist_ok=True)
    for spec in CATALOGUE:
        bpy.ops.wm.read_factory_settings(use_empty=True)
        obj, count = build(spec)
        obj.select_set(True)
        bpy.context.view_layer.objects.active = obj
        bpy.ops.export_scene.gltf(
            filepath=os.path.join(OUT, spec["name"] + ".glb"),
            export_format="GLB",
            use_selection=True,
            export_apply=True,
            export_materials="EXPORT",
            export_yup=True,
        )
        print(f"built {spec['name']}: {count} faces")


main()
