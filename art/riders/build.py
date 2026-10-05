"""Builds the riders on their bikes (R46, ADR 0011) and exports them for Godot.

Run in the art container:
    scripts/art.sh blender --background --factory-startup --python art/riders/build.py

Each rider starts from Blender Studio's CC0 stylized "primitive" body (art/sources/
human-base-meshes): a mannequin of parts with their origins at the joints. The script keeps the
parts at their base level (no subdivision), so they are faceted and lean; drops fingers, toes
and eyes; brings the proportions closer to natural ones (longer arms, smaller heads); dresses
the parts by material name; and adds a helmet, sunglasses and, for the female rider, a
ponytail. It then seats the rider: the hips where the legs reach the pedals almost straight,
the torso bent forward until the hands reach the hoods of a bike sized to fit. The bike is
built around those contact points, so each rider gets their own.

`rider_<sex>.glb` holds these nodes, in Godot's axes (x right, y up, −z forward; metres, the
origin on the ground between the wheels):

- `body`: everything of the rider that does not move while riding, posed;
- `thigh_l`, `shin_l`, `foot_l` and the same with `_r`: the legs, each with its origin at its
  joint (hip, knee, ankle), thigh and shin running down their −y axis with their front to −z,
  so the app can aim them at the pedals; the foot level;
- `frame`, `wheel_front`, `wheel_rear`: the bike, the wheels with their origins at the hubs;
- `crankset` (origin at the bottom bracket, the right crank pointing forward) and `pedal_l`,
  `pedal_r` (origins at the axles).

`riders.json` beside them holds what the app needs to pedal: hips, thigh and shin lengths,
where the ankle sits over the pedal axle, and the bike's hubs, bottom bracket and cranks. The
app colours the materials by name from the palette (`rider_female`, `rider_male`, `bike`).
"""

import json
import math
import os

import bmesh
import bpy
from mathutils import Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.normpath(
    os.path.join(HERE, "..", "sources", "human-base-meshes", "primitive_stylized_bodies.blend"))
OUT = os.path.normpath(os.path.join(HERE, "..", "..", "app", "assets", "models", "riders"))

# Parts too small to show at riding distance, or hidden behind sunglasses.
DROP = ("finger", "thumb", "toe", "eye")
# Parts with many small facets for their size: thinned to this share of their faces.
THIN = {"nose": 0.3, "nose_bridge": 0.45, "ear": 0.35}
# The cycling kit, by part; the rest is skin.
DRESS = {
    "pelvis": "shorts", "leg_upper": "shorts", "belly": "jersey", "chest": "jersey",
    "breast": "jersey", "shoulder": "sleeve", "arm_upper": "sleeve", "hand": "gloves",
    "foot": "shoes",
}
LEGS = ("leg_upper", "leg_lower", "foot")
LEG_NODES = {"leg_upper": "thigh", "leg_lower": "shin", "foot": "foot"}
# The stylized bodies have short arms, a short torso (female) and big heads; Torqa's riders
# have natural proportions (ADR 0011). Arms, hands (standing in for the dropped fingers) and
# feet are lengthened by these factors, the torso by metres, the head scaled.
PROPORTIONS = {
    "female": {"arms": 1.3, "hands": 1.35, "feet": 1.2, "torso": 0.07, "head": 0.86},
    "male": {"arms": 1.22, "hands": 1.35, "feet": 1.2, "torso": 0.0, "head": 0.86},
}

# The bike, in Blender's axes: x right, y forward, z up. Built with the bottom bracket at
# y = 0, then moved so the origin lies between the wheels.
WHEEL_RADIUS = 0.34
BOTTOM_BRACKET = Vector((0.0, 0.0, 0.27))
CHAINSTAY = 0.41
CRANK = 0.17
PEDAL_OFFSET = 0.12
SEAT_ANGLE = math.radians(74.0)
HEAD_ANGLE = math.radians(72.5)
FORK_RAKE = 0.045
# Axle to crown, along the steering axis.
FORK_LENGTH = 0.37
STEM = 0.1
HOODS_HALF_WIDTH = 0.2
# Saddle top to hoods top.
DROP_TO_HOODS = 0.03
# The hoods at least this far ahead of the bottom bracket, so the front wheel clears the feet.
HOODS_AHEAD = 0.5
# Bike fit: the knee's bend with the pedal at the bottom, and the foot's pitch there (toes
# down; app/scenes/rider_avatar.gd turns the feet so, ANKLE_PITCH and ANKLE_SWING).
KNEE_AT_BOTTOM = math.radians(30.0)
FOOT_AT_BOTTOM = math.radians(-18.0)
# Arm extension on the hoods: shoulder to wrist as a share of the arm, elbows a little bent.
REACH = 0.92
# The torso's angle above horizontal: upright enough to look relaxed, flat enough to reach.
TORSO_ANGLES = (math.radians(50.0), math.radians(28.0))
# The wrist sits behind and above the hood's top, the palm on it.
WRIST_ON_HOOD = Vector((0.0, -0.06, 0.045))

PREVIEW = {
    "skin": (0.74, 0.53, 0.47), "jersey": (0.94, 0.71, 0.8), "sleeve": (0.81, 0.35, 0.65),
    "shorts": (0.18, 0.24, 0.48), "gloves": (0.38, 0.26, 0.4), "shoes": (0.22, 0.35, 0.63),
    "helmet": (0.96, 0.76, 0.43), "glasses": (0.29, 0.18, 0.27), "hair": (0.38, 0.26, 0.4),
    "frame": (0.89, 0.44, 0.37), "tyre": (0.29, 0.18, 0.27), "rim": (0.95, 0.92, 0.85),
    "metal": (0.56, 0.58, 0.61), "saddle": (0.38, 0.26, 0.4), "bar": (0.38, 0.26, 0.4),
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

    def tube(self, start, end, radius, material_name, sides=6, caps=True):
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
        mesh.shade_flat()
        obj = bpy.data.objects.new(name, mesh)
        obj.matrix_world = Matrix.Translation(at)
        bpy.context.scene.collection.objects.link(obj)
        return obj


# --- the body --------------------------------------------------------------------------------


def load_body(sex):
    """The parts of one body from the source, keyed by part name (`chest`, `arm_upper.L`, …)."""
    marker = f"_{sex}_primitive_stylized"
    with bpy.data.libraries.load(SOURCE, link=False) as (source, target):
        target.objects = [name for name in source.objects if marker in name]
    parts = {}
    for obj in target.objects:
        bpy.context.scene.collection.objects.link(obj)
        parts[obj.name.replace("GEO-", "").replace(marker, "")] = obj
    update()
    for part in [p for p in parts if p.startswith(DROP)]:
        bpy.data.objects.remove(parts.pop(part))
    for part, obj in parts.items():
        kind = part.split(".")[0]
        # Left and right share their meshes; each part gets its own, to be reshaped alone.
        obj.data = obj.data.copy()
        obj.modifiers.clear()
        if kind in THIN:
            thin = obj.modifiers.new("thin", "DECIMATE")
            thin.ratio = THIN[kind]
            depsgraph = bpy.context.evaluated_depsgraph_get()
            thinned = bpy.data.meshes.new_from_object(obj.evaluated_get(depsgraph))
            obj.modifiers.clear()
            obj.data = thinned
        obj.data.materials.clear()
        obj.data.materials.append(material(DRESS.get(kind, "skin")))
        for polygon in obj.data.polygons:
            polygon.material_index = 0
        obj.data.shade_flat()
    return parts


def stretch(obj, start, direction, factor):
    """Lengthens a part beyond `start` along `direction` by `factor`; parts that hang from it
    move along."""
    direction = direction.normalized()
    to_world = obj.matrix_world.copy()
    to_local = to_world.inverted()
    for vertex in obj.data.vertices:
        point = to_world @ vertex.co
        beyond = (point - start).dot(direction)
        if beyond > 0.0:
            vertex.co = to_local @ (point + direction * beyond * (factor - 1.0))
    for child in obj.children:
        beyond = (origin(child) - start).dot(direction)
        if beyond > 0.0:
            move(child, direction * beyond * (factor - 1.0))


def proportion(parts, sex):
    """Brings the standing body (facing −y) to natural proportions."""
    change = PROPORTIONS[sex]
    belly, chest = parts["belly"], parts["chest"]
    waist = origin(belly) + Vector((0, 0, 0.03))
    stretch(belly, waist, Vector((0, 0, 1)),
            1.0 + change["torso"] / (origin(chest).z - waist.z))
    for side in ("L", "R"):
        upper, lower, hand = (parts[f"{p}.{side}"] for p in ("arm_upper", "arm_lower", "hand"))
        stretch(upper, origin(upper), origin(lower) - origin(upper), change["arms"])
        forearm = origin(hand) - origin(lower)
        stretch(lower, origin(lower), forearm, change["arms"])
        stretch(hand, origin(hand), forearm, change["hands"])
        foot = parts[f"foot.{side}"]
        stretch(foot, origin(foot), Vector((0, -1, 0)), change["feet"])
    head = parts["head"]
    transform(head, Matrix.Scale(change["head"], 3), origin(head))


def along_part(obj, start, end):
    """Each vertex's and each face's distance along the segment from `start` towards `end`,
    as a share of its length."""
    axis = end - start
    length = axis.length
    axis /= length
    points = world_vertices(obj)
    shares = [(p - start).dot(axis) / length for p in points]
    faces = [sum(shares[v] for v in polygon.vertices) / len(polygon.vertices)
             for polygon in obj.data.polygons]
    return shares, faces


def tailor(parts):
    """Ends the shorts above the knees and the sleeves halfway down the upper arms, and trims
    the thighs at hip and knee so they do not show through when the legs bend."""
    skin = material("skin")
    for side in ("L", "R"):
        thigh, shin = parts[f"leg_upper.{side}"], parts[f"leg_lower.{side}"]
        hip, knee = origin(thigh), origin(shin)
        shares, faces = along_part(thigh, hip, knee)
        axis = knee - hip
        to_local = thigh.matrix_world.inverted()
        for vertex, share in zip(thigh.data.vertices, shares):
            trim = max(share - 1.0, 0.0) + min(share + 0.05, 0.0)
            if trim:
                vertex.co = to_local @ (thigh.matrix_world @ vertex.co - axis * trim)
        for obj, cut, faces in ((thigh, 0.8, faces),
                                (parts[f"arm_upper.{side}"], 0.55,
                                 along_part(parts[f"arm_upper.{side}"],
                                            origin(parts[f"arm_upper.{side}"]),
                                            origin(parts[f"arm_lower.{side}"]))[1])):
            obj.data.materials.append(skin)
            for polygon, share in zip(obj.data.polygons, faces):
                if share > cut:
                    polygon.material_index = 1


def measure_legs(parts):
    """Thigh and shin lengths, and the ball of the foot from the ankle in a frame facing +y."""
    hip, knee, ankle = (origin(parts[f"{p}.L"]) for p in LEGS)
    foot = [p - ankle for p in world_vertices(parts["foot.L"])]
    ahead = [-p.y for p in foot]
    heel, toe = min(ahead), max(ahead)
    ball = Vector((0.0, heel + 0.68 * (toe - heel), min(p.z for p in foot)))
    return (knee - hip).length, (ankle - knee).length, ball


def hips(parts):
    return middle(origin(parts["leg_upper.L"]), origin(parts["leg_upper.R"]))


def shoulders(parts):
    return middle(origin(parts["arm_upper.L"]), origin(parts["arm_upper.R"]))


def headgear(head, sex):
    """Helmet, sunglasses and (female) ponytail on the upright head facing +y, parented to it."""
    low, high = bounds(head)
    centre = middle(low, high)
    half_x, half_y = (high.x - low.x) / 2.0, (high.y - low.y) / 2.0
    eyes = low.z + 0.48 * (high.z - low.z)
    gear = Faces()
    # Helmet: a dome of facets, longer than wide, drawn out at the back.
    base, top = eyes + 0.035, high.z + 0.03
    segments = 10
    layers = []
    for lift, spread in ((0.0, 1.0), (0.45, 0.93), (0.8, 0.62)):
        layer = []
        for k in range(segments):
            a = 2 * math.pi * k / segments
            back = 1.2 if math.cos(a) < 0.0 else 1.0
            layer.append(Vector((centre.x + half_x * 1.18 * spread * math.sin(a),
                                 centre.y - 0.01 + half_y * 1.12 * back * spread * math.cos(a),
                                 base + (top - base) * lift)))
        layers.append(layer)
    inside = Vector((centre.x, centre.y, base - 0.05))
    for lower, upper in zip(layers, layers[1:]):
        for k in range(segments):
            j = (k + 1) % segments
            gear.add([lower[k], lower[j], upper[j], upper[k]], "helmet", inside)
    crown = Vector((centre.x, centre.y - 0.02, top))
    for k in range(segments):
        gear.add([layers[-1][k], layers[-1][(k + 1) % segments], crown], "helmet", inside)
    # Sunglasses: wraparound lenses in front, thin arms back to the ears.
    rim = []
    for k in range(11):
        a = -1.5 + 3.0 * k / 10
        lens = abs(a) < 0.8
        point = Vector((centre.x + half_x * 1.05 * math.sin(a),
                        centre.y + half_y * 1.07 * math.cos(a), eyes))
        rim.append((point - Vector((0, 0, 0.02 if lens else 0.005)),
                    point + Vector((0, 0, 0.015 if lens else 0.005))))
    inside = Vector((centre.x, centre.y, eyes))
    for (a_low, a_high), (b_low, b_high) in zip(rim, rim[1:]):
        gear.add([a_low, b_low, b_high, a_high], "glasses", inside)
    if sex == "female":
        start = Vector((centre.x, centre.y - half_y * 1.08, eyes + 0.02))
        tip = start + Vector((0.0, -0.13, -0.15))
        axis = (tip - start).normalized()
        side = axis.cross(Vector((1, 0, 0))).normalized()
        across = axis.cross(side)
        around = [start + 0.04 * (math.cos(2 * math.pi * k / 6) * side
                                  + math.sin(2 * math.pi * k / 6) * across) for k in range(6)]
        inside = middle(start, tip)
        for k in range(6):
            gear.add([around[k], around[(k + 1) % 6], tip], "hair", inside)
        gear.add(around, "hair", inside)
    obj = gear.to_object("headgear", origin(head))
    obj.parent = head
    obj.matrix_parent_inverse = head.matrix_world.inverted()
    update()


def pelvis_bottom(parts):
    """The lowest point of the pelvis between the sit bones: where the saddle meets it."""
    centre = hips(parts)
    points = [p for p in world_vertices(parts["pelvis"]) if abs(p.x - centre.x) < 0.07]
    return min(points, key=lambda p: p.z)


def hoods_for(parts, saddle_top, arm):
    """Where the hoods go for the body as posed: the arms at `REACH`, the hoods
    `DROP_TO_HOODS` below the saddle. None if the arms cannot reach down that far."""
    shoulder = origin(parts["arm_upper.R"])
    wrist_z = saddle_top - DROP_TO_HOODS + WRIST_ON_HOOD.z
    across = HOODS_HALF_WIDTH - shoulder.x
    down = shoulder.z - wrist_z
    ahead = (REACH * arm) ** 2 - across ** 2 - down ** 2
    if ahead < 0.0:
        return None
    return Vector((0.0, shoulder.y + math.sqrt(ahead) - WRIST_ON_HOOD.y,
                   saddle_top - DROP_TO_HOODS))


def lean(parts, arm):
    """Bends pelvis, belly and chest forward until the hoods end up far enough ahead (or the
    torso is as flat as it goes), then lifts the head to look down the road. Returns the
    saddle's top, the hoods and the torso's angle above horizontal."""
    pivot = hips(parts)
    total = 0.0
    step = math.radians(1.0)
    while True:
        torso = shoulders(parts) - hips(parts)
        angle = math.atan2(torso.z, torso.y)
        saddle_top = pelvis_bottom(parts).z
        hoods = hoods_for(parts, saddle_top, arm)
        ready = hoods is not None and hoods.y >= HOODS_AHEAD
        if (angle <= TORSO_ANGLES[0] and ready) or angle <= TORSO_ANGLES[1]:
            break
        # Turning about −x tips the top towards +y, forward.
        transform(parts["pelvis"], Matrix.Rotation(-0.4 * step, 3, "X"), pivot)
        transform(parts["belly"], Matrix.Rotation(-0.3 * step, 3, "X"), origin(parts["belly"]))
        transform(parts["chest"], Matrix.Rotation(-0.3 * step, 3, "X"), origin(parts["chest"]))
        total += step
    if hoods is None:
        raise RuntimeError("the arms cannot reach the hoods")
    transform(parts["neck"], Matrix.Rotation(0.4 * total, 3, "X"), origin(parts["neck"]))
    transform(parts["head"], Matrix.Rotation(0.45 * total, 3, "X"), origin(parts["head"]))
    return saddle_top, hoods, math.degrees(angle)


def reach_hoods(parts, hoods):
    """Puts the hands on the hoods, elbows bent out and down."""
    for side, sign in (("L", -1.0), ("R", 1.0)):
        upper, lower, hand = (parts[f"{p}.{side}"] for p in ("arm_upper", "arm_lower", "hand"))
        shoulder = origin(upper)
        wrist = Vector((sign * HOODS_HALF_WIDTH, hoods.y, hoods.z)) + WRIST_ON_HOOD
        first = (origin(lower) - shoulder).length
        second = (origin(hand) - origin(lower)).length
        elbow = two_bone(shoulder, wrist, first, second, Vector((sign * 0.5, -0.3, -1.0)))
        turn = (origin(lower) - shoulder).rotation_difference(elbow - shoulder)
        transform(upper, turn.to_matrix(), shoulder)
        start = origin(lower)
        turn = (origin(hand) - start).rotation_difference(wrist - start)
        transform(lower, turn.to_matrix(), start)
        # The hand bends down over the hood.
        transform(hand, Matrix.Rotation(math.radians(-25.0), 3, "X"), origin(hand))


def baked(obj):
    """A copy of the object's mesh in world coordinates."""
    mesh = obj.data.copy()
    mesh.transform(obj.matrix_world)
    if obj.matrix_world.determinant() < 0.0:
        mesh.flip_normals()
    return mesh


def leg_nodes(parts):
    """Each leg part as its own node: origin at its joint, thigh and shin running down −z with
    their front to +y (Godot: down −y, front −z), the foot level."""
    nodes = []
    for side in ("L", "R"):
        chain = [parts[f"{p}.{side}"] for p in LEGS]
        joints = [origin(obj) for obj in chain]
        for k, (part, obj) in enumerate(zip(LEGS, chain)):
            mesh = baked(obj)
            mesh.transform(Matrix.Translation(-joints[k]))
            if k + 1 < len(chain):
                along = (joints[k + 1] - joints[k]).normalized()
                mesh.transform(along.rotation_difference(Vector((0, 0, -1))).to_matrix().to_4x4())
            mesh.name = f"{LEG_NODES[part]}_{side.lower()}"
            node = bpy.data.objects.new(mesh.name, mesh)
            node.matrix_world = Matrix.Translation(joints[k])
            bpy.context.scene.collection.objects.link(node)
            nodes.append(node)
    for obj in [parts[f"{p}.{s}"] for p in LEGS for s in ("L", "R")]:
        bpy.data.objects.remove(obj)
    update()
    return nodes


def join(objects, name):
    """One object from several, in world coordinates, with one slot per material name."""
    joined = bmesh.new()
    names = []
    for obj in objects:
        mesh = baked(obj)
        slots = []
        for slot in mesh.materials:
            if slot.name not in names:
                names.append(slot.name)
            slots.append(names.index(slot.name))
        before = len(joined.faces)
        joined.from_mesh(mesh)
        joined.faces.ensure_lookup_table()
        for k in range(before, len(joined.faces)):
            joined.faces[k].material_index = slots[joined.faces[k].material_index]
        bpy.data.meshes.remove(mesh)
    data = bpy.data.meshes.new(name)
    joined.to_mesh(data)
    joined.free()
    for n in names:
        data.materials.append(material(n))
    data.shade_flat()
    for obj in objects:
        bpy.data.objects.remove(obj)
    result = bpy.data.objects.new(name, data)
    bpy.context.scene.collection.objects.link(result)
    return result


# --- the bike --------------------------------------------------------------------------------


def wheel(name, hub):
    """A wheel in the y–z plane: a faceted tyre and rim, a hub and a few spokes, so it shows
    turning."""
    faces = Faces()
    segments = 18

    def at(radius, angle, x):
        return hub + Vector((x, radius * math.cos(angle), radius * math.sin(angle)))

    for outer, inner, half, mat in ((WHEEL_RADIUS, WHEEL_RADIUS - 0.03, 0.014, "tyre"),
                                    (WHEEL_RADIUS - 0.03, WHEEL_RADIUS - 0.06, 0.01, "rim")):
        for k in range(segments):
            a0, a1 = 2 * math.pi * k / segments, 2 * math.pi * (k + 1) / segments
            inside = at((outer + inner) / 2.0, (a0 + a1) / 2.0, 0.0)
            for radius in (outer, inner):
                faces.add([at(radius, a0, -half), at(radius, a1, -half), at(radius, a1, half),
                           at(radius, a0, half)], mat, inside)
            for x in (-half, half):
                faces.add([at(inner, a0, x), at(inner, a1, x), at(outer, a1, x),
                           at(outer, a0, x)], mat, inside)
    for k in range(8):
        angle = 2 * math.pi * k / 8
        faces.tube(at(0.03, angle, 0.0), at(WHEEL_RADIUS - 0.058, angle, 0.0), 0.006, "metal",
                   sides=3, caps=False)
    faces.tube(hub - Vector((0.05, 0, 0)), hub + Vector((0.05, 0, 0)), 0.03, "metal")
    return faces.to_object(name, hub)


def saddle_outline(back, nose, z):
    """The saddle's outline at height `z`: wide at the back, narrow at the nose."""
    shape = [(-0.07, 0.0), (0.07, 0.0), (0.06, 0.06), (0.025, 0.17), (0.02, 0.26),
             (-0.02, 0.26), (-0.025, 0.17), (-0.06, 0.06)]
    return [Vector((x, back + y / 0.26 * (nose - back), z)) for x, y in shape]


def bike(saddle, hoods):
    """Frame, wheels, crankset and pedals around the rider's contact points: the saddle's top
    middle and the hoods (both in the y–z plane). Returns the objects and the hubs."""
    seat_axis = Vector((0.0, -math.cos(SEAT_ANGLE), math.sin(SEAT_ANGLE)))
    steer = Vector((0.0, math.cos(HEAD_ANGLE), -math.sin(HEAD_ANGLE)))  # down the steerer
    frame = Faces()

    # Bars: the tops a little below the hoods' top, a level stem back to the steerer.
    bar = Vector((0.0, hoods.y - 0.08, hoods.z - 0.01))
    steerer_top = bar + Vector((0.0, -STEM, 0.01))
    head_top = steerer_top + steer * 0.03
    crown_z = WHEEL_RADIUS + FORK_LENGTH * math.sin(HEAD_ANGLE)
    head_length = max((head_top.z - crown_z) / math.sin(HEAD_ANGLE), 0.08)
    head_bottom = head_top + steer * head_length
    # The steering axis meets hub height; the hub sits the rake ahead of it.
    to_hub = (head_bottom.z - WHEEL_RADIUS) / math.sin(HEAD_ANGLE)
    front_hub = head_bottom + steer * to_hub + Vector((0.0, FORK_RAKE / math.sin(HEAD_ANGLE), 0.0))
    front_hub.z = WHEEL_RADIUS
    rear_hub = Vector((0.0, -math.sqrt(CHAINSTAY ** 2 - (WHEEL_RADIUS - BOTTOM_BRACKET.z) ** 2),
                       WHEEL_RADIUS))

    # The top tube slopes a little; the seat tube ends well below the saddle.
    cluster_z = min(head_top.z - 0.03, saddle.z - 0.16)
    cluster = BOTTOM_BRACKET + seat_axis * ((cluster_z - BOTTOM_BRACKET.z) / seat_axis.z)
    post_top = BOTTOM_BRACKET + seat_axis * ((saddle.z - 0.05 - BOTTOM_BRACKET.z) / seat_axis.z)

    frame.tube(BOTTOM_BRACKET + Vector((-0.04, 0, 0)), BOTTOM_BRACKET + Vector((0.04, 0, 0)),
               0.025, "frame")
    frame.tube(BOTTOM_BRACKET, cluster + seat_axis * 0.02, 0.019, "frame")
    frame.tube(BOTTOM_BRACKET, head_bottom - steer * 0.02, 0.024, "frame")
    frame.tube(cluster, head_top + steer * 0.02, 0.018, "frame")
    frame.tube(head_top - steer * 0.01, head_bottom, 0.025, "frame")
    for sign in (-1.0, 1.0):
        side = Vector((sign, 0.0, 0.0))
        frame.tube(BOTTOM_BRACKET + side * 0.03, rear_hub + side * 0.06, 0.011, "frame")
        frame.tube(cluster + side * 0.015, rear_hub + side * 0.06, 0.01, "frame")
        # Fork blades: down the steering axis, then bent forward to the hub.
        bend = head_bottom + steer * (to_hub * 0.55) + side * 0.05
        frame.tube(head_bottom + side * 0.03, bend, 0.013, "frame")
        frame.tube(bend, front_hub + side * 0.05, 0.011, "frame")
    frame.tube(cluster, post_top, 0.012, "metal")
    frame.tube(post_top, Vector((0.0, saddle.y, saddle.z - 0.03)), 0.01, "metal")
    frame.tube(steerer_top, head_top, 0.017, "metal")
    frame.tube(steerer_top, bar, 0.014, "bar")
    frame.tube(bar + Vector((-HOODS_HALF_WIDTH, 0, 0)), bar + Vector((HOODS_HALF_WIDTH, 0, 0)),
               0.013, "bar")
    for sign in (-1.0, 1.0):
        corner = bar + Vector((sign * HOODS_HALF_WIDTH, 0.0, 0.0))
        hood = Vector((corner.x, hoods.y, hoods.z - 0.03))
        bend = hood + Vector((0.0, 0.03, -0.08))
        drop = hood + Vector((0.0, 0.0, -0.13))
        frame.tube(corner, hood, 0.013, "bar")
        frame.tube(hood, bend, 0.013, "bar")
        frame.tube(bend, drop, 0.013, "bar")
        frame.tube(drop, drop + Vector((0.0, -0.09, 0.0)), 0.013, "bar")
        # The brake hood, where the hand rests.
        frame.box(hood + Vector((0.0, 0.0, 0.012)), (0.03, 0.07, 0.04), "bar")
    back = saddle.y - 0.11
    frame.prism(saddle_outline(back, back + 0.26, saddle.z - 0.04),
                saddle_outline(back, back + 0.26, saddle.z), "saddle")

    crankset = Faces()
    bb = BOTTOM_BRACKET
    ring = [Vector((0.0, 0.1 * math.cos(2 * math.pi * k / 16), 0.1 * math.sin(2 * math.pi * k / 16)))
            for k in range(16)]
    crankset.prism([bb + p + Vector((0.045, 0, 0)) for p in ring],
                   [bb + p + Vector((0.052, 0, 0)) for p in ring], "metal")
    crankset.tube(bb + Vector((-0.07, 0, 0)), bb + Vector((0.07, 0, 0)), 0.012, "metal")
    for sign in (-1.0, 1.0):
        # The right crank forward, the left one back: the app turns them from there.
        end = bb + Vector((sign * (PEDAL_OFFSET - 0.04), sign * CRANK, 0.0))
        crankset.tube(bb + Vector((sign * 0.06, 0, 0)), end, 0.013, "metal", sides=4)
    pedals = []
    for name, sign in (("pedal_l", -1.0), ("pedal_r", 1.0)):
        pedal = Faces()
        axle = bb + Vector((sign * PEDAL_OFFSET, sign * CRANK, 0.0))
        pedal.box(axle, (0.08, 0.07, 0.02), "bar")
        pedals.append(pedal.to_object(name, axle))
    objects = [frame.to_object("frame", Vector()), wheel("wheel_rear", rear_hub),
               wheel("wheel_front", front_hub), crankset.to_object("crankset", bb), *pedals]
    return objects, rear_hub, front_hub


# --- export ----------------------------------------------------------------------------------


def godot(v):
    return [round(v.x, 4), round(v.z, 4), round(-v.y, 4)]


def triangles(objects):
    return sum(len(p.vertices) - 2 for o in objects for p in o.data.polygons)


def build(sex):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    parts = load_body(sex)
    proportion(parts, sex)
    tailor(parts)
    thigh, shin, ball = measure_legs(parts)
    arm = ((origin(parts["arm_lower.R"]) - origin(parts["arm_upper.R"])).length
           + (origin(parts["hand.R"]) - origin(parts["arm_lower.R"])).length)

    # The hips on the seat tube's line, where the knee bends `KNEE_AT_BOTTOM` with the pedal at
    # the bottom and the ball of the foot on it.
    ankle_over_pedal = Vector((0.0, -ball.y, 0.012 - ball.z))
    ankle = (BOTTOM_BRACKET + Vector((0.0, 0.0, -CRANK))
             + Matrix.Rotation(FOOT_AT_BOTTOM, 3, "X") @ ankle_over_pedal)
    seat_axis = Vector((0.0, -math.cos(SEAT_ANGLE), math.sin(SEAT_ANGLE)))
    offset = BOTTOM_BRACKET - ankle
    leg = math.sqrt(thigh ** 2 + shin ** 2 + 2.0 * thigh * shin * math.cos(KNEE_AT_BOTTOM))
    along = -seat_axis.dot(offset) + math.sqrt(
        seat_axis.dot(offset) ** 2 - offset.length_squared + leg * leg)

    pelvis = parts["pelvis"]
    transform(pelvis, Matrix.Rotation(math.pi, 3, "Z"), origin(pelvis))
    move(pelvis, BOTTOM_BRACKET + seat_axis * along - hips(parts))
    for obj in [parts[f"{p}.{s}"] for p in LEGS for s in ("L", "R")]:
        world = obj.matrix_world.copy()
        obj.parent = None
        obj.matrix_world = world
    update()
    headgear(parts["head"], sex)
    saddle_top, hoods, torso_angle = lean(parts, arm)
    reach_hoods(parts, hoods)
    saddle = Vector((0.0, pelvis_bottom(parts).y, saddle_top))
    bike_objects, rear_hub, front_hub = bike(saddle, hoods)

    # The origin between the wheels.
    shift = Vector((0.0, -(rear_hub.y + front_hub.y) / 2.0, 0.0))
    for obj in list(bpy.context.scene.objects):
        if obj.parent is None:
            move(obj, shift)
    legs = leg_nodes(parts)
    body = join([o for o in bpy.context.scene.objects
                 if o.type == "MESH" and o not in legs and o not in bike_objects], "body")

    os.makedirs(OUT, exist_ok=True)
    bpy.ops.export_scene.gltf(filepath=os.path.join(OUT, f"rider_{sex}.glb"),
                              export_format="GLB", export_yup=True, export_apply=True)
    print(f"built rider_{sex}: {triangles([body, *legs])} rider and {triangles(bike_objects)} "
          f"bike triangles; torso {torso_angle:.0f}° above horizontal; saddle "
          f"{saddle_top:.3f} m high, hoods {hoods.y - saddle.y:.3f} m ahead of it and "
          f"{hoods.y:.3f} m ahead of the bottom bracket; wheelbase "
          f"{front_hub.y - rear_hub.y:.3f} m")
    by_name = {o.name: o for o in legs}
    return {
        "hips": [godot(origin(by_name["thigh_l"])), godot(origin(by_name["thigh_r"]))],
        "thigh": round(thigh, 4),
        "shin": round(shin, 4),
        "ankle_over_pedal": godot(ankle_over_pedal),
        "wheel_radius": WHEEL_RADIUS,
        "wheel_rear": godot(rear_hub + shift),
        "wheel_front": godot(front_hub + shift),
        "bottom_bracket": godot(BOTTOM_BRACKET + shift),
        "crank": CRANK,
        "pedal_offset": PEDAL_OFFSET,
    }


def main():
    manifest = {sex: build(sex) for sex in ("female", "male")}
    with open(os.path.join(OUT, "riders.json"), "w", encoding="utf-8") as file:
        json.dump(manifest, file, indent=1)
        file.write("\n")


main()
