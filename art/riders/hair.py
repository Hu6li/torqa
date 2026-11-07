"""Gathered hair volumes, with broad tresses rather than individual strands (R89)."""

import math

from mathutils import Vector


def scalp(gear, skull, tree, eyes, centre_x):
    """A close scalp cap with an even hairline, leaving the ears and neck exposed."""
    cy = (min(p.y for p in skull) + max(p.y for p in skull)) / 2
    centre = Vector((centre_x, cy, eyes + 0.01))
    width = max(abs(p.x - centre_x) for p in skull)
    depth = (max(p.y for p in skull) - min(p.y for p in skull)) / 2
    height = max(p.z for p in skull) - centre.z
    around, rows = 64, 14
    rings, roots = [], []
    for row in range(rows):
        ring, root = [], []
        t = row / rows
        for side in range(around):
            theta = math.tau * side / around
            front, back = max(math.cos(theta), 0.0), max(-math.cos(theta), 0.0)
            # Hair sweeps above the ear, then down to the nape behind it.
            rim = eyes - 0.025 + 0.08 * front ** 2 - 0.05 * back ** 2
            rim += 0.018 * abs(math.sin(theta)) ** 8
            rim += 0.001 * math.sin(7 * theta) + 0.0006 * math.sin(13 * theta)
            phi = math.atan2(rim - centre.z, depth)
            phi += (math.pi / 2 - phi) * t
            direction = Vector((width * math.sin(theta) * math.cos(phi),
                                depth * math.cos(theta) * math.cos(phi),
                                height * math.sin(phi))).normalized()
            hit = tree.ray_cast(centre + direction * 0.4, -direction)
            if hit[0] is None:
                raise RuntimeError("hair cap missed the scalp")
            point, normal = hit[:2]
            # Compress under the helmet; shallow swept ridges catch a broad highlight.
            thickness = 0.0035 + 0.0025 * (1 - t) ** 2
            thickness += 0.0007 * math.cos(12 * theta + 2.5 * t) * (1 - t)
            ring.append(point + normal * thickness)
            root.append(point + normal * 0.0005)
        rings.append(ring)
        roots.append(root)
    for first, second in zip(rings, rings[1:]):
        for side in range(around):
            other = (side + 1) % around
            gear.add([first[side], first[other], second[other], second[side]], "hair", centre)
    crown = Vector((centre_x, cy, max(p.z for p in skull) + 0.0035))
    for side in range(around):
        other = (side + 1) % around
        gear.add([rings[-1][side], rings[-1][other], crown], "hair", centre)
        gear.add([roots[0][side], roots[0][other], rings[0][other], rings[0][side]],
                 "hair", centre)


def _sample(values, t):
    """Catmull–Rom interpolation keeps a tress tangent continuous through its bends."""
    along = t * (len(values) - 1)
    index = min(int(along), len(values) - 2)
    u = along - index
    a, b, c, d = (values[max(0, min(k, len(values) - 1))]
                  for k in (index - 1, index, index + 1, index + 2))
    return tuple(0.5 * (2 * q + (-p + r) * u + (2 * p - 5 * q + 4 * r - s) * u ** 2
                       + (-p + 3 * q - 3 * r + s) * u ** 3)
                 for p, q, r, s in zip(a, b, c, d))


def _tress(gear, origin, sections, rows=36, sides=24, phase=0.0):
    rings, centres = [], []
    for row in range(rows):
        t = row / (rows - 1)
        x, y, z, width, depth = _sample(sections, t)
        centre = origin + Vector((x, y, z))
        before = Vector(_sample(sections, max(t - 0.001, 0.0))[:3])
        after = Vector(_sample(sections, min(t + 0.001, 1.0))[:3])
        tangent = (after - before).normalized()
        # A fixed lateral reference avoids the old tube's frame flipping at steep bends.
        across = Vector((1, 0, 0))
        across = (across - tangent * tangent.dot(across)).normalized()
        deep = tangent.cross(across)
        ring = []
        for side in range(sides):
            angle = math.tau * side / sides
            relief = 1 + 0.055 * math.cos(5 * angle + phase + 1.3 * t)
            relief += 0.025 * math.sin(9 * angle - 2 * t + phase)
            ring.append(centre + relief * (across * width * math.cos(angle)
                                          + deep * depth * math.sin(angle)))
        rings.append(ring)
        centres.append(centre)
    for row, (first, second) in enumerate(zip(rings, rings[1:])):
        inside = (centres[row] + centres[row + 1]) / 2
        for side in range(sides):
            other = (side + 1) % sides
            gear.add([first[side], first[other], second[other], second[side]], "hair", inside)
    gear.add(list(reversed(rings[0])), "hair", centres[1])
    gear.add(rings[-1], "hair", centres[-2])


def ponytail(gear, nape):
    """A tied bundle that opens below the tie and falls into unequal, tapered ends."""
    _tress(gear, nape, (
        (0, 0.008, 0, 0.014, 0.012),
        (0, -0.018, -0.007, 0.014, 0.012),
        (0.002, -0.055, -0.028, 0.022, 0.017),
        (0.005, -0.095, -0.068, 0.024, 0.018),
        (0.004, -0.122, -0.117, 0.020, 0.014),
        (-0.002, -0.134, -0.165, 0.013, 0.009),
        (-0.009, -0.132, -0.204, 0.005, 0.003),
        (-0.012, -0.125, -0.22, 0.0008, 0.0006),
    ))
    # These end locks stay within the bundle above, separating only near its tips.
    for sign, length in ((-1, 0.195), (1, 0.207)):
        _tress(gear, nape, (
            (sign * 0.01, -0.105, -0.09, 0.010, 0.008),
            (sign * 0.014, -0.128, -0.14, 0.009, 0.007),
            (sign * 0.015, -0.137, -0.18, 0.006, 0.004),
            (sign * 0.018, -0.14, -length, 0.0007, 0.0005),
        ), rows=18, sides=12, phase=sign)
    axis = Vector((0, -0.026, -0.007)).normalized()
    tie = nape + Vector((0, -0.013, -0.006))
    gear.sweep([tie + axis * offset for offset in (-0.004, -0.002, 0.002, 0.004)],
               [(0.030, 0.026), (0.032, 0.028), (0.032, 0.028), (0.030, 0.026)],
               "sleeve", sides=24)
