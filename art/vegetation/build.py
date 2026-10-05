"""Builds the vegetation models — trees, a bush and rocks — and exports them for Godot
(ADR 0009, ADR 0011).

Run in the art container:
    scripts/art.sh blender --background --factory-startup --python art/vegetation/build.py

The look is faceted low-poly (skill torqa-look): conifers as stacked cones, broadleaf crowns as
a few chunky facets, rocks as rough blocks. Each model is one mesh with named materials
(`leaves`, `trunk`, `rock`); the app colours them — leaves and rocks per instance from the
palette — so the files carry only shape and material names. `models.json` beside them lists
each model's kind and height for the world to choose and scale them. Pass model names after
`--` to build only those. Random shapes use fixed seeds, so rebuilds give the same files.
"""

import json
import math
import os
import random
import sys

import bmesh
import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.normpath(os.path.join(HERE, "..", "..", "app", "assets", "models", "vegetation"))

# Preview colours in Blender only; the app replaces the materials by name.
PREVIEW = {"leaves": (0.55, 0.6, 0.33), "trunk": (0.48, 0.34, 0.33), "rock": (0.76, 0.73, 0.7)}


class Mesh:
    """Faces with material names, built up piece by piece (z up, metres)."""

    def __init__(self):
        self.verts = []
        self.faces = []
        self.materials = []

    def face(self, points, material):
        start = len(self.verts)
        self.verts.extend(points)
        self.faces.append(list(range(start, start + len(points))))
        self.materials.append(material)


def ring(centre, radius, sides, turn, height):
    return [
        (centre[0] + radius * math.cos(turn + 2 * math.pi * i / sides),
         centre[1] + radius * math.sin(turn + 2 * math.pi * i / sides),
         height)
        for i in range(sides)
    ]


def frustum(mesh, base, top, r_base, r_top, sides, material, turn=0.0, cap=True):
    """A cone (r_top 0) or truncated cone from height base to top, counter-clockwise from
    outside; `cap` closes its underside."""
    low = ring((0, 0), r_base, sides, turn, base)
    if r_top > 0:
        high = ring((0, 0), r_top, sides, turn, top)
        for i in range(sides):
            j = (i + 1) % sides
            mesh.face([low[i], low[j], high[j], high[i]], material)
    else:
        for i in range(sides):
            j = (i + 1) % sides
            mesh.face([low[i], low[j], (0.0, 0.0, top)], material)
    if cap:
        mesh.face(list(reversed(low)), material)


ICO_FACES = [(0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11), (1, 5, 9), (5, 11, 4),
             (11, 10, 2), (10, 7, 6), (7, 1, 8), (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8),
             (3, 8, 9), (4, 9, 5), (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1)]


def blob(mesh, centre, radius, material, seed, scale=(1.0, 1.0, 1.0), jitter=0.18, floor=None):
    """A rough ball of 20 facets: an icosahedron with its corners pushed in and out; `floor`
    flattens its underside at that height."""
    t = (1 + 5 ** 0.5) / 2
    corners = [(-1, t, 0), (1, t, 0), (-1, -t, 0), (1, -t, 0), (0, -1, t), (0, 1, t), (0, -1, -t),
               (0, 1, -t), (t, 0, -1), (t, 0, 1), (-t, 0, -1), (-t, 0, 1)]
    rng = random.Random(seed)
    turn = rng.uniform(0, 2 * math.pi)
    points = []
    for x, y, z in corners:
        length = math.sqrt(x * x + y * y + z * z)
        k = radius * (1 + rng.uniform(-jitter, jitter)) / length
        x, y, z = x * k * scale[0], y * k * scale[1], z * k * scale[2]
        x, y = x * math.cos(turn) - y * math.sin(turn), x * math.sin(turn) + y * math.cos(turn)
        z = centre[2] + z
        if floor is not None:
            z = max(z, floor)
        points.append((centre[0] + x, centre[1] + y, z))
    for a, b, c in ICO_FACES:
        face = [points[a], points[b], points[c]]
        if not outward(face, centre):
            face.reverse()
        mesh.face(face, material)


def outward(face, centre):
    """Whether the face's corners run counter-clockwise seen from outside, away from centre."""
    (ax, ay, az), (bx, by, bz), (cx, cy, cz) = face
    ux, uy, uz = bx - ax, by - ay, bz - az
    vx, vy, vz = cx - ax, cy - ay, cz - az
    normal = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx)
    middle = [(ax + bx + cx) / 3 - centre[0], (ay + by + cy) / 3 - centre[1],
              (az + bz + cz) / 3 - centre[2]]
    return sum(n * m for n, m in zip(normal, middle)) > 0


# --- kinds ---------------------------------------------------------------------------------


def conifer(spec):
    """A trunk under stacked cones, each turned a little against the one below."""
    mesh = Mesh()
    frustum(mesh, -0.6, spec["trunk"], 0.32, 0.22, 5, "trunk", cap=False)
    for k, (base, top, radius) in enumerate(spec["tiers"]):
        frustum(mesh, base, top, radius, 0.0, spec.get("sides", 6), "leaves", turn=k * 0.45)
    return mesh, max(top for _, top, _ in spec["tiers"])


def broadleaf(spec):
    """A trunk under one or more chunky blobs of leaves."""
    mesh = Mesh()
    frustum(mesh, -0.6, spec["trunk"], 0.34, 0.2, 5, "trunk", cap=False)
    height = 0.0
    for k, (x, y, z, radius, squash) in enumerate(spec["crowns"]):
        blob(mesh, (x, y, z), radius, "leaves", spec["seed"] + k, scale=(1.0, 1.0, squash))
        height = max(height, z + radius * squash * 1.15)
    return mesh, height


def bush(spec):
    mesh = Mesh()
    height = 0.0
    for k, (x, y, z, radius, squash) in enumerate(spec["clumps"]):
        blob(mesh, (x, y, z), radius, "leaves", spec["seed"] + k, scale=(1.15, 1.0, squash),
             floor=-0.2)
        height = max(height, z + radius * squash * 1.15)
    return mesh, height


def rock(spec):
    """Rough blocks, sunk into the ground a little so slopes do not show their undersides."""
    mesh = Mesh()
    height = 0.0
    for k, (x, y, z, radius, scale) in enumerate(spec["blocks"]):
        blob(mesh, (x, y, z), radius, "rock", spec["seed"] + k, scale=scale, jitter=0.3)
        height = max(height, z + radius * scale[2] * 1.3)
    return mesh, height


KINDS = {"conifer": conifer, "broadleaf": broadleaf, "bush": bush, "rock": rock}

CATALOGUE = [
    {"name": "conifer_tall", "kind": "conifer", "trunk": 2.4, "sides": 6,
     "tiers": [(1.6, 6.6, 2.6), (4.4, 9.2, 2.1), (7.0, 11.8, 1.6), (9.6, 14.0, 1.1)]},
    {"name": "conifer_broad", "kind": "conifer", "trunk": 1.8, "sides": 7,
     "tiers": [(1.2, 5.6, 3.0), (3.8, 8.0, 2.3), (6.2, 10.4, 1.5)]},
    {"name": "broadleaf_round", "kind": "broadleaf", "trunk": 3.6, "seed": 11,
     "crowns": [(0.0, 0.0, 5.6, 3.2, 0.9)]},
    {"name": "broadleaf_cluster", "kind": "broadleaf", "trunk": 3.4, "seed": 23,
     "crowns": [(0.6, 0.3, 4.9, 2.4, 0.85), (-0.8, -0.4, 5.3, 2.3, 0.85),
                (0.1, -0.1, 6.9, 2.0, 0.9)]},
    {"name": "bush", "kind": "bush", "seed": 37,
     "clumps": [(0.0, 0.0, 0.55, 1.05, 0.75), (0.75, 0.45, 0.4, 0.8, 0.7)]},
    {"name": "rock_block", "kind": "rock", "seed": 41,
     "blocks": [(0.0, 0.0, 0.25, 1.0, (1.3, 1.0, 0.75))]},
    {"name": "rock_pair", "kind": "rock", "seed": 53,
     "blocks": [(0.0, 0.0, 0.2, 0.9, (1.2, 1.0, 0.7)), (1.1, 0.5, 0.0, 0.6, (1.1, 1.0, 0.8))]},
]


# --- export ----------------------------------------------------------------------------------


def material(name):
    found = bpy.data.materials.get(name)
    if found:
        return found
    created = bpy.data.materials.new(name)
    created.diffuse_color = (*PREVIEW[name], 1.0)
    return created


def to_object(mesh, name):
    data = bpy.data.meshes.new(name)
    data.from_pydata(mesh.verts, [], mesh.faces)
    names = sorted(set(mesh.materials))
    for n in names:
        data.materials.append(material(n))
    index = {n: i for i, n in enumerate(names)}
    data.polygons.foreach_set("material_index", [index[m] for m in mesh.materials])
    # Flat: one normal per face (skill torqa-look); faces stay separate for that.
    data.polygons.foreach_set("use_smooth", [False] * len(mesh.faces))
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
    os.makedirs(OUT, exist_ok=True)
    manifest = {}
    for spec in CATALOGUE:
        if wanted and spec["name"] not in wanted:
            continue
        bpy.ops.wm.read_factory_settings(use_empty=True)
        mesh, height = KINDS[spec["kind"]](spec)
        export(to_object(mesh, spec["name"]), os.path.join(OUT, spec["name"] + ".glb"))
        manifest[spec["name"]] = {"kind": spec["kind"], "height": round(height, 2)}
        triangles = sum(len(face) - 2 for face in mesh.faces)
        print(f"built {spec['name']}: {triangles} triangles, {height:.1f} m")
    path = os.path.join(OUT, "models.json")
    if wanted and os.path.exists(path):
        with open(path, encoding="utf-8") as file:
            manifest = {**json.load(file)["models"], **manifest}
    with open(path, "w", encoding="utf-8") as file:
        json.dump({"models": dict(sorted(manifest.items()))}, file, indent=1)
        file.write("\n")


main()
