"""Japanese starter set: tiled timber houses and tiered plaster castle keeps.

These are regional scenery archetypes, not reconstructions of specific monuments.
"""

import math

from mathutils import Vector

from kit import UP, Mesh, Opening, Rect, door, gable_roof, wall, window


def beam_on_wall(mesh, facade, u0, u1, z, width=0.11):
    mesh.beam(facade.point(u0, z, -0.04), facade.point(u1, z, -0.04),
              width, width, "wood_dark")


def tiled_roof(mesh, rect, eaves, pitch=28, overhang=0.9, hipped=False):
    """A gently swept ceramic roof with closed soffits, fascia and raised ridge caps.

    Four slope bands put the subtle eave sweep in the silhouette; shared metre UVs keep
    the existing tile material at its physical scale.
    """
    l, w = rect.length / 2, rect.width / 2
    slope = math.tan(math.radians(pitch))
    reach, span = l + overhang, w + overhang
    half_ridge = max(l - w, 0.0) if hipped else reach
    steps = (0.0, overhang / span, 0.32, 0.62, 1.0)
    thickness = 0.18

    def roof_z(y):
        fraction = abs(y) / span
        # The sweep must pass through the wall plate, or daylight shows above the wall.
        sweep = 0.18 * (fraction ** 6 - (w / span) ** 5 * fraction)
        return eaves + (w - abs(y)) * slope + sweep

    def ring(t, lift=0.0):
        x = reach + (half_ridge - reach) * t
        y = span * (1 - t)
        z = roof_z(y) + lift
        return [Vector((sx * x, sy * y, z))
                for sx, sy in ((-1, -1), (1, -1), (1, 1), (-1, 1))]

    sides = range(4) if hipped else (0, 2)
    for a, b in zip(steps, steps[1:]):
        for lift, material, direction in ((thickness, "tiles", 1), (0.0, "wood_dark", -1)):
            outer, inner = ring(a, lift), ring(b, lift)
            for k in sides:
                j = (k + 1) % 4
                points = [outer[k], outer[j], inner[j], inner[k]]
                unique = []
                for point in points:
                    if all((point - p).length > 1e-6 for p in unique):
                        unique.append(point)
                mesh.facing(unique, UP * direction, material)

    low, high = ring(0), ring(0, thickness)
    for k in range(4):
        j = (k + 1) % 4
        outward = Vector((low[k].x + low[j].x, low[k].y + low[j].y, 0)).normalized()
        mesh.facing([low[k], low[j], high[j], high[k]], outward, "wood_dark")

    ridge = ring(1, thickness)[0].z
    if half_ridge > 0.01:
        mesh.cylinder((-half_ridge - 0.08, 0, ridge + 0.07),
                      (half_ridge + 0.08, 0, ridge + 0.07), 0.12, 8, "tiles")
    if hipped:
        for k in range(4):
            for a, b in zip(steps, steps[1:]):
                mesh.beam(ring(a, thickness + 0.05)[k], ring(b, thickness + 0.05)[k],
                          0.18, 0.09, "tiles")
    else:
        # Gable walls stand at the footprint, beneath the roof's projecting verge.
        for end in (-1, 1):
            points = [Vector((end * l, -w, eaves)), Vector((end * l, w, eaves))]
            for y in (w, w * 0.7, w * 0.4, 0, -w * 0.4, -w * 0.7, -w):
                points.append(Vector((end * l, y, roof_z(y))))
            mesh.facing(points, (end, 0, 0), "plaster")
            for side in (-1, 1):
                for a, b in zip(steps, steps[1:]):
                    k = 1 if side < 0 else 2
                    points = [ring(a)[k], ring(b)[k],
                              ring(b, thickness)[k], ring(a, thickness)[k]]
                    for point in points:
                        point.x = end * reach
                    mesh.facing(points, (end, 0, 0), "wood_dark")
        # Rounded tile rolls run down the gable slopes, visible at riding distance.
        count = max(1, int(2 * reach / 0.6))
        for i in range(count + 1):
            x = -reach + 2 * reach * i / count
            for side in (-1, 1):
                for a, b in zip(steps, steps[1:]):
                    k = 1 if side < 0 else 2
                    p, q = ring(a, thickness + 0.025)[k], ring(b, thickness + 0.025)[k]
                    p.x, q.x = x, x
                    mesh.cylinder(p, q, 0.035, 4, "tiles", caps=False)
    return ridge + 0.19


def facade_windows(mesh, rect, bottom, top, castle=False, entrance=False):
    """Recessed glazing and dark timber mullions, without European shutters or flower boxes."""
    for side, facade in enumerate(rect.facades()):
        bays = max(1, int(facade.length / (3.2 if castle else 2.9)))
        spacing = facade.length / bays
        openings, doors = [], []
        for bay in range(bays):
            u = (bay + 0.5) * spacing
            if entrance and side == 0 and bay == bays // 2:
                doors.append(Opening(u - 0.7, u + 0.7, bottom, bottom + 2.2, depth=0.22))
            else:
                half = min(spacing * 0.32, 0.55 if castle else 0.88)
                openings.append(Opening(u - half, u + half, bottom + 0.85,
                                        min(bottom + 2.15, top - 0.35), depth=0.2))
        wall(mesh, facade, bottom, top, "plaster", openings + doors)
        for opening in openings:
            window(mesh, facade, opening, frame="wood_dark", sill=None)
            bars = 2 if castle else 4
            for k in range(1, bars + 1):
                u = opening.u0 + opening.width * k / (bars + 1)
                mesh.beam(facade.point(u, opening.z0, 0.12),
                          facade.point(u, opening.z1, 0.12), 0.045, 0.055, "wood_dark")
        for opening in doors:
            door(mesh, facade, opening, leaf="wood_dark", step="stone")
        if not castle:
            beam_on_wall(mesh, facade, 0, facade.length, top - 0.13, 0.16)
            beam_on_wall(mesh, facade, 0, facade.length, bottom + 0.35)
            for u in (0.06, facade.length - 0.06):
                mesh.beam(facade.point(u, bottom, -0.04), facade.point(u, top, -0.04),
                          0.15, 0.15, "wood_dark")


def japanese_house(spec):
    rect = Rect(spec["length"], spec["width"])
    mesh = Mesh()
    floor, step = 0.35, 2.9
    eaves = floor + spec["storeys"] * step + 0.25
    for facade in rect.facades():
        wall(mesh, facade, -3.0, floor, "stone")
    for storey in range(spec["storeys"]):
        bottom = floor + storey * step
        top = bottom + step + (0.25 if storey + 1 == spec["storeys"] else 0)
        facade_windows(mesh, rect, bottom, top, entrance=storey == 0)
    # A shallow tiled canopy shades the ground-floor frontage on the two-storey house.
    if spec["storeys"] > 1:
        y, z = -rect.width / 2, floor + 2.45
        mesh.facing([(-rect.length / 2, y - 0.7, z), (rect.length / 2, y - 0.7, z),
                     (rect.length / 2, y, z + 0.25), (-rect.length / 2, y, z + 0.25)],
                    UP, "tiles")
        mesh.box((-rect.length / 2, y - 0.7, z - 0.1),
                 (rect.length / 2, y, z), "wood_dark")
    top = tiled_roof(mesh, rect, eaves, spec.get("pitch", 28),
                     hipped=spec.get("roof") == "hipped")
    return mesh, {"length": rect.length, "width": rect.width, "eaves": eaves,
                  "height": top, "storeys": spec["storeys"]}


def japanese_castle(spec):
    rect = Rect(spec["length"], spec["width"])
    mesh = Mesh()
    platform, step = 3.0, 3.4
    # Battered stone base, closed above and below, extending into sloping terrain.
    low = [Vector((x, y, -8.0)) for x, y in rect.corners()]
    ground = [Vector((x, y, 0.0)) for x, y in rect.corners()]
    high = [Vector((x * 0.88, y * 0.88, platform)) for x, y in rect.corners()]
    mesh.facing(low, -UP, "castle_stone")
    mesh.facing(high, UP, "castle_stone")
    for k in range(4):
        j = (k + 1) % 4
        out = Vector((low[k].x + low[j].x, low[k].y + low[j].y, 0))
        mesh.facing([low[k], low[j], ground[j], ground[k]], out, "castle_stone")
        mesh.facing([ground[k], ground[j], high[j], high[k]], out, "castle_stone")
    top = 0.0
    for tier in range(spec["storeys"]):
        shrink = 0.88 * 0.78 ** tier
        keep = Rect(rect.length * shrink, rect.width * shrink)
        bottom = platform + tier * step
        eaves = bottom + step
        facade_windows(mesh, keep, bottom, eaves, castle=True, entrance=tier == 0)
        top = max(top, tiled_roof(mesh, keep, eaves, spec.get("pitch", 28),
                                  hipped=tier + 1 < spec["storeys"]))
        if tier + 1 < spec["storeys"] and keep.width >= 8:
            # A projecting gable breaks the long lower roof, as on Japanese keeps.
            dormer = Mesh()
            dormer.box((-1.1, -1.5, -0.5), (1.1, 1.5, 0.55), "plaster", skip=("-z",))
            gable_roof(dormer, Rect(2.2, 3.0), 0.55, 35, overhang=0.25, verge=0.25,
                       roof="tiles", under="wood_dark", fascia="plaster")
            mesh.add(dormer, offset=(0, -keep.width / 2 + 0.65, eaves + 0.2), turn=90)
    return mesh, {"length": rect.length, "width": rect.width, "eaves": platform + step,
                  "height": top, "storeys": spec["storeys"]}
