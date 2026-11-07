"""The riders' road-racing helmet (R225): a white, unbranded shell after the Lazer Z1
KinetiCore, fitted round the head.

Cutting vents into a fitted grid leaves straight walls and a cage of square windows. The
reference's long channels, rounded rib edges and recessed braces come from smooth booleans
on a signed distance field instead, meshed by OpenVDB (bundled with Blender) and thinned to
the triangle budget. The white shell and the dark foam inside the vents are then split
along a contour a little below the outer surface, so the material edge runs clean.

Coordinates: metres, x across the head, y forward, z up, relative to the skull's middle
across and along it and to the eyes' height.
"""

import bpy
import numpy as np
import openvdb
from mathutils import Vector

# The shell stands this far off the scalp (pressing the hair under it flat).
GAP = 0.007
# Shell thickness over the side, brow, back and crown.
THICKNESS = {"side": 0.019, "front": 0.03, "back": 0.031, "top": 0.023}
# Solid bands above the rim, which the vents end at: the brow, the sides and the back.
BANDS = {"front": 0.016, "side": 0.026, "back": 0.042}
# Rounding of the rim and of the vents' outer edges.
RIM_ROUND = 0.005
EDGE_ROUND = 0.0035
# Below the outer surface the polycarbonate skin ends and the dark foam shows.
SKIN = 0.0022
# Voxel size of the distance field, and the triangles kept after thinning.
VOXEL = 0.0006
TRIANGLES = 7000
# Squareness of the shell's sections (2 is an ellipsoid).
SQUARENESS = 2.3
FRONT_SQUARENESS = 2.0
BACK_SQUARENESS = 2.4
# The long axis the vents are laid out round: from a point behind the shell, high at the
# back (height above the eyes and distance behind), to one below the brow in front. Like the
# Z1's, the ribs then meet at the fit dial and fan out over the brow.
AXIS = {"back": 0.128, "behind": 0.03, "front": -0.03}


def rim_height(ahead):
    """The rim above the eyes: over the brow in front (`ahead` 1), above the ears at the
    sides (0), lower over the back of the head (−1)."""
    return 0.02 + 0.02 * ahead + 0.015 * (1.0 - ahead * ahead)


def smin(a, b, k):
    h = np.clip(k - np.abs(a - b), 0.0, None) / k
    return np.minimum(a, b) - h * h * k * 0.25


def smax(a, b, k):
    return -smin(-a, -b, k)


class Ovoid:
    """A squarish egg: semi-axes forward and back, up and down, and a width that grows
    towards the back. `distance` estimates the signed distance to its surface."""

    def __init__(self, middle, across, front, back, up, down, wider_back):
        self.middle, self.across = middle, across
        self.front, self.back, self.up, self.down = front, back, up, down
        self.wider_back = wider_back

    def grown(self, side, front, back, up, down):
        return Ovoid(self.middle, self.across + side, self.front + front, self.back + back,
                     self.up + up, self.down + down, self.wider_back)

    def scaled(self, factor):
        return Ovoid(self.middle, self.across * factor, self.front * factor,
                     self.back * factor, self.up * factor, self.down * factor,
                     self.wider_back)

    def radius(self, x, y, z):
        """The radius relative to the surface (1 on it) and its gradient."""
        dy, dz = y - self.middle[1], z - self.middle[2]
        long = np.where(dy > 0.0, self.front, self.back)
        high = np.where(dz > 0.0, self.up, self.down)
        width = self.across * (1.0 - self.wider_back * np.clip(dy / long, -1.0, 1.0))
        # Rounder over the brow, so the front runs low and long; squarer at the back.
        brow = np.clip(dy / long * 2.0, 0.0, 1.0) * np.clip(dz / high * 2.0, 0.0, 1.0)
        rear = np.clip(-dy / long * 2.0, 0.0, 1.0)
        n = (SQUARENESS + (FRONT_SQUARENESS - SQUARENESS) * brow * brow
             + (BACK_SQUARENESS - SQUARENESS) * rear * rear)
        terms = [np.abs(x) / width, np.abs(dy) / long, np.abs(dz) / high]
        total = sum(t ** n for t in terms)
        r = total ** (1.0 / n)
        scale = np.maximum(r, 1e-6) ** (1.0 - n)
        grad = [scale * t ** (n - 1.0) / s for t, s in zip(terms, (width, long, high))]
        return r, grad

    def distance(self, x, y, z):
        r, grad = self.radius(x, y, z)
        return (r - 1.0) / np.maximum(np.sqrt(sum(g * g for g in grad)), 1e-6)


class Helmet:
    """The shell's shape fitted to a head: `field` gives the signed distance to it and how
    far a point lies within the white skin (negative where the foam shows)."""

    def __init__(self, skull, half_y):
        self.half_y = half_y
        xs = np.array([p[0] for p in skull])
        ys = np.array([p[1] for p in skull])
        zs = np.array([p[2] for p in skull])
        covered = zs > self.rim(ys) - 0.005
        xs, ys, zs = xs[covered], ys[covered], zs[covered]
        # The widest section sits about where the rim runs above the ears.
        middle = (0.0, (ys.max() + ys.min()) / 2.0, rim_height(0.0))
        inner = Ovoid(middle, np.abs(xs).max() + GAP, ys.max() - middle[1] + GAP,
                      middle[1] - ys.min() + GAP, zs.max() - middle[2] + GAP, 0.11, 0.16)
        # Every covered point of the head at least `GAP` inside the shell.
        r, _ = inner.radius(np.abs(xs), ys, zs)
        self.inner = inner.scaled(max(1.0, float(r.max())))
        t = THICKNESS
        self.outer = self.inner.grown(t["side"], t["front"], t["back"], t["top"], t["side"])
        back = np.array((0.0, self.outer.middle[1] - self.outer.back - AXIS["behind"],
                         AXIS["back"]))
        front = np.array((0.0, self.outer.middle[1] + self.outer.front, AXIS["front"]))
        self.along_axis = (front - back) / np.linalg.norm(front - back)
        self.up_axis = np.array((0.0, -self.along_axis[2], self.along_axis[1]))
        self.axis = back - self.along_axis * (back[1] / self.along_axis[1])
        self.dial = self.surface(-self.along_axis)

    def rim(self, y):
        ahead = np.clip(y / self.half_y, -1.0, 1.0)
        return rim_height(ahead)

    def band(self, y):
        ahead = np.clip(y / self.half_y, -1.0, 1.0)
        b = BANDS
        return np.where(ahead > 0.0, b["side"] + (b["front"] - b["side"]) * ahead,
                        b["side"] + (b["back"] - b["side"]) * -ahead)

    def chart(self, x, y, z):
        """Angle round the long axis (0 on top), a conformal length along it, and the
        metres per chart unit there."""
        dz = z - self.axis[2]
        length = y * self.along_axis[1] + dz * self.along_axis[2]
        height = y * self.up_axis[1] + dz * self.up_axis[2]
        across = np.abs(x)
        rho = np.maximum(np.hypot(across, height), 1e-4)
        return np.arctan2(across, height), np.arcsinh(length / rho), rho

    def surface(self, direction):
        """Where a ray from the axis point leaves the outer surface."""
        near, far = 0.0, 0.4
        for _ in range(40):
            mid = (near + far) / 2.0
            p = self.axis + direction * mid
            inside = self.outer.distance(*(np.array([v]) for v in p))[0] < 0.0
            near, far = (mid, far) if inside else (near, mid)
        return self.axis + direction * near

    def field(self, x, y, z):
        outer = self.outer.distance(x, y, z)
        inner = self.inner.distance(x, y, z)
        solid = smax(smax(outer, -inner, 0.002), self.rim(y) - z, RIM_ROUND)
        alpha, along, rho = self.chart(x, y, z)
        vents = np.full_like(solid, 1.0)
        for corners, rounding in VENTS:
            vents = np.minimum(vents, (polygon(alpha, along, corners) - rounding) * rho)
        # The back's exhausts are drawn as seen from behind, on a plane behind the head, and
        # cut along rays from the head's middle so they pass through the back only.
        dx, dy, dz = np.abs(x), y - REAR_CENTRE[1], z - REAR_CENTRE[2]
        behind = np.maximum(-dy, 1e-4)
        u, v = REAR_PLANE * dx / behind, REAR_CENTRE[2] + REAR_PLANE * dz / behind
        scale = np.sqrt(dx * dx + dy * dy + dz * dz) / REAR_PLANE
        facing = 0.6 * np.sqrt(dx * dx + dz * dz) - behind
        for corners, rounding in REAR_VENTS:
            rear = smax((polygon(u, v, corners) - rounding) * scale, facing, 0.004)
            vents = np.minimum(vents, rear)
        # The vents end where the bands above the rim begin.
        vents = smax(vents, self.rim(y) + self.band(y) - z, 0.006)
        depth = -outer
        cut = smax(solid, -vents, EDGE_ROUND)
        white = SKIN - depth
        # Braces recessed below the skin bridge the vents; their tops are white too.
        for corners, below in BRACES:
            span = polygon(alpha, along, corners) * rho
            brace = smax(np.maximum(span, outer + below), solid, 0.001)
            cut = smin(cut, brace, 0.0015)
            top = np.minimum(np.minimum(SKIN - np.abs(depth - below), 0.0005 - span), -vents)
            white = np.maximum(white, top)
        # The fit dial's roller in a notch where the ribs meet at the back.
        dial = self.dial
        inward = self.along_axis
        offset = np.stack((x - dial[0], y - dial[1], z - dial[2]))
        across = np.abs(offset[0]) - NOTCH[0]
        deep = np.abs(offset[1] * inward[1] + offset[2] * inward[2]) - NOTCH[2]
        up = np.abs(offset[1] * self.up_axis[1] + offset[2] * self.up_axis[2]) - NOTCH[1]
        notch = np.maximum(np.maximum(across, deep), up)
        cut = smax(cut, -notch, 0.002)
        axle = dial + inward * DIAL[0]
        roller = smax(np.hypot(y - axle[1], z - axle[2]) - DIAL[1], np.abs(x) - DIAL[2], 0.002)
        cut = smin(cut, roller, 0.001)
        white = np.minimum(white, roller - 0.0006)
        return cut, white


def polygon(px, py, corners):
    """Signed distance to a polygon in the plane, negative inside."""
    nearest = np.full_like(px, np.inf)
    inside = np.zeros(px.shape, dtype=bool)
    for (ax, ay), (bx, by) in zip(corners, corners[1:] + corners[:1]):
        ex, ey = bx - ax, by - ay
        wx, wy = px - ax, py - ay
        t = np.clip((wx * ex + wy * ey) / (ex * ex + ey * ey), 0.0, 1.0)
        dx, dy = wx - ex * t, wy - ey * t
        nearest = np.minimum(nearest, dx * dx + dy * dy)
        up, down, left = py >= ay, py < by, ex * wy > ey * wx
        inside ^= (up & down & left) | (~up & ~down & ~left)
    return np.where(inside, -1.0, 1.0) * np.sqrt(nearest)


# The layout follows the Z1, on the chart round the long axis: the angle round it (0 along
# the top, the side rim near 1.3, π down the back from the dial) and the length along it
# (the brow near 1.2, the dial at its far end). Vents: the corners of their cores and how
# much they are rounded; the bands above the rim end them.
VENTS = (
    # Either side of the spine, inside the ring that meets at the dial.
    ([(0.17, -2.2), (0.40, -2.6), (0.41, 0.5), (0.24, 1.6), (0.19, 1.6)], 0.035),
    # Three long vents down each side, ending before the dial.
    ([(0.59, -2.3), (0.69, -2.4), (0.70, 1.6), (0.60, 1.6)], 0.035),
    ([(0.87, -2.1), (0.96, -2.2), (0.97, 1.6), (0.88, 1.6)], 0.035),
    ([(1.13, -1.9), (1.22, -2.0), (1.23, 1.6), (1.14, 1.6)], 0.035),
    # The low side vent from the temple, above the band.
    ([(1.37, -0.7), (1.45, -0.8), (1.46, 1.6), (1.38, 1.6)], 0.035),
    # The intake under the brow arch and the window in the spine.
    ([(0.0, 0.55), (0.06, 0.55), (0.06, 1.6), (0.0, 1.6)], 0.035),
    ([(0.0, -1.1), (0.01, -1.1), (0.01, -0.7), (0.0, -0.7)], 0.025),
)
# Exhausts in the back, drawn as seen from behind (across and up, metres) on a plane
# `REAR_PLANE` behind `REAR_CENTRE`.
REAR_VENTS = (
    # Beside the spine below the dial, widening upwards, and a slot in each shoulder.
    ([(0.024, 0.050), (0.046, 0.050), (0.070, 0.100), (0.052, 0.126), (0.026, 0.126)], 0.005),
    ([(0.080, 0.060), (0.089, 0.056), (0.101, 0.094), (0.092, 0.100)], 0.004),
)
REAR_CENTRE = (0.0, 0.0, 0.05)
REAR_PLANE = 0.15
# Braces deep in the vents: corners in the chart and depth below the skin.
BRACES = (
    ([(0.10, -1.42), (0.46, -1.12), (0.46, -1.00), (0.10, -1.30)], 0.013),
    ([(0.10, -0.22), (0.46, -0.02), (0.46, 0.10), (0.10, -0.10)], 0.013),
    ([(0.55, -1.66), (0.74, -1.66), (0.74, -1.54), (0.55, -1.54)], 0.013),
    ([(0.55, -0.44), (0.74, -0.44), (0.74, -0.32), (0.55, -0.32)], 0.013),
    ([(0.83, -1.14), (1.01, -1.14), (1.01, -1.02), (0.83, -1.02)], 0.013),
    ([(0.83, 0.16), (1.01, 0.16), (1.01, 0.28), (0.83, 0.28)], 0.013),
    ([(1.09, -0.84), (1.27, -0.84), (1.27, -0.72), (1.09, -0.72)], 0.013),
)
# The fit dial at the back, where the ribs meet: how deep its axle sits, its radius and
# half length, and the notch round it (half sizes across, up and deep).
DIAL = (0.007, 0.0075, 0.011)
NOTCH = (0.014, 0.011, 0.012)


def mesh_field(helmet):
    """The shell's points (local coordinates) and faces, from its distance field."""
    outer = helmet.outer
    margin = 0.004
    low = np.array((-(outer.across * (1.0 + outer.wider_back)) - margin,
                    outer.middle[1] - outer.back - margin,
                    float(helmet.rim(np.array([-helmet.half_y]))[0]) - margin))
    high = np.array((-low[0], outer.middle[1] + outer.front + margin,
                     outer.middle[2] + outer.up + margin))
    counts = np.ceil((high - low) / VOXEL).astype(int) + 1
    # Symmetric across, so the mesh is too.
    low[0] = -(counts[0] - 1) * VOXEL / 2.0
    xs, ys, zs = (low[i] + VOXEL * np.arange(counts[i]) for i in range(3))
    values = np.empty(counts, dtype=np.float32)
    for i, x in enumerate(xs):
        y, z = np.meshgrid(ys, zs, indexing="ij")
        values[i] = helmet.field(np.full_like(y, x), y, z)[0]
    grid = openvdb.FloatGrid(VOXEL * 4.0)
    grid.copyFromArray(values)
    points, triangles, quads = grid.convertToPolygons(isovalue=0.0, adaptivity=0.0)
    points = points.astype(np.float64) * VOXEL + low
    faces = [list(f) for f in triangles] + [list(f) for f in quads]
    return points, faces


def thinned(points, faces, target):
    """Collapses edges down to about `target` triangles, keeping the shell symmetric."""
    mesh = bpy.data.meshes.new("helmet_dense")
    mesh.from_pydata([tuple(p) for p in points], [], faces)
    obj = bpy.data.objects.new("helmet_dense", mesh)
    bpy.context.scene.collection.objects.link(obj)
    count = sum(len(p.vertices) - 2 for p in mesh.polygons)
    thin = obj.modifiers.new("thin", "DECIMATE")
    thin.ratio = min(1.0, target / count)
    thin.use_symmetry = True
    thin.symmetry_axis = "X"
    evaluated = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
    result = evaluated.to_mesh()
    result.calc_loop_triangles()
    vertices = np.array([v.co[:] for v in result.vertices])
    tris = [list(t.vertices) for t in result.loop_triangles]
    evaluated.to_mesh_clear()
    bpy.data.objects.remove(obj)
    bpy.data.meshes.remove(mesh)
    return vertices, tris


def canonical(points, faces):
    """Vertices and faces in an order that depends only on where they are."""
    order = np.lexsort(np.round(points, 7).T[::-1])
    index = np.empty(len(order), dtype=int)
    index[order] = np.arange(len(order))
    faces = [[int(index[v]) for v in f] for f in faces]
    faces = [f[f.index(min(f)):] + f[:f.index(min(f))] for f in faces]
    return points[order], sorted(faces)


def split(points, triangles, white):
    """Triangles cut along the zero contour of `white`, each with its material."""
    points = [Vector(p) for p in points]
    crossings = {}

    def crossing(a, b):
        key = (min(a, b), max(a, b))
        if key not in crossings:
            t = white[a] / (white[a] - white[b])
            crossings[key] = len(points)
            points.append(points[a].lerp(points[b], t))
        return crossings[key]

    faces = []
    for tri in triangles:
        side = [white[v] >= 0.0 for v in tri]
        if all(side) or not any(side):
            faces.append((tri, "helmet" if side[0] else "vents"))
            continue
        # Turn the triangle so its lone corner comes first.
        lone = next(k for k in range(3) if side.count(side[k]) == 1)
        a, b, c = tri[lone:] + tri[:lone]
        ab, ac = crossing(a, b), crossing(a, c)
        mine, theirs = ("helmet", "vents") if side[lone] else ("vents", "helmet")
        faces.append(([a, ab, ac], mine))
        faces.append(([ab, b, c], theirs))
        faces.append(([ab, c, ac], theirs))
    return points, faces


def road_helmet(gear, skull, eyes, centre, half_y):
    """Adds the helmet to `gear`, fitted to the head's points `skull` (world) round its
    middle `centre` (across and along) at the eyes' height."""
    origin = Vector((centre.x, centre.y, eyes))
    helmet = Helmet([tuple(p - origin) for p in skull], half_y)
    points, faces = mesh_field(helmet)
    points, faces = canonical(points, faces)
    points, triangles = thinned(points, faces, TRIANGLES)
    points, triangles = canonical(points, triangles)
    _, white = helmet.field(points[:, 0], points[:, 1], points[:, 2])
    # The contour must not run through a corner.
    white = np.where(np.abs(white) < 1e-7, 1e-7, white)
    corners, faces = split(points, triangles, white)
    for tri, material_name in faces:
        gear.add([corners[k] + origin for k in tri], material_name)
