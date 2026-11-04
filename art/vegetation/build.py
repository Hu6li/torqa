"""Builds the vegetation models — trees, bushes, ferns and rocks — and exports them for Godot
(ADR 0009, ADR 0011).

Run in the art container:
    scripts/art.sh blender --background --factory-startup --python art/vegetation/build.py

The look is realistic (skill orqwa-look, ADR 0020): smooth, tapered trunks with bark; crowns of
many foliage cards — sprays of needles drooping in whorls for conifers, clusters of leaves for
broadleaf trees and bushes — whose normals point out of the crown so it is lit as a whole;
weathered, fractured boulders. Each model is one mesh with named materials (`trunk`, `leaves`, `needles`,
`frond`, `rock`); the app textures and colours them — foliage and rocks per instance from the
palette — so the files carry shape, texture coordinates, normals and material names. `models.json` beside them lists
each model's kind and height for the world to choose and scale them. Pass model names after
`--` to build only those, and `--preview DIR` to render them there instead of exporting
(art/preview.py). Random shapes use fixed seeds, so rebuilds give the same files.
"""

import json
import math
import os
import random
import sys

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.normpath(os.path.join(HERE, "..", "..", "app", "assets", "models", "vegetation"))
sys.path.insert(0, os.path.dirname(HERE))

import preview  # noqa: E402  (needs the path above)

# Preview colours in Blender only; the app replaces the materials by name.
PREVIEW = {"leaves": (0.25, 0.36, 0.12), "needles": (0.15, 0.25, 0.1), "frond": (0.3, 0.42, 0.14),
           "trunk": (0.3, 0.22, 0.16), "rock": (0.45, 0.44, 0.42)}


class Mesh:
    """Faces with material names, built up piece by piece (z up, metres). Each face may carry
    texture coordinates and normals per corner; faces without normals are shaded smooth within
    their `group` (a trunk, a rock), with their neighbours of the same group."""

    def __init__(self):
        self.verts = []
        self.faces = []
        self.materials = []
        self.uvs = []
        self.normals = []
        self.groups = []
        self._group = 0

    def new_group(self):
        self._group += 1
        return self._group

    def face(self, points, material, uvs=None, normals=None, group=None):
        start = len(self.verts)
        self.verts.extend(points)
        self.faces.append(list(range(start, start + len(points))))
        self.materials.append(material)
        self.uvs.append(uvs or [(0.0, 0.0)] * len(points))
        self.normals.append(normals)
        self.groups.append(self._group if group is None else group)

    def corner_normals(self):
        """One normal per face corner: the face's own where given, else smooth across the
        group's faces meeting at that point."""
        def face_normal(points):
            (ax, ay, az), (bx, by, bz), (cx, cy, cz) = points[0], points[1], points[2]
            ux, uy, uz = bx - ax, by - ay, bz - az
            vx, vy, vz = cx - ax, cy - ay, cz - az
            return (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx)

        def key(point, group):
            return (round(point[0], 3), round(point[1], 3), round(point[2], 3), group)

        sums = {}
        for face, group, given in zip(self.faces, self.groups, self.normals):
            if given is not None:
                continue
            n = face_normal([self.verts[i] for i in face])
            for i in face:
                k = key(self.verts[i], group)
                total = sums.get(k, (0.0, 0.0, 0.0))
                sums[k] = (total[0] + n[0], total[1] + n[1], total[2] + n[2])
        corners = []
        for face, group, given in zip(self.faces, self.groups, self.normals):
            if given is not None:
                corners.extend(unit(n) for n in given)
            else:
                corners.extend(unit(sums[key(self.verts[i], group)]) for i in face)
        return corners


def unit(v):
    length = math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) or 1.0
    return (v[0] / length, v[1] / length, v[2] / length)


def ring(centre, radius, sides, turn, height):
    return [
        (centre[0] + radius * math.cos(turn + 2 * math.pi * i / sides),
         centre[1] + radius * math.sin(turn + 2 * math.pi * i / sides),
         height)
        for i in range(sides)
    ]


ICO_FACES = [(0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11), (1, 5, 9), (5, 11, 4),
             (11, 10, 2), (10, 7, 6), (7, 1, 8), (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8),
             (3, 8, 9), (4, 9, 5), (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1)]


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


def trunk(mesh, height, r_base, r_top, sides=8, segments=5, lean=0.0, start=(0.0, 0.0)):
    """A smooth, tapered trunk from below the ground to `height`, curving over by `lean`
    metres at its top; texture coordinates in metres around and up (for the bark). Returns the
    top's centre."""
    group = mesh.new_group()

    def centre_at(t):
        return (start[0] + lean * t * t, start[1], -0.6 + (height + 0.6) * t)

    for k in range(segments):
        t0, t1 = k / segments, (k + 1) / segments
        (x0, y0, z0), (x1, y1, z1) = centre_at(t0), centre_at(t1)
        r0 = r_base + (r_top - r_base) * t0
        r1 = r_base + (r_top - r_base) * t1
        low = ring((x0, y0), r0, sides, 0.0, z0)
        high = ring((x1, y1), r1, sides, 0.0, z1)
        around = 2 * math.pi * (r0 + r1) / 2
        for i in range(sides):
            j = i + 1
            u0, u1 = around * i / sides, around * j / sides
            mesh.face([low[i], low[j % sides], high[j % sides], high[i]], "trunk",
                      uvs=[(u0, -z0), (u1, -z0), (u1, -z1), (u0, -z1)], group=group)
    return centre_at(1.0)


def branch(mesh, base, tip, r_base, r_top, sides=5):
    """A tapered limb from `base` to `tip`, smooth, bark mapped like the trunk."""
    group = mesh.new_group()
    axis = (tip[0] - base[0], tip[1] - base[1], tip[2] - base[2])
    length = math.sqrt(sum(c * c for c in axis))
    a = unit(axis)
    helper = (0.0, 0.0, 1.0) if abs(a[2]) < 0.9 else (1.0, 0.0, 0.0)
    side = unit((a[1] * helper[2] - a[2] * helper[1], a[2] * helper[0] - a[0] * helper[2],
                 a[0] * helper[1] - a[1] * helper[0]))
    other = (a[1] * side[2] - a[2] * side[1], a[2] * side[0] - a[0] * side[2],
             a[0] * side[1] - a[1] * side[0])

    def circle(centre, radius):
        return [tuple(centre[c] + radius * (math.cos(2 * math.pi * i / sides) * side[c]
                                            + math.sin(2 * math.pi * i / sides) * other[c])
                      for c in range(3)) for i in range(sides)]

    low, high = circle(base, r_base), circle(tip, r_top)
    around = 2 * math.pi * (r_base + r_top) / 2
    for i in range(sides):
        j = i + 1
        u0, u1 = around * i / sides, around * j / sides
        mesh.face([low[i], low[j % sides], high[j % sides], high[i]], "trunk",
                  uvs=[(u0, 0.0), (u1, 0.0), (u1, -length), (u0, -length)], group=group)


def card(mesh, base, out, up, length, width, material, normal):
    """A foliage card standing at `base`, reaching `length` along `out` and `width` across
    (`up` sets which way across is), its texture's stalk at the base and tip at the far end.
    Every corner gets `normal`, so the crown is lit as one soft volume."""
    o = unit(out)
    across = unit((o[1] * up[2] - o[2] * up[1], o[2] * up[0] - o[0] * up[2],
                   o[0] * up[1] - o[1] * up[0]))
    half = width / 2
    a = tuple(base[c] - across[c] * half for c in range(3))
    b = tuple(base[c] + across[c] * half for c in range(3))
    c_ = tuple(base[c] + across[c] * half + o[c] * length for c in range(3))
    d = tuple(base[c] - across[c] * half + o[c] * length for c in range(3))
    # Godot's v runs down the image: the stalk is drawn at the bottom (v = 1).
    mesh.face([a, b, c_, d], material, uvs=[(0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)],
              normals=[normal] * 4)


def conifer_reach(spec, z):
    """Crown envelope narrowing continuously to the leader, without a broad shelf at its tip."""
    remaining = (spec["height"] - z) / (spec["height"] - spec["crown_base"])
    return spec["radius"] * remaining ** 0.85 + 0.35 * min((spec["height"] - z) / 2.6, 1.0)


def conifer_leader(mesh, spec):
    """Needle planes sharing one upright shoot and a single tapered apex at every LOD."""
    height = spec["height"]
    base = height - 2.6
    width = conifer_reach(spec, base)
    for k in range(3):
        heading = k * math.pi / 3
        across = (math.cos(heading) * width, math.sin(heading) * width)
        # Splayed rectangular sprays make competing tops; all planes end on the trunk axis.
        mesh.face([(-across[0], -across[1], base), (across[0], across[1], base),
                   (0.0, 0.0, height)], "needles",
                  uvs=[(0.0, 1.0), (1.0, 1.0), (0.5, 0.0)],
                  normals=[(0.0, 0.0, 1.0)] * 3)


def conifer(spec):
    """A spruce: a straight trunk to near the top, whorls of needle sprays reaching out and
    drooping, long at the bottom and short at the top, beneath one upright leader."""
    mesh = Mesh()
    rng = random.Random(spec["seed"])
    height, crown_base, radius = spec["height"], spec["crown_base"], spec["radius"]
    # The stem supports the upper whorls and ends inside the leader's needles.
    trunk(mesh, height - 0.2, 0.28 * radius / 2.5, 0.015)
    z = crown_base
    whorl = 0
    while z < height - 0.8:
        t = (z - crown_base) / (height - crown_base)
        reach = conifer_reach(spec, z)
        count = spec.get("sprays", 9)
        turn = whorl * 0.9
        for k in range(count):
            heading = turn + 2 * math.pi * k / count + rng.uniform(-0.2, 0.2)
            droop = rng.uniform(0.25, 0.55) * (1.0 - 0.5 * t)
            out = (math.cos(heading), math.sin(heading), -droop)
            normal = unit((math.cos(heading), math.sin(heading), 0.6))
            length = reach * rng.uniform(0.85, 1.1)
            card(mesh, (0.0, 0.0, z), out, (0.0, 0.0, 1.0), length, length * 0.9, "needles",
                 normal)
        z += rng.uniform(0.45, 0.65) * (1.0 - 0.3 * t)
        whorl += 1
    conifer_leader(mesh, spec)
    return mesh, height


def crown(mesh, centre, radii, count, size, material, rng):
    """`count` foliage cards spread through an ellipsoid crown, most near its surface, each
    facing roughly outward with a normal from the crown's middle (tilted up)."""
    for _ in range(count):
        while True:
            x, y, z = (rng.uniform(-1, 1) for _ in range(3))
            r = x * x + y * y + z * z
            if 0.25 < r <= 1.0:
                break
        point = (centre[0] + x * radii[0], centre[1] + y * radii[1], centre[2] + z * radii[2])
        outward = unit((x, y, z + 0.3))
        out = unit((outward[0] + rng.uniform(-0.6, 0.6), outward[1] + rng.uniform(-0.6, 0.6),
                    outward[2] * 0.5 + rng.uniform(-0.3, 0.5)))
        up = unit((rng.uniform(-1, 1), rng.uniform(-1, 1), 1.0))
        length = size * rng.uniform(0.8, 1.2)
        start = tuple(point[c] - out[c] * length * 0.45 for c in range(3))
        card(mesh, start, out, up, length, length, material, unit((x, y, z + 0.5)))


def broadleaf(spec):
    """A broadleaf tree: a trunk forking into a few limbs that reach into a crown of leaf
    clusters."""
    mesh = Mesh()
    rng = random.Random(spec["seed"])
    top = trunk(mesh, spec["trunk"], 0.3, 0.2)
    height = 0.0
    for k, (x, y, z, radius, squash) in enumerate(spec["crowns"]):
        for limb in range(3):
            heading = 2 * math.pi * limb / 3 + k + rng.uniform(-0.3, 0.3)
            tip = (x + math.cos(heading) * radius * 0.6, y + math.sin(heading) * radius * 0.6,
                   z + radius * squash * rng.uniform(-0.1, 0.4))
            branch(mesh, (top[0], top[1], top[2] - 0.3), tip, 0.14, 0.04)
        count = int(spec.get("density", 9) * radius * radius)
        crown(mesh, (x, y, z), (radius, radius, radius * squash), count, 1.7, "leaves", rng)
        height = max(height, z + radius * squash + 0.6)
    return mesh, height


def bush(spec):
    """A bush: clumps of leaf clusters from the ground up."""
    mesh = Mesh()
    rng = random.Random(spec["seed"])
    height = 0.0
    for x, y, z, radius, squash in spec["clumps"]:
        crown(mesh, (x, y, z), (radius * 1.15, radius, radius * squash), int(14 * radius * radius),
              0.9, "leaves", rng)
        height = max(height, z + radius * squash + 0.3)
    return mesh, height


def rock(spec):
    """Fractured boulders: asymmetric masses cut by weathered planes, sunk into the
    ground a little so slopes do not show their undersides."""
    mesh = Mesh()
    height = 0.0
    for k, (x, y, z, radius, scale) in enumerate(spec["blocks"]):
        boulder(mesh, (x, y, z), radius, scale, spec["seed"] + k)
        height = max(height, z + radius * scale[2] * 1.2)
    return mesh, height


def boulder(mesh, centre, radius, scale, seed):
    group = mesh.new_group()
    t = (1 + 5 ** 0.5) / 2
    corners = [unit(c) for c in [(-1, t, 0), (1, t, 0), (-1, -t, 0), (1, -t, 0), (0, -1, t),
                                 (0, 1, t), (0, -1, -t), (0, 1, -t), (t, 0, -1), (t, 0, 1),
                                 (-t, 0, -1), (-t, 0, 1)]]
    triangles = [tuple(corners[i] for i in face) for face in ICO_FACES]
    for _ in range(2):
        finer = []
        for a, b, c in triangles:
            ab, bc, ca = (unit(tuple((p[i] + q[i]) / 2 for i in range(3)))
                          for p, q in ((a, b), (b, c), (c, a)))
            finer += [(a, ab, ca), (ab, b, bc), (ca, bc, c), (ab, bc, ca)]
        triangles = finer
    rng = random.Random(seed)
    lumps = [(unit((rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-1, 1))),
              rng.uniform(-0.4, 0.45)) for _ in range(14)]
    # Planes the stone broke along: flat faces with rounded edges.
    cuts = [(unit((rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-0.3, 1))),
             rng.uniform(0.52, 0.88)) for _ in range(7)]

    def place(p):
        bump = sum(amount * max(0.0, p[0] * d[0] + p[1] * d[1] + p[2] * d[2]) ** 2
                   for d, amount in lumps)
        q = [c * (1.0 + bump) for c in p]
        for d, depth in cuts:
            over = q[0] * d[0] + q[1] * d[1] + q[2] * d[2] - depth
            if over > 0:
                q = [q[i] - d[i] * over * 0.98 for i in range(3)]
        # A tilted crown and off-centre shoulders avoid a flattened-egg silhouette.
        q[0] += 0.14 * q[2] + 0.08 * q[1] * q[2]
        q[2] += 0.12 * q[1]
        k = radius * (1.0 - 0.12 * abs(p[2]))
        return (centre[0] + q[0] * k * scale[0], centre[1] + q[1] * k * scale[1],
                centre[2] + q[2] * k * scale[2])

    for a, b, c in triangles:
        face = [place(a), place(b), place(c)]
        if not outward(face, centre):
            face.reverse()
        mesh.face(face, "rock", group=group)


def both_sides(mesh, points, material):
    """A thin surface seen from either side (leaves): the face and its reverse, each smooth with
    its own side only (together their normals would cancel)."""
    mesh.face(list(points), material)
    mesh.face(list(reversed(points)), material, group=-mesh.groups[-1] - 1)


def leaf(mesh, base, heading, length, width, rise, droop, segments, material, fold=0.25,
         tip_width=0.0):
    """A long leaf from `base` out along `heading` (radians): first rising `rise`, then
    drooping `droop` towards its tip, `width` wide where it leaves the stem and `tip_width`
    at its end, folded along its midrib (V-shaped, `fold` of its width deep) so its facets
    catch the light differently."""
    dx, dy = math.cos(heading), math.sin(heading)
    sx, sy = -dy, dx
    mesh.new_group()
    spine, edges = [], []
    for k in range(segments + 1):
        t = k / segments
        reach = length * t
        z = base[2] + rise * math.sin(math.pi * 0.5 * min(1.0, 2.0 * t)) - droop * t * t
        half = (width + (tip_width - width) * t) / 2.0 * (1.0 if 0 < k else 0.35)
        centre = (base[0] + dx * reach, base[1] + dy * reach, z)
        spine.append((centre[0], centre[1], z - fold * half))
        edges.append(((centre[0] + sx * half, centre[1] + sy * half, z),
                      (centre[0] - sx * half, centre[1] - sy * half, z)))
    for k in range(segments):
        (l0, r0), (l1, r1) = edges[k], edges[k + 1]
        both_sides(mesh, [spine[k], spine[k + 1], l1, l0], material)
        both_sides(mesh, [spine[k], r0, r1, spine[k + 1]], material)


def palm(spec):
    """A palm: a slender trunk, curving over a little for coconut palms, under a crown of
    long feathery fronds arching out and drooping, with a few coconuts; or for fan palms a
    straight trunk under a ball of stiff fans."""
    mesh = Mesh()
    rng = random.Random(spec["seed"])
    top = trunk(mesh, spec["height"], spec.get("base", 0.28), spec.get("top", 0.18), sides=7,
                segments=spec.get("segments", 4) + 2, lean=spec.get("lean", 0.0))
    fronds = spec["fronds"]
    if spec.get("fans"):
        for k in range(fronds):
            heading = 2 * math.pi * k / fronds + rng.uniform(-0.2, 0.2)
            tilt = rng.uniform(-0.3, 0.6)
            dx, dy = math.cos(heading), math.sin(heading)
            stalk = 0.9
            hub = (top[0] + dx * stalk, top[1] + dy * stalk, top[2] + tilt * stalk)
            # A fan: a half disc of a few facets, standing out and up from the stalk's end.
            fan = [hub]
            lift = spec["leaf"] * (0.45 + tilt * 0.5)
            for i in range(5):
                spread = i / 4.0 - 0.5
                a = heading + spread * 2.0
                fan.append((hub[0] + math.cos(a) * spec["leaf"],
                            hub[1] + math.sin(a) * spec["leaf"],
                            hub[2] + lift * math.cos(spread)))
            for i in range(1, 5):
                both_sides(mesh, [fan[0], fan[i], fan[i + 1]], "frond")
    else:
        for k in range(fronds):
            heading = 2 * math.pi * k / fronds + rng.uniform(-0.25, 0.25)
            length = spec["leaf"] * rng.uniform(0.85, 1.1)
            leaf(mesh, (top[0], top[1], top[2] - 0.1), heading, length, 1.4, 0.7,
                 rng.uniform(2.8, 3.6), 4, "frond", tip_width=0.2)
        # A short upright tuft of young fronds in the middle.
        tuft = spec.get("tuft", 3)
        for k in range(tuft):
            heading = 2 * math.pi * k / tuft + 0.5
            leaf(mesh, top, heading, spec["leaf"] * 0.45, 0.5, 1.4, 0.2, 2, "frond")
        for k in range(spec.get("nuts", 0)):
            a = 2 * math.pi * k / spec["nuts"]
            boulder(mesh, (top[0] + 0.32 * math.cos(a), top[1] + 0.32 * math.sin(a),
                           top[2] - 0.45), 0.2, (1.0, 1.0, 1.1), spec["seed"] + 100 + k)
    return mesh, max(v[2] for v in mesh.verts)


def banana(spec):
    """A banana plant: a few stout green stems under big paddle leaves rising and arching
    out."""
    mesh = Mesh()
    rng = random.Random(spec["seed"])
    stem = spec["stem"]
    trunk(mesh, stem + 0.2, 0.2, 0.13, sides=7, segments=3)
    for k in range(spec["leaves"]):
        heading = 2 * math.pi * k / spec["leaves"] + rng.uniform(-0.3, 0.3)
        leaf(mesh, (0.0, 0.0, stem - 0.3 * (k % 3)), heading,
             spec["leaf"] * rng.uniform(0.8, 1.1), 1.0, 1.3, rng.uniform(0.8, 1.4), 4, "frond",
             fold=0.15, tip_width=0.6)
    return mesh, max(v[2] for v in mesh.verts)


def fern(spec):
    """A crown of arching fronds, each with paired, tapered leaflets and a narrow rachis.
    Leaflets have a raised midrib so their two sides catch the light differently."""
    mesh = Mesh()
    rng = random.Random(spec["seed"])
    for k in range(spec["fronds"]):
        heading = k * 2 * math.pi / spec["fronds"] + rng.uniform(-0.18, 0.18)
        reach = spec["reach"] * rng.uniform(0.8, 1.1)
        height = spec["height"] * rng.uniform(0.85, 1.1)
        dx, dy = math.cos(heading), math.sin(heading)
        sx, sy = -dy, dx

        def spine(t):
            return (dx * reach * t ** 1.35, dy * reach * t ** 1.35,
                    -0.06 + height * math.sin(math.pi * 0.82 * t))

        mesh.new_group()
        for j in range(10):
            t0, t1 = j / 10, (j + 1) / 10
            a, b = spine(t0), spine(t1)
            w0, w1 = 0.006 * (1 - 0.9 * t0), 0.006 * (1 - 0.9 * t1)
            both_sides(mesh, [(a[0] - sx * w0, a[1] - sy * w0, a[2]),
                              (a[0] + sx * w0, a[1] + sy * w0, a[2]),
                              (b[0] + sx * w1, b[1] + sy * w1, b[2]),
                              (b[0] - sx * w1, b[1] - sy * w1, b[2])], "frond")
        for j in range(spec["pairs"]):
            t = 0.18 + j * 0.78 / spec["pairs"]
            length = spec["width"] * math.sin(math.pi * t) ** 0.8 * (1 - 0.35 * t)
            for side in (-1, 1):
                base = spine(t + side * 0.008)
                angle = heading + side * rng.uniform(0.95, 1.25)
                lx, ly = math.cos(angle), math.sin(angle)
                cross = (-ly, lx)
                tip = (base[0] + lx * length, base[1] + ly * length, base[2] - length * 0.12)
                mid = (base[0] + lx * length * 0.4, base[1] + ly * length * 0.4,
                       base[2] + length * 0.07)
                half = length * 0.18
                left = (mid[0] + cross[0] * half, mid[1] + cross[1] * half, mid[2] - half * 0.5)
                right = (mid[0] - cross[0] * half, mid[1] - cross[1] * half, mid[2] - half * 0.5)
                mesh.new_group()
                both_sides(mesh, [base, left, mid], "frond")
                both_sides(mesh, [left, tip, mid], "frond")
                both_sides(mesh, [mid, tip, right], "frond")
                both_sides(mesh, [base, mid, right], "frond")
    return mesh, max(v[2] for v in mesh.verts)


def tropical_bush(spec):
    """A broadleaf shrub of the tropics: a dense clump with big leaves fanning out of it."""
    mesh, height = bush(spec)
    rng = random.Random(spec["seed"] + 7)
    for k in range(spec["blades"]):
        heading = 2 * math.pi * k / spec["blades"] + rng.uniform(-0.3, 0.3)
        leaf(mesh, (0.0, 0.0, height * 0.4), heading, spec["blade"], 0.6, 0.4, 0.6, 3,
             "frond", fold=0.2, tip_width=0.15)
    return mesh, max(height, max(v[2] for v in mesh.verts))


KINDS = {"conifer": conifer, "broadleaf": broadleaf, "bush": bush, "rock": rock, "palm": palm,
         "banana": banana, "tropical_bush": tropical_bush, "fern": fern}

CATALOGUE = [
    {"name": "conifer_tall", "kind": "conifer", "seed": 5, "height": 14.0, "crown_base": 1.6,
     "radius": 2.6},
    {"name": "conifer_broad", "kind": "conifer", "seed": 7, "height": 10.4, "crown_base": 1.0,
     "radius": 3.0, "sprays": 10},
    {"name": "broadleaf_round", "kind": "broadleaf", "trunk": 3.6, "seed": 11,
     "crowns": [(0.0, 0.0, 5.6, 3.2, 0.9)]},
    {"name": "broadleaf_cluster", "kind": "broadleaf", "trunk": 3.4, "seed": 23,
     "crowns": [(0.6, 0.3, 4.9, 2.4, 0.85), (-0.8, -0.4, 5.3, 2.3, 0.85),
                (0.1, -0.1, 6.9, 2.0, 0.9)]},
    {"name": "bush", "kind": "bush", "seed": 37,
     "clumps": [(0.0, 0.0, 0.55, 1.05, 0.75), (0.75, 0.45, 0.4, 0.8, 0.7)]},
    {"name": "fern_spreading", "kind": "fern", "seed": 83, "fronds": 7, "pairs": 10,
     "height": 0.55, "reach": 0.85, "width": 0.36},
    {"name": "fern_upright", "kind": "fern", "seed": 89, "fronds": 8, "pairs": 9,
     "height": 0.78, "reach": 0.6, "width": 0.3},
    {"name": "rock_block", "kind": "rock", "seed": 41,
     "blocks": [(0.0, 0.0, 0.25, 1.0, (1.3, 1.0, 0.75))]},
    {"name": "rock_pair", "kind": "rock", "seed": 53,
     "blocks": [(0.0, 0.0, 0.2, 0.9, (1.2, 1.0, 0.7)), (1.1, 0.5, 0.0, 0.6, (1.1, 1.0, 0.8))]},
    # The tropics (#136): palms, banana plants and broadleaf shrubs.
    {"name": "palm_coconut", "kind": "palm", "seed": 61, "height": 9.5, "lean": 1.6,
     "fronds": 8, "leaf": 3.6, "nuts": 2, "tuft": 2},
    {"name": "palm_short", "kind": "palm", "seed": 67, "height": 5.5, "lean": 0.5, "base": 0.32,
     "top": 0.22, "fronds": 7, "leaf": 3.0, "nuts": 2},
    {"name": "palm_fan", "kind": "palm", "seed": 71, "height": 6.5, "base": 0.3, "top": 0.24,
     "fronds": 9, "leaf": 1.5, "fans": True},
    {"name": "banana", "kind": "banana", "seed": 73, "stem": 2.2, "leaves": 6, "leaf": 2.2},
    {"name": "tropical_bush", "kind": "tropical_bush", "seed": 79, "blades": 6, "blade": 1.7,
     "clumps": [(0.0, 0.0, 0.6, 1.0, 0.8), (0.7, -0.4, 0.45, 0.75, 0.75)]},
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
    data.polygons.foreach_set("use_smooth", [True] * len(mesh.faces))
    layer = data.uv_layers.new(name="UVMap")
    layer.data.foreach_set("uv", [c for uvs in mesh.uvs for uv in uvs for c in uv])
    # Blender's v runs up the image, the cards' coordinates were written as Godot reads them.
    for loop in layer.data:
        loop.uv = (loop.uv[0], 1.0 - loop.uv[1])
    data.normals_split_custom_set(mesh.corner_normals())
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
    wanted, preview_dir = preview.options()
    os.makedirs(OUT, exist_ok=True)
    manifest = {}
    for spec in CATALOGUE:
        if wanted and spec["name"] not in wanted:
            continue
        bpy.ops.wm.read_factory_settings(use_empty=True)
        mesh, height = KINDS[spec["kind"]](spec)
        obj = to_object(mesh, spec["name"])
        if preview_dir:
            preview.render(obj, preview_dir, spec["name"])
            continue
        export(obj, os.path.join(OUT, spec["name"] + ".glb"))
        manifest[spec["name"]] = {"kind": spec["kind"], "height": round(height, 2)}
        triangles = sum(len(face) - 2 for face in mesh.faces)
        print(f"built {spec['name']}: {triangles} triangles, {height:.1f} m")
    if preview_dir:
        return
    path = os.path.join(OUT, "models.json")
    if wanted and os.path.exists(path):
        with open(path, encoding="utf-8") as file:
            manifest = {**json.load(file)["models"], **manifest}
    with open(path, "w", encoding="utf-8") as file:
        json.dump({"models": dict(sorted(manifest.items()))}, file, indent=1)
        file.write("\n")


if __name__ == "__main__":
    main()
