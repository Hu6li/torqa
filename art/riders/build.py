"""Builds the riders on their bikes (R46, ADR 0020) and exports them for Godot.

Run in the art container:
    scripts/art.sh blender --background --factory-startup --python art/riders/build.py
    (with PREVIEW=<dir> also renders Cycles previews of the riders there)

Each rider starts from a realistic body of Blender Studio's CC0 Human Base Meshes
(art/sources/human-base-meshes/realistic_bodies.blend): a skeleton is built from the bundle's
own joints, scaled to the body, and the body skinned to it. Hair, a helmet and sunglasses are
fitted to the head's shape (and a ponytail for the female rider). The rider is seated as a bike
fitter would: the hips where the legs reach the pedals almost straight, the back bent forward
until the hands reach the hoods, the feet on the pedals; the bike is built around those contact
points, so each rider gets their own. The riding pose becomes the skeleton's rest; the body is
thinned for the app, the kit cut into it along planes across the limbs (shorts, jersey with
sleeves, socks, white shoes), shoes closed over the toes, bare hands gripping the hoods.

`rider_<sex>.glb` holds, in Godot's axes (x right, y up, −z forward; metres, the origin on the
ground between the wheels):
- `skeleton` with `body` skinned to it: bones pelvis, spine, chest, neck, head and
  shoulder/upper_arm/forearm/hand, thigh/shin/foot/toe with `.L` and `.R`; the app turns the
  leg bones to the pedals and moves the pelvis (and all above it) as the rider sways and stands;
- `frame`, `wheel_front`, `wheel_rear`: the bike, the wheels with their origins at the hubs;
- `crankset` (origin at the bottom bracket, the right crank pointing forward) and `pedal_l`,
  `pedal_r` (origins at the axles).

`riders.json` beside them holds what the app needs to pedal: hips, thigh and shin lengths,
where the ankle sits over the pedal axle, the hands on the hoods, and the bike's hubs, bottom
bracket and cranks. The app colours the materials by name from the palette (`rider_female`,
`rider_male`, `bike`).
"""

import json
import math
import os
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import helmet  # noqa: E402  (needs the path above)

OUT = os.path.normpath(os.path.join(HERE, "..", "..", "app", "assets", "models", "riders"))

EYES = ("eye",)

# The bike, in Blender's axes: x right, y forward, z up. Built with the bottom bracket at
# y = 0, then moved so the origin lies between the wheels.
WHEEL_RADIUS = 0.34
BOTTOM_BRACKET = Vector((0.0, 0.0, 0.27))
CHAINSTAY = 0.41
CRANK = 0.17
PEDAL_OFFSET = 0.12
# The pedal's top above its axle, and the cleat and sole under the shoe.
PEDAL_TOP = 0.008
CLEAT = 0.01
SEAT_ANGLE = math.radians(74.0)
HEAD_ANGLE = math.radians(72.5)
FORK_RAKE = 0.045
# Axle to crown, along the steering axis.
FORK_LENGTH = 0.37
STEM = 0.1
HOODS_HALF_WIDTH = 0.2
# Saddle top to hoods top, as on a road bike set up for riding fast.
DROP_TO_HOODS = 0.08
# The hoods at least this far ahead of the bottom bracket for a rider of the reference
# skeleton's height (1.762 m), scaled to each rider: a road bike's reach and stem; it also
# keeps the front wheel clear of the feet.
HOODS_AHEAD = 0.57
# Bike fit: the knee's bend with the pedal at the bottom, and the foot's pitch there (toes
# down; app/scenes/rider_avatar.gd turns the feet so, ANKLE_PITCH and ANKLE_SWING).
KNEE_AT_BOTTOM = math.radians(30.0)
FOOT_AT_BOTTOM = math.radians(-18.0)
# How far the hood's body sits ahead of where the palm rests on it: behind, so the hand holds
# the horn, the fingers wrapped over its front onto the lever, as racers ride the hoods.
HOOD_AHEAD_OF_PALM = -0.05
# Where the knuckles' joints go, from the hood's top: at the horn, the palm pressed onto the
# hood; the fingers then curl down over the horn's front.
KNUCKLES_ON_HOOD = Vector((0.0, 0.0, 0.01))
# The wrist sits behind and above the hood's top, the palm on it.
WRIST_ON_HOOD = Vector((0.0, -0.06, 0.045))

PREVIEW = {
    "skin": (0.74, 0.53, 0.47), "jersey": (0.94, 0.71, 0.8), "sleeve": (0.81, 0.35, 0.65),
    "shorts": (0.18, 0.24, 0.48), "shoes": (0.92, 0.92, 0.9),
    "helmet": (0.96, 0.76, 0.43), "glasses": (0.29, 0.18, 0.27), "hair": (0.38, 0.26, 0.4),
    "vents": (0.29, 0.18, 0.27),
    "frame": (0.89, 0.44, 0.37), "tyre": (0.29, 0.18, 0.27), "rim": (0.95, 0.92, 0.85),
    "metal": (0.56, 0.58, 0.61), "saddle": (0.38, 0.26, 0.4), "bar": (0.38, 0.26, 0.4),
    "sidewall": (0.39, 0.34, 0.29), "carbon": (0.08, 0.09, 0.1),
    "alloy": (0.12, 0.13, 0.15), "bottle": (0.8, 0.81, 0.78),
    "socks": (0.92, 0.92, 0.9),
}


def material(name):
    found = bpy.data.materials.get(name)
    if found:
        return found
    created = bpy.data.materials.new(name)
    created.diffuse_color = (*PREVIEW[name], 1.0)
    return created


def update():
    bpy.context.view_layer.update()


def origin(obj):
    return obj.matrix_world.translation.copy()


def transform(obj, matrix, about):
    """Applies a 3×3 `matrix` (turn or scale) to an object about a world point; parts that hang
    from it follow."""
    pivot = Matrix.Translation(about) @ matrix.to_4x4() @ Matrix.Translation(-about)
    obj.matrix_world = pivot @ obj.matrix_world
    update()


def move(obj, offset):
    obj.matrix_world = Matrix.Translation(offset) @ obj.matrix_world
    update()


def world_vertices(obj):
    return [obj.matrix_world @ v.co for v in obj.data.vertices]


def bounds(obj):
    points = world_vertices(obj)
    low = Vector((min(p.x for p in points), min(p.y for p in points), min(p.z for p in points)))
    high = Vector((max(p.x for p in points), max(p.y for p in points), max(p.z for p in points)))
    return low, high


def middle(a, b):
    return (a + b) / 2.0


def two_bone(start, end, first, second, pole):
    """The middle joint of a limb from `start` to `end` with segments `first` and `second`,
    bent towards `pole`."""
    span = end - start
    reach = min(span.length, (first + second) * 0.999)
    along = span.normalized()
    distance = (first * first - second * second + reach * reach) / (2.0 * reach)
    out = math.sqrt(max(first * first - distance * distance, 0.0))
    bend = (pole - along * pole.dot(along)).normalized()
    return start + along * distance + bend * out


def outward(face, inside):
    """The face wound counter-clockwise seen from outside, away from the point `inside`."""
    normal = (face[1] - face[0]).cross(face[2] - face[0])
    centre = sum(face, Vector()) / len(face)
    return face if normal.dot(centre - inside) >= 0.0 else list(reversed(face))


class Faces:
    """Faces with a material name each, made into an object at the end."""

    def __init__(self):
        self.faces = []

    def add(self, points, material_name, inside=None):
        points = [Vector(p) for p in points]
        self.faces.append((outward(points, inside) if inside is not None else points,
                           material_name))

    def tube(self, start, end, radius, material_name, sides=10, caps=True):
        start, end = Vector(start), Vector(end)
        axis = (end - start).normalized()
        helper = Vector((0, 0, 1)) if abs(axis.z) < 0.9 else Vector((1, 0, 0))
        side = axis.cross(helper).normalized()
        across = axis.cross(side)

        def ring(centre):
            return [centre + radius * (math.cos(2 * math.pi * k / sides) * side
                                       + math.sin(2 * math.pi * k / sides) * across)
                    for k in range(sides)]

        a, b = ring(start), ring(end)
        inside = middle(start, end)
        for k in range(sides):
            j = (k + 1) % sides
            self.add([a[k], a[j], b[j], b[k]], material_name, inside)
        if caps:
            self.add(a, material_name, inside)
            self.add(b, material_name, inside)

    def sweep(self, points, sizes, material_name, sides=10, power=2.0, lateral=None,
              caps=True):
        """A tube along `points` with a section at each that is `sizes[k]` = (across, deep):
        across `lateral` (the bike's x axis unless given) and deep at right angles to it and
        the path. `power` above 2 squares the ellipse off, as carbon tubes are."""
        lateral = Vector((1.0, 0.0, 0.0)) if lateral is None else Vector(lateral)
        points = [Vector(p) for p in points]
        last = len(points) - 1
        exponent = 2.0 / power
        rings = []
        for k, point in enumerate(points):
            tangent = (points[min(k + 1, last)] - points[max(k - 1, 0)]).normalized()
            across = (lateral - tangent * lateral.dot(tangent)).normalized()
            deep = tangent.cross(across)
            width, depth = sizes[k]
            ring = []
            for j in range(sides):
                a = 2 * math.pi * j / sides
                c, s = math.cos(a), math.sin(a)
                ring.append(point + across * (width / 2 * math.copysign(abs(c) ** exponent, c))
                            + deep * (depth / 2 * math.copysign(abs(s) ** exponent, s)))
            rings.append(ring)
        for k in range(last):
            inside = middle(points[k], points[k + 1])
            for j in range(sides):
                i = (j + 1) % sides
                self.add([rings[k][j], rings[k][i], rings[k + 1][i], rings[k + 1][j]],
                         material_name, inside)
        if caps:
            self.add(rings[0], material_name, points[0] + (points[1] - points[0]) * 0.5)
            self.add(rings[-1], material_name, points[-1] + (points[-2] - points[-1]) * 0.5)

    def prism(self, bottom, top, material_name):
        """A solid between two outlines with the same number of corners."""
        inside = (sum(bottom, Vector()) + sum(top, Vector())) / (2 * len(bottom))
        count = len(bottom)
        for k in range(count):
            j = (k + 1) % count
            self.add([bottom[k], bottom[j], top[j], top[k]], material_name, inside)
        self.add(bottom, material_name, inside)
        self.add(top, material_name, inside)

    def box(self, centre, size, material_name):
        hx, hy, hz = (s / 2.0 for s in size)
        outline = [Vector((-hx, -hy, 0)), Vector((hx, -hy, 0)), Vector((hx, hy, 0)),
                   Vector((-hx, hy, 0))]
        self.prism([centre + p - Vector((0, 0, hz)) for p in outline],
                   [centre + p + Vector((0, 0, hz)) for p in outline], material_name)

    def to_object(self, name, at):
        """An object with its origin at `at` holding the faces."""
        mesh = bpy.data.meshes.new(name)
        vertices, indices, names = [], [], []
        for points, material_name in self.faces:
            indices.append(list(range(len(vertices), len(vertices) + len(points))))
            vertices.extend(p - at for p in points)
            if material_name not in names:
                names.append(material_name)
        mesh.from_pydata([tuple(v) for v in vertices], [], indices)
        for n in names:
            mesh.materials.append(material(n))
        for polygon, (_, material_name) in zip(mesh.polygons, self.faces):
            polygon.material_index = names.index(material_name)
        shared = bmesh.new()
        shared.from_mesh(mesh)
        bmesh.ops.remove_doubles(shared, verts=shared.verts, dist=1e-5)
        shared.to_mesh(mesh)
        shared.free()
        smooth(mesh)
        obj = bpy.data.objects.new(name, mesh)
        obj.matrix_world = Matrix.Translation(at)
        bpy.context.scene.collection.objects.link(obj)
        return obj


def smooth(mesh):
    """Smooth shading, hard only where faces meet at a sharp angle (ADR 0020)."""
    mesh.shade_smooth()
    mesh.set_sharp_from_angle(angle=math.radians(40))


# --- the body --------------------------------------------------------------------------------


# How sharply the sunglasses' shield wraps back round the face (metres per metre squared).
GLASSES_WRAP = 3.0
# Hair and helmet: how far they stand off the scalp, and where the hair ends (metres from the
# eyes' height: at the back, at the sides).
HAIR = {"female": {"thickness": 0.014, "back": -0.085, "sides": -0.035},
        "male": {"thickness": 0.006, "back": -0.05, "sides": -0.005}}


def scalp(head, keep, rim=None):
    """The head's faces whose centre `keep` accepts, in world coordinates: points, their
    normals and the faces as lists of point indices. `rim(point)` gives the height the open
    edge is evened out to."""
    mesh = bmesh.new()
    mesh.from_mesh(head.data)
    mesh.transform(head.matrix_world)
    mesh.normal_update()
    chosen = [f for f in mesh.faces if keep(f.calc_center_median())]
    used = sorted({v.index for f in chosen for v in f.verts})
    mesh.verts.ensure_lookup_table()
    index = {v: k for k, v in enumerate(used)}
    points = [mesh.verts[v].co.copy() for v in used]
    normals = [mesh.verts[v].normal.copy() for v in used]
    faces = [[index[v.index] for v in f.verts] for f in chosen]
    mesh.free()
    if rim is not None:
        for k in edge_points(faces):
            points[k].z = rim(points[k])
    return points, normals, faces


def edge_points(faces):
    """The points on the open edge of a patch of faces."""
    count = {}
    for face in faces:
        for a, b in zip(face, face[1:] + face[:1]):
            key = (min(a, b), max(a, b))
            count[key] = count.get(key, 0) + 1
    return {k for (a, b), n in count.items() if n == 1 for k in (a, b)}


def shell(gear, patch, inner, outer, material_name, shape=None):
    """A solid shell over a patch of the scalp, from `inner` to `outer` metres off it (`outer`
    may depend on the scalp's point), closed at its edge; `shape(point)` reshapes the outer
    surface."""
    points, normals, faces = patch
    thickness = outer if callable(outer) else (lambda point: outer)
    low = [p + n * inner for p, n in zip(points, normals)]
    high = [p + n * thickness(p) for p, n in zip(points, normals)]
    if shape is not None:
        high = [shape(p) for p in high]
    count = {}
    for face in faces:
        middle_point = sum((points[k] for k in face), Vector()) / len(face)
        gear.add([high[k] for k in face], material_name, middle_point)
        gear.add([low[k] for k in reversed(face)], material_name)
        for a, b in zip(face, face[1:] + face[:1]):
            key = (min(a, b), max(a, b))
            count[key] = count.get(key, 0) + 1
    for face in faces:
        middle_point = sum((points[k] for k in face), Vector()) / len(face)
        for a, b in zip(face, face[1:] + face[:1]):
            if count[(min(a, b), max(a, b))] == 1:
                gear.add([low[a], low[b], high[b], high[a]], material_name, middle_point)


def tube_along(gear, spine, radii, material_name, sides=10):
    """A faceted tube along a spine of points with a radius at each, closed at both ends (a
    radius of 0 ends in a point)."""
    rings = []
    for k, (point, radius) in enumerate(zip(spine, radii)):
        ahead = spine[min(k + 1, len(spine) - 1)] - spine[max(k - 1, 0)]
        axis = ahead.normalized()
        side = axis.cross(Vector((0, 0, 1))).normalized() if abs(axis.z) < 0.95 else Vector((1, 0, 0))
        across = axis.cross(side)
        # Every other ring turned half a step: facets, not stripes.
        turn = math.pi / sides * (k % 2)
        rings.append([point + radius * (math.cos(2 * math.pi * j / sides + turn) * side
                                        + math.sin(2 * math.pi * j / sides + turn) * across)
                      for j in range(sides)])
    for k, (a, b) in enumerate(zip(rings, rings[1:])):
        inside = middle(spine[k], spine[k + 1])
        for j in range(sides):
            n = (j + 1) % sides
            gear.add([a[j], a[n], b[n], b[j]], material_name, inside)
    gear.add(rings[0], material_name, spine[1])
    gear.add(rings[-1], material_name, spine[-2])


def headgear(parts, sex):
    """Hair, helmet and sunglasses on the upright head facing +y, fitted to its shape and
    parented to it."""
    head = parts["head"]
    low, high = bounds(head)
    centre = middle(low, high)
    half_y = (high.y - low.y) / 2.0
    eyes_l, eyes_r = origin(parts["eye.L"]), origin(parts["eye.R"])
    eyes = middle(eyes_l, eyes_r).z
    hair = HAIR[sex]

    def ahead(point):
        """Front (+1) to back (−1) of the head."""
        return max(-1.0, min(1.0, (point.y - centre.y) / half_y))

    def face(point):
        return ahead(point) > 0.25 and point.z < eyes + 0.065

    def hairline(point):
        back = max(-ahead(point), 0.0)
        return eyes + hair["sides"] + (hair["back"] - hair["sides"]) * back

    def helmet_rim(point):
        return eyes + helmet.rim_height(ahead(point))

    def hair_thickness(point):
        # Full where it shows below the helmet, flat under it.
        return hair["thickness"] if point.z < helmet_rim(point) + 0.01 else helmet.GAP * 0.5

    gear = Faces()
    shell(gear, scalp(head, lambda c: c.z > hairline(c) and not face(c)),
          0.0, hair_thickness, "hair")

    before = len(gear.faces)
    helmet.road_helmet(gear, parts["skull"], eyes, centre, half_y)
    # The fitted shell also provides the strap anchors, just above its rim.
    worn = [points for points, _ in gear.faces[before:]]
    corners = [p for face in worn for p in face]
    polygons, start = [], 0
    for face in worn:
        polygons.append(list(range(start, start + len(face))))
        start += len(face)
    top = BVHTree.FromPolygons(corners, polygons)
    # Sunglasses: two lenses on a smooth shield wrapped round the face in front of the eyes, a
    # bridge over the nose and arms back over the ears.
    surface = [head] + [parts[p] for p in ("nose", "nose_bridge")]
    points, polygons = [], []
    for obj in surface:
        base = len(points)
        points.extend(world_vertices(obj))
        polygons.extend([[base + v for v in p.vertices] for p in obj.data.polygons])
    tree = BVHTree.FromPolygons(points, polygons)

    bridge = tree.ray_cast(Vector((centre.x, centre.y + 0.5, eyes)), Vector((0, -1, 0)))[0]

    def on_face(x, z, stand_off=0.008):
        hit = tree.ray_cast(Vector((x, centre.y + 0.5, z)), Vector((0, -1, 0)))[0]
        shield = bridge.y + 0.006 - GLASSES_WRAP * (x - centre.x) ** 2
        y = max(shield, hit.y + stand_off) if hit is not None else shield
        return Vector((x, y, z))

    # One wrapped piece over both eyes: a straight top rising a little to the temples, deep
    # lenses with rounded outer corners, and a notch over the nose between them.
    def lens_bottom(across):
        if across < 0.012:
            return eyes - 0.004 - 0.012 * across / 0.012
        if across < 0.026:
            return eyes - 0.016 - 0.018 * (across - 0.012) / 0.014
        if across < 0.05:
            return eyes - 0.034
        return eyes - 0.034 + 0.012 * ((across - 0.05) / 0.016) ** 2

    def lens_top(across):
        return eyes + 0.016 + 0.006 * (across / 0.066) ** 2

    offsets = [-0.066, -0.058, -0.05, -0.038, -0.026, -0.012, 0.0, 0.012, 0.026, 0.038, 0.05,
               0.058, 0.066]
    rows = 4
    grid = [[on_face(centre.x + dx, lens_bottom(abs(dx))
                     + (lens_top(abs(dx)) - lens_bottom(abs(dx))) * r / (rows - 1))
             for dx in offsets] for r in range(rows)]
    inside = Vector((centre.x, centre.y, eyes))
    for r in range(rows - 1):
        for c in range(len(offsets) - 1):
            gear.add([grid[r][c], grid[r][c + 1], grid[r + 1][c + 1], grid[r + 1][c]],
                     "glasses", inside)
    # Arms from the top outer corners back over the ears.
    for corner, name in ((grid[-1][0], "ear.L"), (grid[-1][-1], "ear.R")):
        ear = origin(parts[name]) + Vector((0, 0, 0.02))
        out = 1.0 if ear.x > centre.x else -1.0
        temple = Vector((ear.x + out * 0.006, middle(corner, ear).y, eyes + 0.012))
        hit = tree.ray_cast(temple + Vector((out * 0.3, 0, 0)), Vector((-out, 0, 0)))[0]
        if hit is not None:
            temple = hit + Vector((out * 0.006, 0, 0))
        for start, end in ((corner - Vector((0, 0, 0.004)), temple), (temple, ear)):
            gear.tube(start, end, 0.0035, "glasses", sides=4)

    # Chin straps: from the rim in front of and behind each ear, meeting below the ear, then
    # along the jaw to under the chin, laid on the skin.
    skin = parts["skull_tree"]
    front_points = [p for p in world_vertices(head) if p.y > centre.y]
    chin = min(front_points, key=lambda p: p.z)
    under_chin = Vector((centre.x, chin.y - 0.02, chin.z + 0.004))

    def strap(corners):
        spine = []
        for start, end in zip(corners, corners[1:]):
            for k in range(6):
                point = start + (end - start) * (k / 6)
                nearest = skin.find_nearest(point)
                spine.append(nearest[0] + nearest[1] * 0.004 if nearest[0] is not None
                             else point)
        spine.append(corners[-1])
        gear.sweep(spine, [(0.007, 0.007)] * len(spine), "vents", sides=6)

    for name in ("ear.L", "ear.R"):
        ear = origin(parts[name])
        out = 1.0 if ear.x > centre.x else -1.0
        joint = Vector((ear.x - out * 0.008, ear.y + 0.02, ear.z - 0.04))
        jaw = Vector((ear.x - out * 0.02, ear.y + 0.04, ear.z - 0.075))
        for ahead_of_ear in (0.03, -0.025):
            anchor = Vector((ear.x + out * 0.3, ear.y + ahead_of_ear, 0.0))
            anchor.z = helmet_rim(anchor) + 0.006
            hit = top.ray_cast(anchor, Vector((-out, 0, 0)))[0]
            if hit is not None:
                strap([hit - Vector((0, 0, 0.004)), joint])
        strap([joint, jaw, under_chin])

    if sex == "female":
        # A ponytail from under the back of the helmet, with a hair tie in the kit's colour.
        nape = tree.ray_cast(Vector((centre.x, centre.y - 0.5, eyes - 0.005)),
                             Vector((0, 1, 0)))[0]
        spine = [nape + Vector(offset) for offset in (
            (0, 0.01, 0), (0, -0.03, -0.012), (0, -0.065, -0.05), (0, -0.085, -0.105),
            (0, -0.088, -0.16), (0, -0.08, -0.21), (0, -0.07, -0.25))]
        tube_along(gear, spine, (0.022, 0.026, 0.03, 0.028, 0.022, 0.014, 0.0), "hair")
        tie = spine[0] + (spine[1] - spine[0]) * 0.55
        gear.tube(tie - (spine[1] - spine[0]).normalized() * 0.007,
                  tie + (spine[1] - spine[0]).normalized() * 0.007, 0.027, "sleeve")

    obj = gear.to_object("headgear", origin(head))
    obj.parent = head
    obj.matrix_parent_inverse = head.matrix_world.inverted()
    for part in [p for p in parts if p.startswith(EYES)]:
        bpy.data.objects.remove(parts.pop(part))
    update()


# --- the bike --------------------------------------------------------------------------------


def disc(faces, hub, radius, x, thickness, material_name, count=32, teeth=0.0):
    """A thin disc round the wheel's axis (rotor, cog, chainring, pulley), with `teeth` deep
    teeth on every other corner where set."""
    outline = []
    for k in range(count):
        r = radius - (teeth if k % 2 else 0.0)
        a = 2 * math.pi * k / count
        outline.append(Vector((0.0, r * math.cos(a), r * math.sin(a))))
    faces.prism([hub + p + Vector((x - thickness / 2, 0, 0)) for p in outline],
                [hub + p + Vector((x + thickness / 2, 0, 0)) for p in outline], material_name)


def annulus(faces, hub, outer, inner, x, thickness, material_name, count=32, teeth=0.0,
            slots=False):
    """A flat ring round the wheel's axis (chainring, brake rotor), with `teeth` deep teeth on
    every other corner of its outside where set."""
    def at(radius, k, side):
        a = 2 * math.pi * k / count
        return hub + Vector((x + side * thickness / 2, radius * math.cos(a), radius * math.sin(a)))

    for k in range(count):
        if slots and k % 4 == 2:
            continue
        j = (k + 1) % count
        r0 = outer - (teeth if k % 2 else 0.0)
        r1 = outer - (teeth if j % 2 else 0.0)
        a = 2 * math.pi * (k + 0.5) / count
        inside = hub + Vector((x, (outer + inner) / 2 * math.cos(a), (outer + inner) / 2 * math.sin(a)))
        for side in (-1.0, 1.0):
            faces.add([at(r0, k, side), at(r1, j, side), at(inner, j, side), at(inner, k, side)],
                      material_name, inside)
        faces.add([at(r0, k, -1.0), at(r1, j, -1.0), at(r1, j, 1.0), at(r0, k, 1.0)],
                  material_name, inside)
        faces.add([at(inner, k, -1.0), at(inner, j, -1.0), at(inner, j, 1.0), at(inner, k, 1.0)],
                  material_name, inside)
        if slots:
            for n, r in ((k, r0), (j, r1)):
                if n % 4 != (3 if n == k else 2):
                    continue
                faces.add([at(inner, n, -1.0), at(r, n, -1.0), at(r, n, 1.0),
                           at(inner, n, 1.0)], material_name, inside)


def path(points, steps=4):
    """A smooth path through `points` (Catmull–Rom), `steps` points per span."""
    points = [Vector(p) for p in points]
    out = []
    for k in range(len(points) - 1):
        p0 = points[max(k - 1, 0)]
        p1, p2 = points[k], points[k + 1]
        p3 = points[min(k + 2, len(points) - 1)]
        for i in range(steps):
            t = i / steps
            out.append(0.5 * ((2 * p1) + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t * t
                              + (-p0 + 3 * p1 - 3 * p2 + p3) * t * t * t))
    out.append(points[-1])
    return out


def taper(count, start, end):
    """`count` sizes going evenly from `start` to `end`."""
    return [tuple(a + (b - a) * k / max(count - 1, 1) for a, b in zip(start, end))
            for k in range(count)]


def wheel(name, hub, cassette=False):
    """A wheel in the y–z plane: a round tyre on a deep-section rim, flanged hub, 24 spokes, a
    disc rotor on the left and, at the back, the cassette on the right — all turning with it."""
    faces = Faces()
    segments = 64
    # Separate casing and tread preserve the tyre's rounded silhouette in oblique views.
    tube_radius = 0.014
    centre = WHEEL_RADIUS - tube_radius
    sides = 8
    for k in range(segments):
        a0, a1 = 2 * math.pi * k / segments, 2 * math.pi * (k + 1) / segments
        for j in range(sides):
            b0, b1 = 2 * math.pi * j / sides, 2 * math.pi * (j + 1) / sides

            def at(a, b):
                r = centre + tube_radius * math.cos(b)
                return hub + Vector((tube_radius * math.sin(b), r * math.cos(a), r * math.sin(a)))

            inside = hub + Vector((0.0, centre * math.cos((a0 + a1) / 2),
                                   centre * math.sin((a0 + a1) / 2)))
            kind = "sidewall" if j in (1, 2, 5, 6) else "tyre"
            faces.add([at(a0, b0), at(a1, b0), at(a1, b1), at(a0, b1)], kind, inside)
    # A deep carbon rim (about 55 mm), widest a little below the tyre and rounding in to the
    # spoke bed, as aero rims are.
    profile = [(WHEEL_RADIUS - 0.024, 0.0125), (WHEEL_RADIUS - 0.04, 0.0145),
               (WHEEL_RADIUS - 0.062, 0.0125), (WHEEL_RADIUS - 0.076, 0.0075)]
    inner = profile[-1][0]
    for k in range(segments):
        a0, a1 = 2 * math.pi * k / segments, 2 * math.pi * (k + 1) / segments

        def rim(radius, a, x):
            return hub + Vector((x, radius * math.cos(a), radius * math.sin(a)))

        inside = rim((profile[0][0] + inner) / 2, (a0 + a1) / 2, 0.0)
        for (r_out, w_out), (r_in, w_in) in zip(profile, profile[1:]):
            for side in (-1.0, 1.0):
                faces.add([rim(r_out, a0, side * w_out), rim(r_out, a1, side * w_out),
                           rim(r_in, a1, side * w_in), rim(r_in, a0, side * w_in)], "rim", inside)
        w = profile[-1][1]
        faces.add([rim(inner, a0, -w), rim(inner, a1, -w), rim(inner, a1, w), rim(inner, a0, w)],
                  "rim", inside)
        w = profile[0][1]
        faces.add([rim(profile[0][0], a0, -w), rim(profile[0][0], a1, -w),
                   rim(profile[0][0], a1, w), rim(profile[0][0], a0, w)], "rim", inside)
    # Two-cross lacing: neighbouring spokes on each flange lead and trail the hub.
    for k in range(24):
        angle = 2 * math.pi * k / 24
        flange = 0.025 if k % 2 else -0.025
        lacing = angle + (1 if (k // 2) % 2 else -1) * math.pi / 3
        start = hub + Vector((flange, 0.025 * math.cos(lacing), 0.025 * math.sin(lacing)))
        end = hub + Vector((0.0, inner * math.cos(angle), inner * math.sin(angle)))
        faces.tube(start, end, 0.001, "alloy", sides=4, caps=False)
    faces.sweep([hub - Vector((0.05, 0, 0)), hub - Vector((0.025, 0, 0)),
                 hub + Vector((0.025, 0, 0)), hub + Vector((0.05, 0, 0))],
                [(0.024, 0.024), (0.034, 0.034), (0.034, 0.034), (0.024, 0.024)], "alloy",
                sides=8, lateral=(0.0, 0.0, 1.0))
    for x in (-0.026, 0.026):
        annulus(faces, hub, 0.027, 0.012, x, 0.003, "alloy", count=16)
    # Valve stems sit inside the spoke bed, so they turn with the wheel.
    valve = Vector((0.0, math.cos(0.4), math.sin(0.4)))
    faces.tube(hub + valve * (inner - 0.026), hub + valve * inner, 0.0025, "metal", sides=6)
    # The brake rotor: a ring on a five-armed carrier, on the left.
    annulus(faces, hub, 0.08, 0.075, -0.042, 0.0018, "metal", count=48)
    annulus(faces, hub, 0.075, 0.063, -0.042, 0.0018, "metal", count=48, slots=True)
    for k in range(5):
        a = 2 * math.pi * k / 5
        faces.tube(hub + Vector((-0.042, 0.0, 0.0)),
                   hub + Vector((-0.042, 0.064 * math.cos(a), 0.064 * math.sin(a))), 0.004,
                   "alloy", sides=4)
    if cassette:
        for k in range(11):
            radius = 0.058 - k * 0.0032
            annulus(faces, hub, radius, max(0.012, radius - 0.012), 0.03 + k * 0.0038,
                    0.0018, "metal", count=24, teeth=0.003)
    obj = faces.to_object(name, hub)
    # The tyre's eight-sided section is rubber, with no hard crease between its facets.
    obj.data.set_sharp_from_angle(angle=math.radians(65))
    return obj


def chain(faces, sprockets, x):
    """A closed chain on external tangents, with separate side plates at bicycle-chain pitch.

    Keeping its path in the frame mesh avoids adding animated objects for individual links.
    """
    sprockets = [(Vector((0.0, centre.y, centre.z)), radius) for centre, radius in sprockets]
    normals = []
    for i, (centre, radius) in enumerate(sprockets):
        next_centre, next_radius = sprockets[(i + 1) % len(sprockets)]
        span = next_centre - centre
        along = span.normalized()
        out = Vector((0.0, along.z, -along.y))
        difference = (radius - next_radius) / span.length
        normals.append(along * difference + out * math.sqrt(1.0 - difference ** 2))
    points = []
    for i, (centre, radius) in enumerate(sprockets):
        incoming, outgoing = normals[i - 1], normals[i]
        angle = math.atan2(incoming.z, incoming.y)
        arc = (math.atan2(outgoing.z, outgoing.y) - angle) % (2 * math.pi)
        steps = max(2, math.ceil(arc * radius / 0.006))
        for k in range(steps + 1):
            a = angle + arc * k / steps
            points.append(Vector((x, centre.y + radius * math.cos(a),
                                  centre.z + radius * math.sin(a))))
    points.append(points[0])
    # A narrow roller belt keeps the route continuous; spaced plates catch the light.
    faces.sweep(points, [(0.004, 0.002)] * len(points), "alloy", sides=4, caps=False)
    lengths = [(b - a).length for a, b in zip(points, points[1:])]
    total = sum(lengths)
    links = round(total / 0.0127)
    segment = 0
    walked = 0.0
    for k in range(links):
        distance = total * k / links
        while segment < len(lengths) - 1 and walked + lengths[segment] < distance:
            walked += lengths[segment]
            segment += 1
        along = (points[segment + 1] - points[segment]).normalized()
        centre = points[segment] + along * (distance - walked)
        deep = along.cross(Vector((1.0, 0.0, 0.0))) * 0.002
        for side in (-1.0, 1.0):
            middle_plate = centre + Vector((side * 0.0025, 0.0, 0.0))
            outline = [middle_plate + along * a + deep * b
                       for a, b in ((-0.0048, -1), (0.0048, -1), (0.0048, 1), (-0.0048, 1))]
            faces.add(outline, "metal", centre)


def bottle_and_cage(faces, bottom, top):
    """A shaped bidon on the down tube, with an open cage attached at two bosses."""
    axis = (top - bottom).normalized()
    inward = Vector((0.0, -axis.z, axis.y))
    base = bottom + (top - bottom) * 0.18 + inward * 0.062
    heights = [0.0, 0.008, 0.105, 0.118, 0.133, 0.146, 0.16, 0.174]
    widths = [0.058, 0.069, 0.069, 0.062, 0.062, 0.068, 0.055, 0.042]
    faces.sweep([base + axis * h for h in heights], [(w, w) for w in widths],
                "bottle", sides=16)
    faces.sweep([base + axis * h for h in (0.174, 0.18, 0.185)],
                [(0.044, 0.044), (0.044, 0.044), (0.037, 0.037)], "bar", sides=12)
    faces.tube(base + axis * 0.185, base + axis * 0.198, 0.009, "bar", sides=8)
    for h in (0.025, 0.089):
        faces.tube(base + axis * h - inward * 0.035,
                   base + axis * h - inward * 0.062, 0.005, "alloy", sides=6)
    for sign in (-1.0, 1.0):
        rail = path([base - inward * 0.034 + Vector((sign * 0.013, 0, 0)),
                     base + inward * 0.023 + Vector((sign * 0.028, 0, 0)),
                     base + axis * 0.045 + inward * 0.028 + Vector((sign * 0.027, 0, 0)),
                     base + axis * 0.102 + Vector((sign * 0.037, 0, 0)),
                     base + axis * 0.12 - inward * 0.031 + Vector((sign * 0.017, 0, 0))], 2)
        faces.sweep(rail, [(0.006, 0.008)] * len(rail), "carbon", sides=6)


def finished_frame(faces):
    """Blend painted tube junctions as a moulded frame; keep small hardware out of the remesh.

    A bounded decimation keeps the welded surface near the original tube budget.
    """
    painted, hardware = Faces(), Faces()
    for points, name in faces.faces:
        (painted if name == "frame" else hardware).add(points, name)
    frame = painted.to_object("frame", Vector())
    bpy.ops.object.select_all(action="DESELECT")
    frame.select_set(True)
    bpy.context.view_layer.objects.active = frame
    weld = frame.modifiers.new("Moulded junctions", "REMESH")
    weld.mode = "VOXEL"
    weld.voxel_size = 0.003
    weld.use_smooth_shade = True
    bpy.ops.object.modifier_apply(modifier=weld.name)
    rounding = frame.modifiers.new("Round junctions", "SMOOTH")
    rounding.factor = 0.6
    rounding.iterations = 4
    bpy.ops.object.modifier_apply(modifier=rounding.name)
    simplify = frame.modifiers.new("Frame budget", "DECIMATE")
    simplify.ratio = min(1.0, 1800 / triangles([frame]))
    bpy.ops.object.modifier_apply(modifier=simplify.name)
    smooth(frame.data)
    detail = hardware.to_object("hardware", Vector())
    detail.select_set(True)
    bpy.ops.object.join()
    frame.data.set_sharp_from_angle(angle=math.radians(65))
    return frame


def bike(saddle, hoods):
    """Frame, wheels, crankset and pedals around the rider's contact points: the saddle's top
    middle and the hoods (both in the y–z plane). Returns the objects and the hubs."""
    seat_axis = Vector((0.0, -math.cos(SEAT_ANGLE), math.sin(SEAT_ANGLE)))
    steer = Vector((0.0, math.cos(HEAD_ANGLE), -math.sin(HEAD_ANGLE)))  # down the steerer
    frame = Faces()

    # The bar tops stay level under the fitted hoods, independently of the stem's slope.
    hood_y = hoods.y + HOOD_AHEAD_OF_PALM
    # A road bar's reach: its tops' line to the hood's horn about 12.5 cm.
    bar = Vector((0.0, hood_y - 0.06, hoods.z - 0.03))
    steerer_top = bar + Vector((0.0, -STEM, 0.0))
    head_top = steerer_top + steer * 0.03
    crown_z = WHEEL_RADIUS + FORK_LENGTH * math.sin(HEAD_ANGLE)
    # A low handlebar fit must drop the stem, not shorten the fork into the tyre.
    minimum_top = crown_z + 0.08 * math.sin(HEAD_ANGLE)
    head_top -= steer * max((minimum_top - head_top.z) / math.sin(HEAD_ANGLE), 0.0)
    steerer_top = head_top - steer * 0.03
    head_length = (head_top.z - crown_z) / math.sin(HEAD_ANGLE)
    head_bottom = head_top + steer * head_length
    # The steering axis meets hub height; the hub sits the rake ahead of it.
    to_hub = (head_bottom.z - WHEEL_RADIUS) / math.sin(HEAD_ANGLE)
    front_hub = head_bottom + steer * to_hub + Vector((0.0, FORK_RAKE / math.sin(HEAD_ANGLE), 0.0))
    front_hub.z = WHEEL_RADIUS
    rear_hub = Vector((0.0, -math.sqrt(CHAINSTAY ** 2 - (WHEEL_RADIUS - BOTTOM_BRACKET.z) ** 2),
                       WHEEL_RADIUS))

    side = Vector((1.0, 0.0, 0.0))
    # A compact race frame: the top tube slopes down to the seat tube, which ends well below
    # the saddle; the seat stays drop to meet it lower still.
    cluster_z = min(head_top.z - 0.07, saddle.z - 0.2)
    cluster = BOTTOM_BRACKET + seat_axis * ((cluster_z - BOTTOM_BRACKET.z) / seat_axis.z)
    stays_top = cluster - seat_axis * 0.07
    # The seat post carries on the seat tube's line up to the saddle's clamp.
    post_top = BOTTOM_BRACKET + seat_axis * ((saddle.z - 0.055 - BOTTOM_BRACKET.z) / seat_axis.z)

    def line(start, end, count=5):
        return [start + (end - start) * k / (count - 1) for k in range(count)]

    # Head tube: tall and deep, flaring at the bottom into the crown.
    frame.sweep(line(head_top - steer * 0.012, head_bottom + steer * 0.01),
                taper(5, (0.038, 0.052), (0.046, 0.062)), "frame", power=2.4)
    # Down tube: the biggest tube, flattened where it meets the bottom bracket.
    frame.sweep(line(BOTTOM_BRACKET + Vector((0, 0.015, 0.01)), head_bottom - steer * 0.03, 6),
                [(0.062, 0.048), (0.056, 0.054), (0.05, 0.056), (0.048, 0.056), (0.046, 0.054),
                 (0.044, 0.05)], "frame", power=2.6)
    # Top tube, slimming towards the seat tube.
    frame.sweep(line(head_top + steer * 0.03, cluster + seat_axis * 0.012),
                taper(5, (0.038, 0.04), (0.026, 0.032)), "frame", power=2.4)
    # Seat tube: deep, aero, from the shell to just above the top tube.
    frame.sweep(line(BOTTOM_BRACKET, cluster + seat_axis * 0.025),
                taper(5, (0.046, 0.05), (0.032, 0.046)), "frame", power=2.4)
    bottle_and_cage(frame, BOTTOM_BRACKET, head_bottom)
    # The bottom bracket shell.
    frame.sweep(line(BOTTOM_BRACKET - side * 0.043, BOTTOM_BRACKET + side * 0.043, 3),
                [(0.05, 0.05)] * 3, "frame", lateral=(0.0, 0.0, 1.0))
    for sign in (-1.0, 1.0):
        out = side * sign
        # Chainstays, bowing out round the tyre; seat stays, thin and dropped.
        chainstay = path([BOTTOM_BRACKET + out * 0.035,
                          BOTTOM_BRACKET + out * 0.05 + (rear_hub - BOTTOM_BRACKET) * 0.3,
                          rear_hub + out * 0.062 + (BOTTOM_BRACKET - rear_hub) * 0.08,
                          rear_hub + out * 0.064], 3)
        frame.sweep(chainstay, taper(len(chainstay), (0.024, 0.034), (0.012, 0.018)), "frame",
                    sides=8, power=2.4)
        stay = path([stays_top + out * 0.012,
                     stays_top + out * 0.03 + (rear_hub - stays_top) * 0.35,
                     rear_hub + out * 0.064], 3)
        frame.sweep(stay, taper(len(stay), (0.018, 0.024), (0.012, 0.014)), "frame",
                    sides=8, power=2.4)
        # Fork blades: from the crown down the steering axis, curving forward to the hub.
        crown = head_bottom + steer * 0.015 + out * 0.032
        tip = front_hub + out * 0.056
        blade = path([crown, middle(crown, tip) + out * 0.006 + Vector((0.0, 0.006, 0.0)),
                      tip], 3)
        frame.sweep(blade, taper(len(blade), (0.03, 0.05), (0.016, 0.022)), "frame",
                    sides=8, power=2.4)
    # The fork crown, across the bottom of the head tube.
    frame.sweep(line(head_bottom + steer * 0.015 - side * 0.04,
                     head_bottom + steer * 0.015 + side * 0.04, 3),
                [(0.05, 0.065)] * 3, "frame", lateral=(0.0, 0.0, 1.0), power=2.6)
    # An aero seat post, black, rising out of the seat tube to the saddle's clamp.
    frame.sweep(line(cluster, post_top, 4), [(0.026, 0.042)] * 4, "carbon", sides=8, power=2.4)

    # The stem meets the fitted bar height; the smaller frame needs a descending stem.
    frame.sweep(line(head_top - steer * 0.012, steerer_top - steer * 0.012, 3),
                [(0.036, 0.046)] * 3, "carbon", sides=8, power=2.4)
    frame.sweep(line(steerer_top - Vector((0.0, 0.012, 0.0)), bar),
                taper(5, (0.036, 0.04), (0.032, 0.03)), "carbon", sides=8, power=2.4)
    for sign in (-1.0, 1.0):
        corner = Vector((sign * HOODS_HALF_WIDTH, hood_y - 0.012, bar.z))
        tops = path([bar, bar + Vector((sign * 0.06, 0.0, 0.0)),
                     bar + Vector((sign * (HOODS_HALF_WIDTH - 0.04), 0.008, 0.0)),
                     corner + Vector((-sign * 0.006, -0.02, 0.0)), corner], 3)
        frame.sweep(tops, taper(len(tops), (0.014, 0.05), (0.024, 0.026)), "carbon", sides=8,
                    lateral=(0.0, 0.0, 1.0), power=2.6)
        drop = path([corner + Vector((0.0, -0.01, 0.0)), corner,
                     corner + Vector((0.0, 0.04, -0.035)), corner + Vector((0.0, 0.045, -0.09)),
                     corner + Vector((0.0, 0.01, -0.128)), corner + Vector((0.0, -0.04, -0.13)),
                     corner + Vector((0.0, -0.085, -0.124))], 3)
        frame.sweep(drop, [(0.024, 0.024)] * len(drop), "bar", sides=8)
        # The brake hood the hand rests on: a rounded body rising to a horn at its front;
        # the lever blade curving down in front of the drop.
        hood = Vector((sign * HOODS_HALF_WIDTH, hood_y, hoods.z))
        body = path([hood + Vector((0, -0.04, -0.03)), hood + Vector((0, -0.012, -0.02)),
                     hood + Vector((0, 0.025, -0.019)), hood + Vector((0, 0.05, -0.012)),
                     hood + Vector((0, 0.064, 0.003))], 3)
        frame.sweep(body, [(0.024, 0.026), (0.026, 0.034), (0.03, 0.04), (0.031, 0.04),
                           (0.03, 0.04), (0.03, 0.04), (0.03, 0.039), (0.029, 0.037),
                           (0.028, 0.034), (0.026, 0.03), (0.022, 0.024), (0.018, 0.018),
                           (0.014, 0.012)], "bar", sides=8, power=2.2)
        # The lever blade comes out of the hood's front, under the horn.
        lever = path([hood + Vector((0, 0.05, -0.012)), hood + Vector((0, 0.062, -0.045)),
                      hood + Vector((0, 0.063, -0.08)), hood + Vector((0, 0.054, -0.11))], 3)
        frame.sweep(lever, taper(len(lever), (0.012, 0.022), (0.009, 0.014)), "bar",
                    sides=8, power=2.4)
        # Bar plugs and a lever pivot separate the rubber grips from the hardware.
        plug = drop[-1]
        frame.tube(plug, plug + Vector((0.0, -0.004, 0.0)), 0.011, "alloy", sides=8)
        pivot = hood + Vector((0, 0.048, -0.021))
        frame.tube(pivot - side * 0.017, pivot + side * 0.017, 0.003, "metal", sides=6)

    # A compact 21 cm shell keeps the broad support under the pelvis and shortens the nose.
    back = saddle.y - 0.09
    shape = [(0.0, 0.118, 0.02, 0.008), (0.018, 0.14, 0.03, 0.005), (0.055, 0.132, 0.034, 0.0),
             (0.105, 0.075, 0.032, 0.0), (0.16, 0.042, 0.028, 0.0), (0.21, 0.03, 0.02, 0.002)]
    centres = [Vector((0.0, back + y, saddle.z - thick / 2 + kick)) for y, _, thick, kick in shape]
    frame.sweep(centres, [(width, thick) for _, width, thick, _ in shape], "saddle",
                sides=16, power=2.6)
    # Its rails, clamped by the seat post's head.
    for sign in (-1.0, 1.0):
        frame.tube(Vector((sign * 0.03, back + 0.03, saddle.z - 0.03)),
                   Vector((sign * 0.02, back + 0.16, saddle.z - 0.03)), 0.0035, "metal", sides=6)
    frame.sweep(line(post_top + Vector((0, 0, -0.01)), post_top + Vector((0, 0, 0.026)), 3),
                [(0.03, 0.05)] * 3, "bar", sides=8, power=2.6)

    # Disc brake calipers on the fork's left blade and the left chainstay.
    frame.box(front_hub + Vector((-0.045, -0.04, 0.05)), (0.022, 0.05, 0.032), "bar")
    frame.box(rear_hub + Vector((-0.045, 0.03, 0.05)), (0.022, 0.05, 0.032), "bar")
    # The rear derailleur under the right dropout, its cage with two pulleys.
    upper_pulley = rear_hub + Vector((0.048, 0.0, -0.065))
    lower_pulley = rear_hub + Vector((0.048, 0.02, -0.12))
    mech = path([rear_hub + Vector((0.065, 0.0, -0.012)),
                 rear_hub + Vector((0.07, -0.01, -0.035)),
                 upper_pulley + Vector((0.012, 0.0, 0.0))], 2)
    frame.sweep(mech, taper(len(mech), (0.024, 0.03), (0.018, 0.022)), "alloy", sides=8)
    # Open cage plates keep the two jockey wheels legible instead of hiding them in a block.
    for x in (-0.006, 0.006):
        cage = path([upper_pulley + Vector((x, -0.018, 0)),
                     lower_pulley + Vector((x, -0.018, 0)),
                     lower_pulley + Vector((x, 0, -0.018)),
                     lower_pulley + Vector((x, 0.018, 0)),
                     upper_pulley + Vector((x, 0.018, 0))], 2)
        frame.sweep(cage, [(0.003, 0.006)] * len(cage), "alloy", sides=4)
    disc(frame, upper_pulley - Vector((0.048, 0, 0)), 0.016, 0.048, 0.006, "alloy", count=12)
    disc(frame, lower_pulley - Vector((0.048, 0, 0)), 0.016, 0.048, 0.006, "alloy", count=12)
    # Pulley centres share the chainline with the selected middle cassette sprocket.
    chain(frame, [(BOTTOM_BRACKET, 0.106), (rear_hub, 0.045),
                  (upper_pulley, 0.016), (lower_pulley, 0.016)], 0.048)

    crankset = Faces()
    bb = BOTTOM_BRACKET
    # Two toothed chainrings (50 and 34), dark, on a four-armed spider.
    annulus(crankset, bb, 0.106, 0.088, 0.048, 0.004, "alloy", count=56, teeth=0.005)
    annulus(crankset, bb, 0.072, 0.058, 0.041, 0.004, "alloy", count=40, teeth=0.005)
    for k in range(4):
        a = math.pi / 4 + k * math.pi / 2
        crankset.sweep([bb + Vector((0.046, 0.0, 0.0)),
                        bb + Vector((0.046, 0.09 * math.cos(a), 0.09 * math.sin(a)))],
                       [(0.006, 0.02), (0.006, 0.012)], "alloy", sides=8, power=2.4)
    crankset.tube(bb + Vector((-0.07, 0, 0)), bb + Vector((0.07, 0, 0)), 0.012, "metal")
    for sign in (-1.0, 1.0):
        # The right crank forward, the left one back: the app turns them from there. Each arm
        # tapers from the spindle to the pedal.
        root = bb + Vector((sign * 0.06, 0, 0))
        end = bb + Vector((sign * (PEDAL_OFFSET - 0.04), sign * CRANK, 0.0))
        crankset.sweep(line(root, end, 4), taper(4, (0.016, 0.034), (0.012, 0.022)), "alloy",
                       sides=10, power=2.6, lateral=(0.0, 0.0, 1.0))
    pedals = []
    for name, sign in (("pedal_l", -1.0), ("pedal_r", 1.0)):
        pedal = Faces()
        axle = bb + Vector((sign * PEDAL_OFFSET, sign * CRANK, 0.0))
        # A road pedal: a flat body wider at its front, on a short axle.
        pedal.prism(
            [axle + Vector((x, y, -0.008)) for x, y in ((-0.03, -0.035), (0.03, -0.035),
                                                         (0.04, 0.03), (-0.04, 0.03))],
            [axle + Vector((x, y, 0.008)) for x, y in ((-0.03, -0.035), (0.03, -0.035),
                                                        (0.04, 0.03), (-0.04, 0.03))], "rim")
        pedal.tube(axle - Vector((sign * 0.05, 0, 0)), axle, 0.005, "metal", sides=6)
        pedals.append(pedal.to_object(name, axle))
    objects = [finished_frame(frame), wheel("wheel_rear", rear_hub, cassette=True),
               wheel("wheel_front", front_hub), crankset.to_object("crankset", bb), *pedals]
    return objects, rear_hub, front_hub


# --- export ----------------------------------------------------------------------------------


def godot(v):
    return [round(v.x, 4), round(v.z, 4), round(-v.y, 4)]


def triangles(objects):
    return sum(len(p.vertices) - 2 for o in objects for p in o.data.polygons)


# --- the rider: a realistic body, rigged and posed on the bike ---------------------------------

REALISTIC_SOURCE = os.path.normpath(
    os.path.join(HERE, "..", "sources", "human-base-meshes", "realistic_bodies.blend"))
# The skeleton the joints come from is this tall; each body's joints are scaled to its height.
SKELETON_HEIGHT = 1.762
SKELETON_X = -7.411
# Heat weighting fails on the detailed bodies: a copy thinned to this share is weighted and its
# weights handed on.
WEIGHT_COPY = 0.25
# The posed body is thinned to this share of its faces for the app (about 35,000 triangles, the hands spared).
REALISTIC_DETAIL = 0.4
# Arm extension on the hoods, shoulder to wrist as a share of the arm: almost straight, as
# riders on the hoods hold them.
REALISTIC_REACH = 0.97
# How far the sit bones, with the skin under them, are below the hip joints' middle (for the
# reference body, 1.762 m tall).
SIT_BONES_BELOW_HIPS = 0.09
# How far the fingers curl about the knuckles (at the hand bone's end) round the hood.
FINGERS = ("thumb", "index", "middle", "ring", "little")
# The hand's vertex groups: the hand and its fingers.
HAND_GROUPS = ("hand.",) + tuple(f"{finger}_" for finger in FINGERS)
# How far each finger joint bends round the hood (from the knuckle out), and the thumb's.
FINGER_CURL = tuple(math.radians(a) for a in (45.0, 50.0, 30.0))
# Where each thumb bone points (inward across the bike, ahead, down): from the palm down to
# the hood's inner flank, then forward along it.
THUMB_AIM = ((0.45, 0.75, 0.5), (0.2, 0.85, 0.5), (0.15, 0.5, 0.85))
# How far from the hand joints' middle the hand's skin reaches (metres), for centring them.
HAND_REACH = 0.09
# Bones within this much of a hand vertex's nearest bone share its weight (metres), falling
# off over `HAND_SOFT`.
HAND_BLEND = 0.02
HAND_SOFT = 0.008
# How much of the base mesh's spread between the fingers is closed for the grip.
FINGER_CLOSE = 0.9
# How far each hand rolls in from palm-down, thumb up on the inside of its hood.
HAND_ROLL = math.radians(40.0)
# Each hand on its hood (armature space): its length forward and a little down, its palm
# down and rolled `HAND_ROLL` in.
HAND_ON_HOODS = {
    side: (Vector((0.0, 0.9, -0.44)),
           Vector((-sign * math.sin(HAND_ROLL), 0.0, -math.cos(HAND_ROLL))))
    for side, sign in (("L", -1.0), ("R", 1.0))
}
# Neck and head, tilted forward from vertical: a rider on the hoods looking ahead.
REALISTIC_NECK = math.radians(45.0)
REALISTIC_HEAD = math.radians(20.0)
# Riders' heights; None keeps the body's own (1.639 m for her).
RIDER_HEIGHT = {"female": None, "male": 1.76}
# The back's angle above horizontal: upright enough to look relaxed at most, flat enough to
# reach hoods far enough ahead at least.
REALISTIC_TORSO = (math.radians(50.0), math.radians(24.0))


def load_realistic(sex):
    """The realistic body standing at the origin facing +y, and its joints in the same frame."""
    with bpy.data.libraries.load(REALISTIC_SOURCE, link=False) as (source, target):
        target.objects = list(source.objects)
    objects = {o.name: o for o in target.objects}
    body = objects[f"GEO-body_{sex}_realistic"]
    bpy.context.scene.collection.objects.link(body)
    update()
    corners = [body.matrix_world @ Vector(c) for c in body.bound_box]
    low = Vector([min(c[i] for c in corners) for i in range(3)])
    high = Vector([max(c[i] for c in corners) for i in range(3)])
    height = high.z - low.z
    grow = (RIDER_HEIGHT[sex] or height) / height
    scale = height * grow / SKELETON_HEIGHT
    # The source faces −y: turn it round so it faces the bike's forward, +y.
    turn = Matrix.Rotation(math.pi, 4, "Z")
    place = (Matrix.Scale(grow, 4) @ turn
             @ Matrix.Translation(Vector((-(low.x + high.x) / 2, 0.0, -low.z))))
    body.data.transform(place @ body.matrix_world)
    body.matrix_world = Matrix.Identity(4)
    joints = {}
    for name, obj in objects.items():
        if name.startswith("GEO-skeletion."):
            # Appended but not linked, an object's world matrix is not computed; the joints
            # have no parents, so their location is where they are.
            p = obj.location
            joints[name.split(".", 1)[1]] = turn @ Vector(
                ((p.x - SKELETON_X) * scale, p.y * scale, p.z * scale))
    return body, joints, scale


def centre_leg_joints(body, joints):
    """Moves the knees, ankles and balls of the feet across to the middle of the leg's own
    section at their height. The bundle has one skeleton, scaled here by height alone: on a
    broader body its legs stand wider than the skeleton's, and posed about joints beside them
    the feet would land outboard of the pedals."""
    points = world_vertices(body)
    for side in ("L", "R"):
        for name in ("leg_patella", "foot_talus", "foot_center"):
            joint = joints[f"{name}.{side}"]
            sign = 1.0 if joint.x > 0 else -1.0
            section = [p.x for p in points
                       if abs(p.z - joint.z) < 0.012 and 0.02 < p.x * sign < 0.3]
            if section:
                joint.x = sum(section) / len(section)


def centre_hand_joints(body, joints):
    """Moves each hand's joints (wrist, knuckles, finger joints) together so they sit in the
    body's own hand: the bundle's one skeleton, scaled by height alone, has its hands beside
    these bodies' hands, and the skin would keep that offset posed, gripping beside the hood."""
    points = world_vertices(body)
    parts = ("hand_center", "metacarpal", "proximal", "middle", "distal")
    for side in ("L", "R"):
        names = [n for n in joints if n.endswith(f".{side}") and n.split(".")[0] in parts]
        cluster = sum((joints[n] for n in names), Vector()) / len(names)
        near = [p for p in points if (p - cluster).length < HAND_REACH]
        if not near:
            continue
        skin = sum(near, Vector()) / len(near)
        shift = Vector((skin.x - cluster.x, skin.y - cluster.y, 0.0))
        for n in names:
            joints[n] = joints[n] + shift


def realistic_skeleton(body, joints, scale):
    """An armature from the joints, the body skinned to it. Bone names are the ones the app
    poses: pelvis, spine, chest, neck, head, shoulder/upper_arm/forearm/hand and
    thigh/shin/foot/toe, each with .L and .R; and three bones a finger (`index_1` … `_3`)
    for gripping the hoods."""
    armature = bpy.data.armatures.new("rig")
    rig = bpy.data.objects.new("rig", armature)
    bpy.context.scene.collection.objects.link(rig)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.mode_set(mode="EDIT")
    bones = armature.edit_bones

    def bone(name, head, tail, parent=None, connect=False):
        made = bones.new(name)
        made.head, made.tail = head, tail
        if parent:
            made.parent = bones[parent]
            made.use_connect = connect

    hip = joints["hip"]
    middle_back = joints["spine_lumbar_l5"].lerp(joints["spine_thoracic_t1"], 0.5)
    bone("pelvis", hip, hip + Vector((0.0, 0.0, 0.1)))
    bone("spine", joints["spine_lumbar_l5"], middle_back, "pelvis")
    bone("chest", middle_back, joints["spine_thoracic_t1"], "spine", True)
    bone("neck", joints["spine_thoracic_t1"], joints["spine_cervical_c1"], "chest", True)
    bone("head", joints["spine_cervical_c1"],
         joints["spine_cervical_c1"] + Vector((0.0, 0.0, 0.2 * scale)), "neck", True)
    for side in ("L", "R"):
        # Behind the kneecap, where the knee turns.
        knee = joints[f"leg_patella.{side}"] + Vector((0.0, -0.03 * scale, 0.0))
        bone(f"thigh.{side}", joints[f"leg_femur.{side}"], knee, "pelvis")
        bone(f"shin.{side}", knee, joints[f"foot_talus.{side}"], f"thigh.{side}", True)
        bone(f"foot.{side}", joints[f"foot_talus.{side}"], joints[f"foot_center.{side}"],
             f"shin.{side}", True)
        # A short toe beyond the ball: the app reads where the foot ends from it.
        toes = joints[f"foot_center.{side}"] - joints[f"foot_talus.{side}"]
        bone(f"toe.{side}", joints[f"foot_center.{side}"],
             joints[f"foot_center.{side}"] + toes.normalized() * 0.04 * scale, f"foot.{side}", True)
        towards = (joints[f"arm_humerus.{side}"] - joints["spine_thoracic_t1"]).normalized()
        bone(f"shoulder.{side}", joints["spine_thoracic_t1"] + towards * 0.03,
             joints[f"arm_humerus.{side}"], "chest")
        bone(f"upper_arm.{side}", joints[f"arm_humerus.{side}"], joints[f"arm_radius.{side}"],
             f"shoulder.{side}", True)
        bone(f"forearm.{side}", joints[f"arm_radius.{side}"], joints[f"hand_center.{side}"],
             f"upper_arm.{side}", True)
        along = (joints[f"hand_center.{side}"] - joints[f"arm_radius.{side}"]).normalized()
        bone(f"hand.{side}", joints[f"hand_center.{side}"],
             joints[f"hand_center.{side}"] + along * 0.09 * scale, f"forearm.{side}", True)
        # Three bones a finger, from the knuckle; the thumb's from its base in the palm. The
        # last runs on past the last joint as far as the one before it is long.
        for finger in FINGERS:
            chain = (("metacarpal", "proximal", "distal") if finger == "thumb"
                     else ("proximal", "middle", "distal"))
            points = [joints[f"{joint}.{finger}.{side}"] for joint in chain]
            points.append(points[2] + (points[2] - points[1]))
            parent = f"hand.{side}"
            for k in range(3):
                name = f"{finger}_{k + 1}.{side}"
                bone(name, points[k], points[k + 1], parent, k > 0)
                parent = name
    bpy.ops.object.mode_set(mode="OBJECT")

    coarse = body.copy()
    coarse.data = body.data.copy()
    bpy.context.scene.collection.objects.link(coarse)
    thin = coarse.modifiers.new("thin", "DECIMATE")
    thin.ratio = WEIGHT_COPY
    bpy.context.view_layer.objects.active = coarse
    bpy.ops.object.modifier_apply(modifier="thin")
    for obj in bpy.context.selected_objects:
        obj.select_set(False)
    coarse.select_set(True)
    rig.select_set(True)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.parent_set(type="ARMATURE_AUTO")
    for group in coarse.vertex_groups:
        body.vertex_groups.new(name=group.name)
    transfer = body.modifiers.new("weights", "DATA_TRANSFER")
    transfer.object = coarse
    transfer.use_vert_data = True
    transfer.data_types_verts = {"VGROUP_WEIGHTS"}
    transfer.vert_mapping = "POLYINTERP_NEAREST"
    transfer.layers_vgroup_select_src = "ALL"
    transfer.layers_vgroup_select_dst = "NAME"
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.modifier_apply(modifier="weights")
    bpy.data.objects.remove(coarse)
    hand_weights(body, rig)
    skin = body.modifiers.new("rig", "ARMATURE")
    skin.object = rig
    body.parent = rig
    return rig


def hand_weights(body, rig):
    """Weights each hand's vertices to the nearest of its bones (the hand and the finger
    bones): heat weighting leaves the fingers to the wrong bones, and they stretch instead of
    curling. The hand's share of a vertex (what the heat weights gave the hand and fingers
    together) is kept, so the wrist still blends into the forearm."""
    groups = {g.name: g for g in body.vertex_groups}

    def distance(point, a, b):
        axis = b - a
        t = max(0.0, min(1.0, (point - a).dot(axis) / axis.length_squared))
        return (point - (a + axis * t)).length

    for side in ("L", "R"):
        names = [f"hand.{side}"] + [f"{finger}_{k + 1}.{side}" for finger in FINGERS
                                   for k in range(3)]
        indices = {groups[n].index for n in names}
        at = lambda n: rig.matrix_world @ rig.data.bones[n].head_local  # noqa: E731
        segments = {n: [(at(n), rig.matrix_world @ rig.data.bones[n].tail_local)]
                    for n in names}
        # The palm: from the wrist out to each finger's knuckle (the thumb's own first bone
        # lies in the palm already).
        wrist = at(f"hand.{side}")
        segments[f"hand.{side}"] = [(wrist, at(f"{finger}_1.{side}")) for finger in FINGERS[1:]]
        for vertex in body.data.vertices:
            share = sum(e.weight for e in vertex.groups if e.group in indices)
            if share <= 0.0:
                continue
            point = body.matrix_world @ vertex.co
            near = sorted((min(distance(point, a, b) for a, b in segments[n]), n)
                          for n in names)
            nearest = near[0][0]
            # A soft falloff over every bone within reach: a hard split tears the skin.
            chosen = [(d, n) for d, n in near if d <= nearest + HAND_BLEND]
            weights = [(1.0 / (d - nearest + HAND_SOFT)) ** 3 for d, _ in chosen]
            total = sum(weights)
            for n in names:
                groups[n].remove([vertex.index])
            for (_, n), w in zip(chosen, weights):
                groups[n].add([vertex.index], share * w / total, "REPLACE")


def aim(rig, name, target):
    """Turns pose bone `name` (armature space) so its tail points at `target`, keeping its roll."""
    bone = rig.pose.bones[name]
    head = bone.head.copy()
    y = (target - head).normalized()
    x = bone.matrix.col[0].to_3d()
    x = (x - y * x.dot(y)).normalized()
    z = x.cross(y)
    bone.matrix = Matrix((
        (x.x, y.x, z.x, head.x),
        (x.y, y.y, z.y, head.y),
        (x.z, y.z, z.z, head.z),
        (0.0, 0.0, 0.0, 1.0),
    ))
    update()


def turn_bone(rig, name, angle, axis="X", about=None):
    """Turns pose bone `name` by `angle` round the armature's `axis`, about its head or the
    point `about`."""
    bone = rig.pose.bones[name]
    pivot = bone.head.copy() if about is None else about
    bone.matrix = (Matrix.Translation(pivot) @ Matrix.Rotation(angle, 4, axis)
                   @ Matrix.Translation(-pivot) @ bone.matrix)
    update()


# The eyes' height above the nose's tip, for the reference body (1.762 m).
EYES_ABOVE_NOSE = 0.036


def realistic_headgear(body, rig, sex, scale):
    """Hair, helmet and sunglasses for the realistic head, as `headgear` fits them to the
    primitive heads: on a copy of the upright head (the faces the head bone moves) with eyes and
    ears placed by its proportions. Returned parented to the head bone."""
    head = body.copy()
    head.data = body.data.copy()
    head.modifiers.clear()
    head.parent = None
    bpy.context.scene.collection.objects.link(head)
    group = head.vertex_groups["head"].index
    mesh = bmesh.new()
    mesh.from_mesh(head.data)
    deform = mesh.verts.layers.deform.verify()
    drop = [v for v in mesh.verts if v[deform].get(group, 0.0) < 0.5]
    bmesh.ops.delete(mesh, geom=drop, context="VERTS")
    mesh.to_mesh(head.data)
    mesh.free()
    low, high = bounds(head)
    centre = middle(low, high)
    # The eyes a little above the nose's tip, the face's most forward point on its middle.
    nose = max((p for p in world_vertices(head) if abs(p.x - centre.x) < 0.012),
               key=lambda p: p.y)
    eye_z = nose.z + EYES_ABOVE_NOSE * scale
    front = max(p.y for p in world_vertices(head) if abs(p.z - eye_z) < 0.01 and abs(p.x) < 0.02)
    markers = {}
    for side, sign in (("L", -1.0), ("R", 1.0)):
        eye = bpy.data.objects.new(f"eye.{side}", None)
        eye.location = Vector((sign * 0.032 * scale, front - 0.02, eye_z))
        ear = bpy.data.objects.new(f"ear.{side}", None)
        ear.location = Vector((sign * (high.x - low.x) * 0.47, centre.y - 0.005, eye_z - 0.01))
        for marker in (eye, ear):
            bpy.context.scene.collection.objects.link(marker)
            markers[marker.name] = marker
    update()
    skull = [p for p in world_vertices(body)
             if p.z > eye_z - 0.005 and Vector((p.x - centre.x, p.y - centre.y)).length < 0.16]
    # The whole upright body's surface, for fitting the helmet and laying the straps on.
    skull_tree = BVHTree.FromPolygons(world_vertices(body),
                                      [list(p.vertices) for p in body.data.polygons])
    parts = {"head": head, "nose": head, "nose_bridge": head, "skull": skull,
             "skull_tree": skull_tree, **markers}
    headgear(parts, sex)
    gear = bpy.data.objects["headgear"]
    world = gear.matrix_world.copy()
    gear.parent = rig
    gear.parent_type = "BONE"
    gear.parent_bone = "head"
    gear.matrix_world = world
    # `headgear` removes the eyes itself.
    for name in markers:
        if name in bpy.data.objects:
            bpy.data.objects.remove(bpy.data.objects[name])
    bpy.data.objects.remove(head)
    update()
    return gear


def bake_rest_pose(body, gear, rig):
    """The body's mesh takes the riding pose (smoothed where joints bend) and the headgear
    joins it, held by the head bone; `finish_rest_pose` then makes the pose the rest."""
    smooth_joints = body.modifiers.new("smooth", "CORRECTIVE_SMOOTH")
    smooth_joints.factor = 0.6
    smooth_joints.iterations = 8
    smooth_joints.smooth_type = "LENGTH_WEIGHTED"
    smooth_joints.rest_source = "ORCO"
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.modifier_apply(modifier="rig")
    bpy.ops.object.modifier_apply(modifier="smooth")
    world = gear.matrix_world.copy()
    gear.parent = None
    gear.matrix_world = world
    gear.data.transform(gear.matrix_world)
    gear.matrix_world = Matrix.Identity(4)
    held = gear.vertex_groups.new(name="head")
    held.add(range(len(gear.data.vertices)), 1.0, "REPLACE")
    for obj in bpy.context.selected_objects:
        obj.select_set(False)
    gear.select_set(True)
    body.select_set(True)
    bpy.context.view_layer.objects.active = body
    body_world = body.matrix_world.copy()
    body.parent = None
    body.matrix_world = body_world
    bpy.ops.object.join()
    update()


def thin_body(body):
    """Thins the posed body for the app; collapsing edges keeps the weights. The hands are
    spared so fingers stay round, as are the helmet's ribs and vent lips."""
    bpy.context.view_layer.objects.active = body
    hands = [g.index for g in body.vertex_groups if g.name.startswith(HAND_GROUPS)]
    spared = body.vertex_groups.new(name="spared")
    for vertex in body.data.vertices:
        weight = sum(g.weight for g in vertex.groups if g.group in hands)
        if weight > 0.0:
            spared.add([vertex.index], min(weight * 2.0, 1.0), "REPLACE")
    helmet_materials = {index for index, slot in enumerate(body.material_slots)
                        if slot.material and slot.material.name in ("helmet", "vents")}
    helmet_vertices = {v for face in body.data.polygons if face.material_index in helmet_materials
                       for v in face.vertices}
    spared.add(sorted(helmet_vertices), 1.0, "REPLACE")
    lighter = body.modifiers.new("lighter", "DECIMATE")
    lighter.ratio = REALISTIC_DETAIL
    lighter.vertex_group = "spared"
    lighter.invert_vertex_group = True
    lighter.vertex_group_factor = 10.0
    bpy.ops.object.modifier_apply(modifier="lighter")
    body.vertex_groups.remove(body.vertex_groups["spared"])


def finish_rest_pose(body, rig):
    """Makes the pose the skeleton's rest, the body skinned to it again."""
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.mode_set(mode="POSE")
    bpy.ops.pose.armature_apply(selected=False)
    bpy.ops.object.mode_set(mode="OBJECT")
    skin = body.modifiers.new("rig", "ARMATURE")
    skin.object = rig
    body.parent = rig
    body.matrix_parent_inverse = rig.matrix_world.inverted()
    update()


# The headgear's own materials, kept when the body is dressed (the hair tie is in the kit's
# sleeve colour).
HEADGEAR_MATERIALS = ("hair", "helmet", "vents", "glasses", "sleeve")


# Body regions by the bones that move them, for dressing.
# Where the jersey's collar crosses the neck bone, from its root.
COLLAR = 0.25
REGIONS = {
    "shoes": ("foot.", "toe."), "hands": HAND_GROUPS, "head": ("head", "neck"),
    "forearm": ("forearm.",), "upper_arm": ("upper_arm.",), "shin": ("shin.",),
    "thigh": ("thigh.",), "trunk": ("pelvis", "spine", "chest", "shoulder."),
}


def dress_realistic(body, rig):
    """The cycling kit, cut by planes across the skeleton so its edges run clean: shorts over
    the pelvis and thighs to above the knee, the jersey over the trunk with sleeves to half way
    down the upper arm, socks above white shoes; skin elsewhere, bare hands included. The toes
    are closed into a shoe and the fingers curled round the hoods. The headgear keeps its own
    materials."""
    mesh = body.data
    names = [g.name for g in body.vertex_groups]

    def along(name, point):
        # The posed bones: the body is dressed in the riding pose, before it becomes the rest.
        bone = rig.pose.bones[name]
        head, tail = rig.matrix_world @ bone.head, rig.matrix_world @ bone.tail
        axis = tail - head
        return (point - head).dot(axis) / axis.length_squared

    def near_limb(name, point, radius):
        """Past the joint the limb starts at, and within `radius` of its axis (or beyond)."""
        bone = rig.pose.bones[name]
        head, tail = rig.matrix_world @ bone.head, rig.matrix_world @ bone.tail
        axis = (tail - head).normalized()
        t = (point - head).dot(axis)
        length = (tail - head).length
        return 0.0 < t < length * 1.15 and ((point - head) - axis * t).length < radius

    def outside_shoulder(side, point):
        """Farther out from the body's middle than the shoulder joint: the arm, not the chest
        beside it."""
        shoulder = rig.pose.bones[f"upper_arm.{side}"].head
        return abs((rig.matrix_world.inverted() @ point).x) > abs(shoulder.x) - 0.05

    def weight(vertex, prefixes):
        return sum(g.weight for g in vertex.groups if names[g.group].startswith(prefixes))

    # Shoes: each foot below the ankle made the convex hull of its own vertices — a closed,
    # rounded shoe instead of toes. The hull keeps the vertices, and so their weights.
    shape = bmesh.new()
    shape.from_mesh(mesh)
    deform = shape.verts.layers.deform.verify()
    groups = {n: k for k, n in enumerate(names)}
    for prefixes, beyond, bone_of in ((("toe.", "foot."), 0.0, "foot"),):
        for side in ("L", "R"):
            chosen = set()
            for v in shape.verts:
                total = sum(v[deform].get(groups[n], 0.0) for n in names
                            if n.startswith(prefixes) and n.endswith(side))
                if total >= 0.5 and along(f"{bone_of}.{side}", body.matrix_world @ v.co) > beyond:
                    chosen.add(v)
            if len(chosen) < 4:
                continue
            old_faces = [f for f in shape.faces if all(v in chosen for v in f.verts)]
            hull = bmesh.ops.convex_hull(shape, input=list(chosen))
            bmesh.ops.delete(shape, geom=old_faces, context="FACES_ONLY")
            loose = [v for v in hull["geom_interior"] if isinstance(v, bmesh.types.BMVert)]
            bmesh.ops.delete(shape, geom=loose, context="VERTS")
    # The kit's edges cut into the mesh as planes across each limb, so they run straight:
    # sleeve ends and shoulder seams, shorts' hems, the waist, the socks' tops.
    cuts = []
    for side in ("L", "R"):
        cuts += [(f"upper_arm.{side}", 0.0, 0.14), (f"upper_arm.{side}", 0.5, 0.15),
                 (f"thigh.{side}", 0.82, 0.2), (f"shin.{side}", 0.78, 0.16)]
    cuts.append(("spine", 0.0, 0.25))
    cuts.append(("neck", COLLAR, 0.12))
    for name, at, radius in cuts:
        bone = rig.pose.bones[name]
        head, tail = rig.matrix_world @ bone.head, rig.matrix_world @ bone.tail
        axis = tail - head
        point = head + axis * at
        normal = axis.normalized()
        near = [f for f in shape.faces
                if abs((f.calc_center_median() - point).dot(normal)) < 0.15
                and ((f.calc_center_median() - point)
                     - normal * (f.calc_center_median() - point).dot(normal)).length < radius]
        geom = list({e for f in near for e in f.edges}) + near \
            + list({v for f in near for v in f.verts})
        if near:
            bmesh.ops.bisect_plane(shape, geom=geom, plane_co=point, plane_no=normal)
    shape.to_mesh(mesh)
    shape.free()
    mesh.update()

    for polygon in mesh.polygons:
        slot = (mesh.materials[polygon.material_index]
                if polygon.material_index < len(mesh.materials) else None)
        if slot is not None and slot.name in HEADGEAR_MATERIALS:
            continue
        verts = [mesh.vertices[v] for v in polygon.vertices]
        centre = body.matrix_world @ polygon.center
        side = "L" if (rig.matrix_world.inverted() @ centre).x < 0 else "R"
        # The bone that moves the face most decides the region; the planes cut above decide
        # the hems within it. (Distances along a bone alone fail: on the downstroke the shin
        # points back, and the seat would measure as past the knee.)
        region = max(REGIONS, key=lambda r: sum(weight(v, REGIONS[r]) for v in verts))
        if region == "shoes":
            choice = region
        elif region in ("hands", "forearm") or along("neck", centre) > COLLAR:
            choice = "skin"
        elif region == "shin":
            choice = "socks" if along(f"shin.{side}", centre) > 0.78 else "skin"
        elif region == "thigh":
            choice = "shorts" if along(f"thigh.{side}", centre) < 0.82 else "skin"
        elif near_limb(f"upper_arm.{side}", centre, 0.12) and outside_shoulder(side, centre):
            # Past the shoulder's plane: sleeve to half way down the arm, then skin.
            choice = "sleeve" if along(f"upper_arm.{side}", centre) < 0.5 else "skin"
        elif region == "trunk" and along("spine", centre) < 0.0:
            choice = "shorts"
        else:
            choice = "jersey"
        polygon.material_index = material_slot(body, choice)
    smooth(mesh)


def hand_shape(rig, side):
    """The posed hand's length (wrist to middle knuckle) and the way its palm faces (square to
    the knuckles, on the side the thumb lies), from the finger joints."""
    bones = rig.pose.bones
    wrist = bones[f"hand.{side}"].head
    length = bones[f"middle_1.{side}"].head - wrist
    across = bones[f"index_1.{side}"].head - bones[f"little_1.{side}"].head
    palm = length.cross(across).normalized()
    if (bones[f"thumb_3.{side}"].tail - wrist).dot(palm) < 0.0:
        palm = -palm
    return length.normalized(), palm


def hand_frame(length, palm):
    """A rotation frame from a hand's length and palm facing (columns: length, palm, across)."""
    length = length.normalized()
    palm = (palm - length * palm.dot(length)).normalized()
    return Matrix((length, palm, length.cross(palm))).transposed()


def grip(rig, side):
    """Curls each finger round the hood, joint by joint towards the palm, the thumb round the
    hood's inner side."""
    palm = HAND_ON_HOODS[side][1]
    bones = rig.pose.bones
    middle = (bones[f"middle_1.{side}"].tail - bones[f"middle_1.{side}"].head).normalized()
    for finger in FINGERS:
        first = bones[f"finger_1.{side}".replace("finger", finger)]
        direction = (first.tail - first.head).normalized()
        if finger not in ("thumb", "middle"):
            # The base mesh's fingers are spread: close most of the gap to the middle finger.
            spread = math.atan2(direction.cross(middle).dot(palm), direction.dot(middle))
            turn_bone(rig, f"{finger}_1.{side}", spread * FINGER_CLOSE, axis=palm)
            direction = (first.tail - first.head).normalized()
        # One axis for the whole finger, from the knuckle: taken joint by joint, it flips
        # once the finger has curled past the palm's direction and the tip bends back out.
        if finger == "thumb":
            # The thumb lies along the hood's inner flank, pointing forward and a little down,
            # its tip pressed against the hood below the horn.
            inward = 1.0 if side == "L" else -1.0
            for k, (across, ahead, down) in enumerate(THUMB_AIM):
                name = f"thumb_{k + 1}.{side}"
                head = bones[name].head.copy()
                aim(rig, name, head + Vector((inward * across, ahead, -down)))
            continue
        axis = direction.cross(palm)
        if axis.length < 1e-6:
            continue
        axis.normalize()
        for k, angle in enumerate(FINGER_CURL):
            turn_bone(rig, f"{finger}_{k + 1}.{side}", angle, axis=axis)


def material_slot(obj, name):
    """The index of material `name` on `obj`, added if missing."""
    for k, slot in enumerate(obj.data.materials):
        if slot is not None and slot.name == name:
            return k
    obj.data.materials.append(material(name))
    return len(obj.data.materials) - 1


def build(sex):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    body, joints, scale = load_realistic(sex)
    centre_leg_joints(body, joints)
    centre_hand_joints(body, joints)
    bpy.context.view_layer.objects.active = body
    thigh = (joints["leg_patella.R"] - joints["leg_femur.R"]).length
    shin = (joints["foot_talus.R"] - joints["leg_patella.R"]).length
    ball = joints["foot_center.R"] - joints["foot_talus.R"]
    # The hips on the seat tube's line, the knee bending `KNEE_AT_BOTTOM` with the pedal at
    # the bottom and the ball of the foot on it.
    # The sole stands on the pedal with a cleat under it: the ball of the foot's joint is its
    # own height above the sole (the standing body's ground) over the pedal's top.
    ankle_over_pedal = Vector((0.0, -ball.y,
                               PEDAL_TOP + CLEAT + joints["foot_center.R"].z - ball.z))
    ankle = (BOTTOM_BRACKET + Vector((0.0, 0.0, -CRANK))
             + Matrix.Rotation(FOOT_AT_BOTTOM, 3, "X") @ ankle_over_pedal)
    seat_axis = Vector((0.0, -math.cos(SEAT_ANGLE), math.sin(SEAT_ANGLE)))
    offset = BOTTOM_BRACKET - ankle
    leg = math.sqrt(thigh ** 2 + shin ** 2 + 2.0 * thigh * shin * math.cos(KNEE_AT_BOTTOM))
    along = -seat_axis.dot(offset) + math.sqrt(
        seat_axis.dot(offset) ** 2 - offset.length_squared + leg * leg)
    hips_middle = (joints["leg_femur.L"] + joints["leg_femur.R"]) / 2
    move_by = BOTTOM_BRACKET + seat_axis * along - hips_middle
    body.data.transform(Matrix.Translation(move_by))
    for name in joints:
        joints[name] = joints[name] + move_by
    centre = hips_middle + move_by
    # The sit bones rest on the saddle. The standing body's lowest point between the hips is
    # the inner thighs, far lower, which put the saddle and hoods too low and the back flat.
    saddle_top = centre.z - SIT_BONES_BELOW_HIPS * scale
    rig = realistic_skeleton(body, joints, scale)
    gear = realistic_headgear(body, rig, sex, scale)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.mode_set(mode="POSE")

    upper = (joints["arm_radius.R"] - joints["arm_humerus.R"]).length
    lower = (joints["hand_center.R"] - joints["arm_radius.R"]).length
    arm = upper + lower

    def shoulders():
        return (rig.pose.bones["upper_arm.L"].head + rig.pose.bones["upper_arm.R"].head) / 2

    # Bend the back until the hands reach hoods far enough ahead, as `lean` does.
    step = math.radians(1.0)
    while True:
        torso = shoulders() - centre
        angle = math.atan2(torso.z, torso.y)
        shoulder = rig.pose.bones["upper_arm.R"].head
        hoods_z = saddle_top - DROP_TO_HOODS
        across = HOODS_HALF_WIDTH - abs(shoulder.x)
        # The arm reaches the wrist, which sits behind and above the hood.
        wrist_z = hoods_z + WRIST_ON_HOOD.z
        reach = (arm * REALISTIC_REACH) ** 2 - (shoulder.z - wrist_z) ** 2 - across ** 2
        hoods = (Vector((0.0, shoulder.y + math.sqrt(reach) - WRIST_ON_HOOD.y, hoods_z))
                 if reach > 0 else None)
        ready = hoods is not None and hoods.y >= HOODS_AHEAD * scale
        if (angle <= REALISTIC_TORSO[0] and ready) or angle <= REALISTIC_TORSO[1]:
            break
        # About the hip joints, which stay where the fit put them.
        # Mostly the pelvis tipping forward, for a straight back rather than a hunched one.
        turn_bone(rig, "pelvis", -0.6 * step, about=centre)
        turn_bone(rig, "spine", -0.25 * step)
        turn_bone(rig, "chest", -0.15 * step)
    if hoods is None:
        raise RuntimeError("the arms cannot reach the hoods")
    # The neck reaches forward and up out of the bent back; the head comes up over it, nearly
    # upright, so the rider looks up the road rather than at the front wheel.
    for name, from_vertical in (("neck", REALISTIC_NECK), ("head", REALISTIC_HEAD)):
        bone = rig.pose.bones[name]
        aim(rig, name, bone.head + Vector((0.0, math.sin(from_vertical), math.cos(from_vertical))))

    def place_arm(side, sign, wrist):
        shoulder = rig.pose.bones[f"upper_arm.{side}"].head
        elbow = two_bone(shoulder, wrist, upper, lower, Vector((sign * 0.5, -0.3, -1.0)))
        aim(rig, f"upper_arm.{side}", elbow)
        aim(rig, f"forearm.{side}", wrist)
        aim(rig, f"hand.{side}", wrist + Vector((0.0, 0.08, -0.04)))
        # The base mesh's hand lies turned on its bone: turn it by its finger joints, fingers
        # forward and a little down, the palm onto the hood.
        hand = rig.pose.bones[f"hand.{side}"]
        length, palm = hand_shape(rig, side)
        turn = (hand_frame(*HAND_ON_HOODS[side]) @ hand_frame(length, palm).transposed()).to_4x4()
        head = hand.head.copy()
        hand.matrix = Matrix.Translation(head) @ turn @ Matrix.Translation(-head) @ hand.matrix
        update()

    def knuckles(side):
        return sum((rig.pose.bones[f"{finger}_1.{side}"].head for finger in FINGERS[1:]),
                   Vector()) / (len(FINGERS) - 1)

    def hand_across(side):
        """Where the posed hand's skin lies across the bike (its middle x), where the bones
        alone do not say."""
        evaluated = body.evaluated_get(bpy.context.evaluated_depsgraph_get())
        mesh = evaluated.to_mesh()
        groups = {g.index for g in body.vertex_groups
                  if g.name.startswith(HAND_GROUPS) and g.name.endswith(side)}
        xs = sorted((evaluated.matrix_world @ mesh.vertices[v.index].co).x
                    for v in body.data.vertices
                    if sum(g.weight for g in v.groups if g.group in groups) > 0.5)
        evaluated.to_mesh_clear()
        return xs[len(xs) // 2]

    for side, sign in (("L", -1.0), ("R", 1.0)):
        # Place the hand, see where its knuckles landed, and move the wrist by the difference,
        # so the knuckles are just behind the horn and the palm rests on the hood.
        hood = Vector((sign * HOODS_HALF_WIDTH, hoods.y, hoods.z))
        wrist = hood + WRIST_ON_HOOD
        place_arm(side, sign, wrist)
        wrist += (hood + KNUCKLES_ON_HOOD) - knuckles(side)
        place_arm(side, sign, wrist)
        # Across the bike, the skin, not the bones, has to sit on the hood.
        wrist.x += hood.x - hand_across(side)
        place_arm(side, sign, wrist)
        grip(rig, side)
        # The legs at crank angle 0: the right pedal forward, the left one back.
        pedal = BOTTOM_BRACKET + Vector((sign * PEDAL_OFFSET, sign * CRANK, 0.0))
        foot_turn = Matrix.Rotation(FOOT_AT_BOTTOM * 0.5, 3, "X")
        ankle_at = pedal + foot_turn @ ankle_over_pedal
        hip = rig.pose.bones[f"thigh.{side}"].head
        knee = two_bone(hip, ankle_at, thigh, shin, Vector((0.0, 1.0, 0.0)))
        aim(rig, f"thigh.{side}", knee)
        aim(rig, f"shin.{side}", ankle_at)
        aim(rig, f"foot.{side}", ankle_at + foot_turn @ Vector((0.0, ball.length, -0.02)))
    bpy.ops.object.mode_set(mode="OBJECT")
    bake_rest_pose(body, gear, rig)
    thin_body(body)
    dress_realistic(body, rig)
    finish_rest_pose(body, rig)

    saddle = Vector((0.0, centre.y - 0.02, saddle_top))
    bike_objects, rear_hub, front_hub = bike(saddle, hoods)
    shift = Vector((0.0, -(rear_hub.y + front_hub.y) / 2.0, 0.0))
    for obj in list(bpy.context.scene.objects):
        if obj.parent is None:
            move(obj, shift)
    body.name = "body"
    rig.name = "skeleton"
    os.makedirs(OUT, exist_ok=True)
    bpy.ops.export_scene.gltf(filepath=os.path.join(OUT, f"rider_{sex}.glb"),
                              export_format="GLB", export_yup=True, export_skins=True,
                              export_apply=False, export_animations=False)
    print(f"built realistic rider_{sex}: torso {math.degrees(angle):.0f}° above horizontal, "
          f"hoods {hoods.y:.3f} m ahead of the bottom bracket, wheelbase "
          f"{front_hub.y - rear_hub.y:.3f} m; {triangles([body])} rider and "
          f"{triangles(bike_objects)} bike triangles")
    preview_dir = os.environ.get("PREVIEW")
    if preview_dir:
        import preview  # noqa: E402  (art/preview.py; on the path below)
        preview.render([body, *bike_objects], preview_dir, f"realistic_{sex}",
                       views=("right", "quarter", "front"))
    pose = rig.pose.bones
    return {
        "hips": [godot(rig.matrix_world @ pose[f"thigh.{side}"].head) for side in ("L", "R")],
        "thigh": round(thigh, 4),
        "shin": round(shin, 4),
        "ankle_over_pedal": godot(ankle_over_pedal),
        "ball_over_pedal": godot(Vector((0.0, 0.0, ankle_over_pedal.z + ball.z))),
        "wheel_radius": WHEEL_RADIUS,
        "wheel_rear": godot(rear_hub + shift),
        "wheel_front": godot(front_hub + shift),
        "bottom_bracket": godot(BOTTOM_BRACKET + shift),
        "hands": godot(hoods + shift),
        "crank": CRANK,
        "pedal_offset": PEDAL_OFFSET,
    }


def main():
    # art/preview.py, for PREVIEW=<dir>.
    sys.path.insert(0, os.path.dirname(HERE))
    manifest = {sex: build(sex) for sex in ("female", "male")}
    with open(os.path.join(OUT, "riders.json"), "w", encoding="utf-8") as file:
        json.dump(manifest, file, indent=1)
        file.write("\n")


main()
