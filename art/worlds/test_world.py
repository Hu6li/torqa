"""Builds the small test world of world files (ADR 0042, docs/world-files.md): 600 m of road
climbing 3 % through rolling meadows, over a bridge across a creek, past a textured tower and
trees placed as instances, in two sections of two levels of detail each, with a far horizon and
its own afternoon light. Everything is made here, so the world is CC0 like the script.

Run in the art container, then pack it in the dev container:
    scripts/art.sh blender --background --factory-startup --python art/worlds/test_world.py
    scripts/dev.sh sh -c 'cd core && cargo run -q -p orqwa-cli -- world-pack \\
        ../art/worlds/build/test-loop fixtures/worlds/test-loop.oqx'

Blender's axes are east (+x), north (+y) and up (+z); its glTF export turns them into the
format's +x east, +y up, −z north. Every part is placed relative to its section's centre.
"""

import json
import math
import os
import random
import shutil
import struct
import sys

import bmesh
import bpy
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import glb  # noqa: E402  (needs the path above)

OUT = os.path.join(HERE, "build", "test-loop")
LENGTH = 600.0
STEP = 5.0
BASE = 500.0
GRADE = 0.03
BRIDGE = (300.0, 340.0)
ROAD_WIDTH = 5.0
# The terrain's extent: east −500…500, north −100…700, the sections split at north 300.
WEST, EAST, SOUTH, NORTH = -500.0, 500.0, -100.0, 700.0
SECTIONS = [
    {"center": (0.0, 100.0, BASE), "south": SOUTH, "north": 300.0},
    {"center": (0.0, 500.0, BASE), "south": 300.0, "north": NORTH},
]


def road_east(north):
    return 40.0 * math.sin(north / 120.0)


def road_height(north):
    return BASE + GRADE * min(max(north, 0.0), LENGTH)


def ground(east, north):
    """The meadow: rising away from the road, rolling, with a creek under the bridge."""
    along = min(max(north, 0.0), LENGTH)
    away = east - road_east(along)
    roll = 4.0 * math.sin(east / 90.0) * math.sin(north / 70.0) * min(1.0, abs(away) / 60.0)
    creek = 9.0 * math.exp(-(((north - 320.0) / 14.0) ** 2))
    return road_height(north) - 0.25 + 0.0006 * away * away + roll - creek


def route():
    points = []
    north = 0.0
    while north <= LENGTH + 1e-6:
        surface = "bridge" if BRIDGE[0] <= north < BRIDGE[1] else "ground"
        points.append({"east": round(road_east(north), 3), "north": round(north, 3),
                       "elevation": round(road_height(north), 3), "surface": surface,
                       "width": ROAD_WIDTH})
        north += STEP
    return points


def noise(size, seed, scale=1.0):
    """Smooth value noise in 0–1, `size` square, tiling."""
    rng = np.random.default_rng(seed)
    total = np.zeros((size, size))
    amplitude = 1.0
    for octave in (4, 8, 16, 32):
        grid = rng.random((octave, octave))
        y = np.linspace(0, octave, size, endpoint=False)
        y0 = np.floor(y).astype(int)
        t = y - y0
        t = t * t * (3 - 2 * t)
        rows = grid[y0 % octave] * (1 - t)[:, None] + grid[(y0 + 1) % octave] * t[:, None]
        layer = rows[:, y0 % octave] * (1 - t)[None, :] + rows[:, (y0 + 1) % octave] * t[None, :]
        total += layer * amplitude
        amplitude *= 0.5
    total /= total.max()
    return np.clip(total * scale, 0, 1)


def colour_png(size, seed, low, high, scale=1.0):
    n = noise(size, seed, scale)[..., None]
    pixels = np.array(low)[None, None, :] * (1 - n) + np.array(high)[None, None, :] * n
    return glb.png((pixels * 255).astype("uint8"))


def normal_png(size, seed, strength=3.0):
    height = noise(size, seed)
    dx = np.roll(height, -1, axis=1) - np.roll(height, 1, axis=1)
    dy = np.roll(height, -1, axis=0) - np.roll(height, 1, axis=0)
    normal = np.stack([-dx * strength, dy * strength, np.ones_like(dx)], axis=-1)
    normal /= np.linalg.norm(normal, axis=-1, keepdims=True)
    return glb.png(((normal * 0.5 + 0.5) * 255).astype("uint8"))


def save(name, data):
    path = os.path.join(OUT, "sources", name)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as file:
        file.write(data)
    return path


def material(name, colour, texture=None, roughness=0.9):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*colour, 1.0)
    bsdf.inputs["Roughness"].default_value = roughness
    if texture is not None:
        node = mat.node_tree.nodes.new("ShaderNodeTexImage")
        node.image = bpy.data.images.load(texture)
        mat.node_tree.links.new(node.outputs["Color"], bsdf.inputs["Base Color"])
    return mat


def mesh_object(name, vertices, faces, uvs, mat):
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(vertices, [], faces)
    layer = mesh.uv_layers.new(name="UVMap")
    for loop in mesh.loops:
        layer.data[loop.index].uv = uvs[loop.vertex_index]
    mesh.materials.append(mat)
    for polygon in mesh.polygons:
        polygon.use_smooth = True
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return obj


def terrain(section, spacing, mat):
    ce, cn, ch = section["center"]
    columns = int((EAST - WEST) / spacing) + 1
    rows = int((section["north"] - section["south"]) / spacing) + 1
    vertices, uvs, faces = [], [], []
    for row in range(rows):
        north = section["south"] + row * spacing
        for column in range(columns):
            east = WEST + column * spacing
            vertices.append((east - ce, north - cn, ground(east, north) - ch))
            uvs.append(((east - WEST) / (EAST - WEST), (north - SOUTH) / (NORTH - SOUTH)))
    for row in range(rows - 1):
        for column in range(columns - 1):
            a = row * columns + column
            faces.append((a, a + 1, a + columns + 1, a + columns))
    return mesh_object("terrain", vertices, faces, uvs, mat)


def road(section, mat, points):
    """The road on the route's own line (docs/world-files.md, Routes), every 5 m."""
    ce, cn, ch = section["center"]
    inside = [p for p in points if section["south"] - STEP <= p["north"] <= section["north"] + STEP]
    vertices, uvs, faces = [], [], []
    along = 0.0
    for index, point in enumerate(inside):
        ahead = inside[min(index + 1, len(inside) - 1)]
        behind = inside[max(index - 1, 0)]
        de, dn = ahead["east"] - behind["east"], ahead["north"] - behind["north"]
        length = math.hypot(de, dn)
        side = (dn / length, -de / length)
        if index:
            along += math.hypot(point["east"] - inside[index - 1]["east"],
                                point["north"] - inside[index - 1]["north"])
        for offset, u in ((-ROAD_WIDTH / 2, 0.0), (ROAD_WIDTH / 2, 1.0)):
            vertices.append((point["east"] + side[0] * offset - ce,
                             point["north"] + side[1] * offset - cn, point["elevation"] - ch))
            uvs.append((u, along / ROAD_WIDTH))
    for index in range(len(inside) - 1):
        a = index * 2
        faces.append((a, a + 1, a + 3, a + 2))
    return mesh_object("road", vertices, faces, uvs, mat)


def deck(section, points):
    """The bridge's slab, under the road along its own line: 1.2 m deep, a metre wider."""
    ce, cn, ch = section["center"]
    over = [p for p in points if BRIDGE[0] - STEP <= p["north"] <= BRIDGE[1]]
    half = ROAD_WIDTH / 2 + 0.5
    vertices, faces = [], []
    for index, point in enumerate(over):
        ahead = over[min(index + 1, len(over) - 1)]
        behind = over[max(index - 1, 0)]
        de, dn = ahead["east"] - behind["east"], ahead["north"] - behind["north"]
        length = math.hypot(de, dn)
        side = (dn / length, -de / length)
        for offset, depth in ((-half, 0.05), (half, 0.05), (half, 1.25), (-half, 1.25)):
            vertices.append((point["east"] + side[0] * offset - ce,
                             point["north"] + side[1] * offset - cn,
                             point["elevation"] - depth - ch))
    for index in range(len(over) - 1):
        a, b = index * 4, index * 4 + 4
        for k in range(4):
            faces.append((a + k, a + (k + 1) % 4, b + (k + 1) % 4, b + k))
    last = (len(over) - 1) * 4
    faces += [(3, 2, 1, 0), (last, last + 1, last + 2, last + 3)]
    obj = mesh_object("deck", vertices, faces, [(v[0] / 4.0, v[2] / 4.0) for v in vertices],
                      MATERIALS["concrete"])
    for polygon in obj.data.polygons:
        polygon.use_smooth = False
    return obj


def box(name, east, north, bottom, size, height, mat, centre):
    ce, cn, ch = centre
    w, d = size
    corners = [(east + x * w / 2 - ce, north + y * d / 2 - cn) for x, y in
               ((-1, -1), (1, -1), (1, 1), (-1, 1))]
    vertices = [(x, y, bottom - ch) for x, y in corners] + [(x, y, bottom + height - ch)
                                                            for x, y in corners]
    faces = [(0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4), (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7)]
    obj = mesh_object(name, vertices, faces, [(v[0] / 4.0, v[2] / 4.0) for v in vertices], mat)
    for polygon in obj.data.polygons:
        polygon.use_smooth = False
    # Bricks by the metre on every wall: the box's own UVs per face.
    layer = obj.data.uv_layers[0]
    for polygon in obj.data.polygons:
        for loop in polygon.loop_indices:
            x, y, z = obj.data.vertices[obj.data.loops[loop].vertex_index].co
            horizontal = x if abs(polygon.normal.y) > 0.5 else y
            layer.data[loop].uv = (horizontal / 4.0, z / 4.0)
    return obj


def tree(lod):
    """A broadleaf tree at the origin: trunk and crown, fewer sides further away."""
    sides = 12 if lod == 0 else 5
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=sides, radius1=0.35, radius2=0.25,
                          depth=4.0, matrix=mathutils_translation(0, 0, 2.0))
    trunk = bpy.data.meshes.new(f"tree-{lod}")
    bm.to_mesh(trunk)
    bm.free()
    crown = bmesh.new()
    bmesh.ops.create_icosphere(crown, subdivisions=2 if lod == 0 else 1, radius=3.2,
                               matrix=mathutils_translation(0, 0, 6.5))
    leaves = bpy.data.meshes.new(f"crown-{lod}")
    crown.to_mesh(leaves)
    crown.free()
    obj = bpy.data.objects.new(f"tree-{lod}", trunk)
    bpy.context.collection.objects.link(obj)
    crown_obj = bpy.data.objects.new(f"crown-{lod}", leaves)
    bpy.context.collection.objects.link(crown_obj)
    trunk.materials.append(MATERIALS["bark"])
    leaves.materials.append(MATERIALS["leaves"])
    # One mesh per asset (docs/world-files.md): joined.
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    crown_obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.join()
    return obj


def mathutils_translation(x, y, z):
    import mathutils
    return mathutils.Matrix.Translation((x, y, z))


def horizon():
    """Hills from 1.5 km out to 8 km round the origin, low and coarse."""
    rings, segments = 7, 48
    vertices, uvs, faces = [], [], []
    rng = random.Random(7)
    heights = [[rng.uniform(20, 260) for _ in range(segments)] for _ in range(rings)]
    for ring in range(rings):
        radius = 1500.0 + ring * (6500.0 / (rings - 1))
        for segment in range(segments):
            angle = segment / segments * math.tau
            rise = 0.0 if ring == 0 else heights[ring][segment] * min(1.0, ring / 2)
            vertices.append((math.sin(angle) * radius, math.cos(angle) * radius + 300.0,
                             480.0 + rise))
            uvs.append((segment / segments, ring / rings))
    for ring in range(rings - 1):
        for segment in range(segments):
            a = ring * segments + segment
            b = ring * segments + (segment + 1) % segments
            faces.append((a, b, b + segments, a + segments))
    return mesh_object("horizon", vertices, faces, uvs, MATERIALS["hills"])


def export(objects, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        use_selection=True,
        export_apply=True,
        export_materials="EXPORT",
        export_yup=True,
        export_animations=False,
        export_cameras=False,
        export_lights=False,
        export_extras=False,
    )


def instances(section, placed, path):
    """The section's trees as an `instances` file: one tinted group of asset 0."""
    ce, cn, ch = section["center"]
    floats = []
    for east, north, height, yaw, scale, tint in placed:
        c, s = math.cos(yaw) * scale, math.sin(yaw) * scale
        floats += [c, 0.0, s, east - ce, 0.0, scale, 0.0, height - ch, -s, 0.0, c, -(north - cn)]
        floats += [*tint, 1.0]
    with open(path, "wb") as file:
        file.write(b"OQXI" + struct.pack("<II", 1, 1))
        file.write(struct.pack("<III", 0, len(placed), 1))
        file.write(struct.pack(f"<{len(floats)}f", *floats))


MATERIALS = {}


def main():
    if os.path.exists(OUT):
        shutil.rmtree(OUT)
    os.makedirs(OUT)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    meadow = save("meadow.png", colour_png(512, 1, (0.30, 0.42, 0.16), (0.52, 0.58, 0.25)))
    asphalt = save("asphalt.png", colour_png(128, 2, (0.16, 0.16, 0.17), (0.32, 0.31, 0.30)))
    bricks = np.zeros((128, 128, 3))
    mortar = (np.arange(128)[:, None] % 32 < 3) | (
        (np.arange(128)[None, :] + 32 * ((np.arange(128)[:, None] // 32) % 2)) % 64 < 3)
    bricks[...] = np.where(mortar[..., None], (0.78, 0.76, 0.72), (0.55, 0.24, 0.17))
    bricks *= 0.85 + 0.15 * noise(128, 3)[..., None]
    brick = save("bricks.png", glb.png((bricks * 255).astype("uint8")))
    MATERIALS.update({
        "meadow": material("meadow", (1, 1, 1), meadow),
        "asphalt": material("asphalt", (1, 1, 1), asphalt, 0.8),
        "concrete": material("concrete", (0.62, 0.61, 0.58), None, 0.85),
        "bricks": material("bricks", (1, 1, 1), brick, 0.9),
        "bark": material("bark", (0.28, 0.2, 0.13), None, 0.95),
        "leaves": material("leaves", (0.2, 0.38, 0.12), None, 0.8),
        "hills": material("hills", (0.36, 0.45, 0.35), None, 1.0),
    })
    points = route()
    os.makedirs(os.path.join(OUT, "routes"))
    with open(os.path.join(OUT, "routes", "loop.json"), "w") as file:
        json.dump({"points": points}, file)

    # Trees beside the road, never on it, on the meadow's ground.
    rng = random.Random(42)
    trees = []
    while len(trees) < 80:
        north = rng.uniform(-60.0, 680.0)
        if BRIDGE[0] - 30 < north < BRIDGE[1] + 30:
            continue
        east = road_east(min(max(north, 0.0), LENGTH)) + rng.choice((-1, 1)) * rng.uniform(9, 120)
        green = rng.uniform(0.8, 1.1)
        trees.append((east, north, ground(east, north) - 0.2, rng.uniform(0, math.tau),
                      rng.uniform(0.8, 1.3), (green * 0.95, green, green * 0.9)))

    world_sections = []
    for index, section in enumerate(SECTIONS):
        folder = os.path.join(OUT, "sections", str(index))
        objects = [terrain(section, 10.0, MATERIALS["meadow"]), road(section, MATERIALS["asphalt"], points)]
        ce, cn, ch = section["center"]
        if section["south"] <= BRIDGE[0] < section["north"]:
            objects.append(deck(section, points))
        if section["south"] <= 150.0 < section["north"]:
            objects.append(box("tower", 80.0, 150.0, ground(80.0, 150.0) - 1, (6, 6), 22,
                               MATERIALS["bricks"], section["center"]))
        export(objects, os.path.join(folder, "lod0.glb"))
        far = [terrain(section, 40.0, MATERIALS["meadow"]), road(section, MATERIALS["asphalt"], points)]
        far += [o for o in objects if o.name.startswith(("deck", "tower"))]
        export(far, os.path.join(folder, "lod1.glb"))
        # The near ground gets detail: grass everywhere, gravel on the road's shoulders.
        gltf, binary = glb.read(os.path.join(folder, "lod0.glb"))
        size = 256
        weights = np.zeros((size, size, 4))
        for y in range(size):
            north = NORTH - (y + 0.5) / size * (NORTH - SOUTH)
            for x in range(size):
                east = WEST + (x + 0.5) / size * (EAST - WEST)
                shoulder = abs(east - road_east(min(max(north, 0.0), LENGTH))) < 6.0
                weights[y, x] = (0.0, 1.0, 0.0, 1.0) if shoulder else (1.0, 0.0, 0.0, 1.0)
        glb.add_terrain_layers(
            gltf, binary, "meadow", [glb.png((weights * 255).astype("uint8"))],
            [(colour_png(256, 4, (0.2, 0.35, 0.1), (0.45, 0.6, 0.2)), normal_png(256, 5), 0.95, 2.0),
             (colour_png(256, 6, (0.45, 0.42, 0.38), (0.7, 0.66, 0.6)), normal_png(256, 7), 0.8, 1.5)])
        glb.write(os.path.join(folder, "lod0.glb"), gltf, binary)
        mine = [t for t in trees if section["south"] <= t[1] < section["north"]]
        instances(section, mine, os.path.join(folder, "instances.bin"))
        world_sections.append({
            "center": [ce, cn, ch], "radius": 560.0,
            "lods": [{"file": f"sections/{index}/lod0.glb", "until_m": 400.0},
                     {"file": f"sections/{index}/lod1.glb"}],
            "instances": f"sections/{index}/instances.bin",
        })
        for name in {obj.name for obj in objects + far}:
            bpy.data.objects.remove(bpy.data.objects[name], do_unlink=True)

    for lod in (0, 1):
        export([tree(lod)], os.path.join(OUT, "assets", "tree", f"lod{lod}.glb"))
    export([horizon()], os.path.join(OUT, "horizon.glb"))
    preview = colour_png(256, 9, (0.25, 0.4, 0.2), (0.5, 0.6, 0.35))
    with open(os.path.join(OUT, "preview.png"), "wb") as file:
        file.write(preview)
    with open(os.path.join(OUT, "world.json"), "w") as file:
        json.dump({
            "format": 1,
            "id": "1d3f1c0e-5b7a-4c1e-9a55-6f0d2c3b4a10",
            "name": "Test loop",
            "version": "1",
            "publisher": "ORQWA",
            "licence": "CC0-1.0",
            "attribution": ["Made by art/worlds/test_world.py"],
            "created_unix_s": 1795132800,
            "origin": {"lat": 46.95, "lon": 7.45, "located": True},
            "drives_on": "right",
            "environment": {
                "sun": {"elevation_deg": 34, "azimuth_deg": 235, "colour": [1.0, 0.95, 0.87],
                        "energy": 1.5},
                "ambient": {"colour": [0.62, 0.7, 0.82], "energy": 0.8},
                "sky": {"kind": "atmosphere", "clouds": 0.3, "overcast": 0.0},
                "fog": {"colour": [0.75, 0.8, 0.88], "density": 0.00008},
            },
            "sections": world_sections,
            "assets": [{"name": "tree", "lods": [{"file": "assets/tree/lod0.glb", "until_m": 250.0},
                                                {"file": "assets/tree/lod1.glb"}]}],
            "horizon": "horizon.glb",
            "preview": "preview.png",
            "routes": [{"id": "loop", "name": "Test loop", "file": "routes/loop.json",
                        "countries": [{"code": "CH", "names": {
                            "en": "Switzerland", "de": "Schweiz", "": "Schweiz"
                        }}]}],
        }, file, indent=1)
    shutil.rmtree(os.path.join(OUT, "sources"))
    print(f"test world written to {OUT}")


main()
