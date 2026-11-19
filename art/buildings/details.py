"""Nearby Ultra additions; the ordinary catalogue geometry remains the default."""

import math

from mathutils import Vector

ENABLED = False
FOOTPRINT = None
GLAZING_BARS = True


def glazing(mesh, facade, inner, front, glass, frame):
    """Separate panes keep curtains at window scale even on long office ribbons."""
    u0, u1, z0, z1 = inner
    count = max(1, math.ceil((u1 - u0) / 1.25))
    p = facade.point
    for index in range(count):
        left = u0 + (u1 - u0) * index / count
        right = u0 + (u1 - u0) * (index + 1) / count
        centre = p((left + right) / 2.0, (z0 + z1) / 2.0)
        seed = math.sin(centre.x * 12.9898 + centre.y * 78.233 + centre.z * 37.719) * 43758.5453
        mesh.facing([p(left, z0, glass), p(right, z0, glass),
                     p(right, z1, glass), p(left, z1, glass)], facade.out, "glass_detail",
                    uvs=[(0, 0), (1, 0), (1, 1), (0, 1)])
        mesh.detail_uvs[-1] = [(seed % 1.0, 0.0)] * 4
        if index:
            mesh.beam(p(left, z0, front), p(left, z1, front), 0.04, 0.04,
                      frame, up=facade.out)
    if mesh.glazing_bars and count == 1 and u1 - u0 > 0.7:
        mid = (u0 + u1) / 2.0
        mesh.beam(p(mid, z0, front), p(mid, z1, front), 0.04, 0.04,
                  frame, up=facade.out)
    if mesh.glazing_bars and z1 - z0 > 1.25:
        z = z0 + (z1 - z0) * 0.68
        mesh.beam(p(u0, z, front), p(u1, z, front), 0.04, 0.04, frame)


def drainage(mesh, rect, eaves, pitch, overhang, verge=None):
    """Only drain the main roof: dormers, entrance canopies and castle tiers keep their forms."""
    footprint = mesh.detail_footprint
    if footprint is None or not (math.isclose(rect.length, footprint[0])
                                 and math.isclose(rect.width, footprint[1])):
        return
    l, w = rect.length / 2.0, rect.width / 2.0
    edge = eaves - overhang * math.tan(math.radians(pitch)) + 0.10
    reach = overhang if verge is None else verge
    x, y = l + reach, w + overhang + 0.04
    for side in (-1.0, 1.0):
        gutter(mesh, (-x, side * y, edge), (x, side * y, edge))
        if verge is None:
            gutter(mesh, (side * x, -y, edge), (side * x, y, edge))
        # Opposite corners avoid unnecessary pipes across front entrances and balconies.
        px, py = side * (l - 0.16), side * (w + 0.13)
        points = [(px, side * y, edge), (px, side * y, edge - 0.22),
                  (px, py, edge - 0.65), (px, py, 0.22), (px, py + side * 0.15, 0.10)]
        for a, b in zip(points, points[1:]):
            mesh.cylinder(a, b, 0.052, 6, "metal")
        for z in range(1, max(1, int(edge - 0.7)), 2):
            mesh.beam((px, side * w, z), (px, py, z), 0.065, 0.065, "metal")


def gutter(mesh, start, end):
    """An open semicircular metal channel, with enough sides for its small silhouette."""
    a, b = Vector(start), Vector(end)
    along = (b - a).normalized()
    across = along.cross(Vector((0, 0, 1))).normalized()
    ring = [across * (0.085 * math.cos(i * math.pi / 6))
            - Vector((0, 0, 0.085 * math.sin(i * math.pi / 6))) for i in range(7)]
    for left, right in zip(ring, ring[1:]):
        normal = (left + right).normalized()
        mesh.facing([a + left, a + right, b + right, b + left], normal, "metal")
        mesh.facing([a + left, a + right, b + right, b + left], -normal, "metal")
