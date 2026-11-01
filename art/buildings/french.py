"""French village houses and château inspired keeps."""

import math

from kit import (
    Facade, Mesh, Opening, Rect, UP, chimney, door, hipped_roof, wall, window,
)


def french_house(spec):
    """A plastered village house with stone base, shutters and a hipped tile or slate roof."""
    rect = Rect(spec["length"], spec["width"])
    mesh = Mesh()
    floor, step = 0.35, 2.9
    eaves = floor + spec["storeys"] * step + 0.25
    for side, facade in enumerate(rect.facades()):
        bays = max(2, int(facade.length / 3.0))
        openings, doors = [], []
        for level in range(spec["storeys"]):
            bottom = floor + level * step
            for bay in range(bays):
                centre = (bay + 0.5) * facade.length / bays
                if side == 0 and level == 0 and bay == bays // 3:
                    doors.append(Opening(centre - 0.55, centre + 0.55,
                                         bottom, bottom + 2.15, depth=0.2))
                else:
                    openings.append(Opening(centre - 0.52, centre + 0.52,
                                            bottom + 0.85, bottom + 2.15, depth=0.18))
        wall(mesh, facade, -3.0, floor, "stone")
        wall(mesh, facade, floor, eaves, "plaster", openings + doors)
        for opening in openings:
            window(mesh, facade, opening, shutters="shutter", sill="stone")
        for opening in doors:
            door(mesh, facade, opening, leaf="wood_dark", step="stone", canopy="tiles")

    roof = spec.get("cover", "slate")
    ridge = hipped_roof(mesh, rect, eaves, spec.get("pitch", 36), overhang=0.55, roof=roof)
    inset = min(rect.length, rect.width) * 0.3
    chimney(mesh, -rect.length * 0.2, rect.width * 0.18,
            eaves + inset * 0.73 - 0.35, ridge + 0.65, size=0.55, material="stone")
    return mesh, {"length": rect.length, "width": rect.width, "eaves": eaves,
                  "height": ridge + 0.9, "storeys": spec["storeys"]}


def _cone(mesh, x, y, z, radius, height):
    base = [(x + radius * math.cos(i * math.tau / 10),
             y + radius * math.sin(i * math.tau / 10), z) for i in range(10)]
    tip = (x, y, z + height)
    for index in range(10):
        mesh.facing([base[index], base[(index + 1) % 10], tip], UP, "slate")


def french_castle(spec):
    """A compact château with a central keep and four round-roofed corner pavilions."""
    rect = Rect(spec["length"], spec["width"])
    mesh = Mesh()
    base, step = 1.5, 3.0
    storeys = spec["storeys"]
    eaves = base + storeys * step
    keep = Rect(rect.length * 0.78, rect.width * 0.76)
    for facade in keep.facades():
        bays = max(2, int(facade.length / 4.0))
        openings = [Opening((bay + 0.5) * facade.length / bays - 0.45,
                            (bay + 0.5) * facade.length / bays + 0.45,
                            base + level * step + 0.95,
                            base + level * step + 2.2, depth=0.18)
                    for level in range(storeys) for bay in range(bays)]
        wall(mesh, facade, -3.0, eaves, "stone", openings)
        for opening in openings:
            window(mesh, facade, opening, frame="frame", sill="stone")

    ridge = hipped_roof(mesh, keep, eaves, spec.get("pitch", 45),
                        overhang=0.45, roof="slate")
    for x, y in keep.corners():
        radius = min(1.8, rect.width * 0.12)
        mesh.cylinder((x, y, base), (x, y, eaves + 0.6), radius, 10, "stone")
        _cone(mesh, x, y, eaves + 0.6, radius * 1.2, 2.4)
    return mesh, {"length": rect.length, "width": rect.width,
                  "eaves": eaves, "height": ridge + 1.7, "storeys": storeys}


def _iron_balcony(mesh, facade, u0, u1, floor):
    """A shallow stone balcony with a fine wrought-iron railing."""
    p = facade.point
    depth = 0.85
    mesh.beam(p(u0, floor, 0.0), p(u1, floor, -depth), 0.2, 0.14, "stone")
    rail_bottom, rail_top = floor + 0.12, floor + 1.0
    front = -depth
    mesh.beam(p(u0, rail_top, front), p(u1, rail_top, front), 0.045, 0.045, "metal")
    mesh.beam(p(u0, rail_bottom, front), p(u1, rail_bottom, front), 0.035, 0.035, "metal")
    bars = max(4, int((u1 - u0) / 0.5))
    for index in range(bars + 1):
        u = u0 + (u1 - u0) * index / bars
        mesh.beam(p(u, rail_bottom, front), p(u, rail_top, front), 0.035, 0.035,
                  "metal", caps=False)
    for u in (u0 + 0.25, u1 - 0.25):
        mesh.beam(p(u, floor, -0.08), p(u, floor - 0.35, -depth), 0.07, 0.07, "metal")


def _mansard_roof(mesh, rect, eaves, dormers):
    """A steep lower zinc roof above an inset upper roof, with front and rear dormers."""
    inset = Rect(rect.length - 4.0, rect.width - 3.5)
    lower_height = 2.4
    lower_run = rect.width / 2.0 - inset.width / 2.0
    outer_corners, inner_corners = rect.corners(), inset.corners()
    for index, facade in enumerate(rect.facades()):
        next_index = (index + 1) % 4
        normal = (facade.out.x, facade.out.y, 1.0)
        mesh.facing([
            (*outer_corners[index], eaves),
            (*outer_corners[next_index], eaves),
            (*inner_corners[next_index], eaves + lower_height),
            (*inner_corners[index], eaves + lower_height),
        ], normal, "sheet")

    for side in (-1.0, 1.0):
        offset = min(0.8, lower_run * 0.45)
        y = side * (rect.width / 2.0 - offset)
        roof_floor = eaves + lower_height * offset / lower_run + 0.03
        for index in range(dormers):
            centre = -rect.length / 2.0 + rect.length * (index + 1) / (dormers + 1)
            width = min(1.45, rect.length / (dormers + 1) * 0.52)
            if side < 0.0:
                facade = Facade((centre - width / 2.0, y), (centre + width / 2.0, y))
                hidden_front = "-y"
            else:
                facade = Facade((centre + width / 2.0, y), (centre - width / 2.0, y))
                hidden_front = "+y"
            z0, z1 = roof_floor, roof_floor + 1.15
            inside_y = y - side * 0.6
            mesh.box((centre - width / 2.0, min(y, inside_y), z0),
                     (centre + width / 2.0, max(y, inside_y), z1), "plaster",
                     skip=(hidden_front, "-z", "+z"))
            opening = Opening(width * 0.2, width * 0.8, z0 + 0.2, z1 - 0.2, depth=0.12)
            wall(mesh, facade, z0, z1, "plaster", [opening])
            window(mesh, facade, opening, frame="frame", sill="frame")
            dormer = Rect(width + 0.35, 1.2, (centre, (y + inside_y) / 2.0))
            hipped_roof(mesh, dormer, z1, 35, overhang=0.16,
                        roof="sheet", under="wood_dark", fascia="metal")

    return hipped_roof(mesh, inset, eaves + lower_height, 25, overhang=0.25,
                       roof="sheet", under="wood_dark", fascia="metal")


def french_city(spec):
    """A Haussmann inspired apartment house with pale stone, iron balconies and zinc dormers."""
    rect = Rect(spec["length"], spec["width"])
    mesh = Mesh()
    floor, step = 0.35, 2.9
    storeys = spec["storeys"]
    eaves = floor + storeys * step + 0.25
    for side, facade in enumerate(rect.facades()):
        bays = max(3, int(facade.length / 3.6))
        openings = []
        for level in range(storeys):
            bottom = floor + level * step
            for bay in range(bays):
                centre = (bay + 0.5) * facade.length / bays
                opening = Opening(centre - 0.48, centre + 0.48,
                                  bottom + 0.72, bottom + 2.35, depth=0.14)
                openings.append(opening)
        wall(mesh, facade, -3.0, floor, "stone")
        wall(mesh, facade, floor, eaves, "plaster", openings)
        for opening in openings:
            window(mesh, facade, opening, frame="frame", sill="stone")
        for level in range(1, storeys):
            z = floor + level * step
            mesh.beam(facade.point(0.0, z, -0.03),
                      facade.point(facade.length, z, -0.03), 0.13, 0.1, "frame")
        if side in (0, 2):
            for level in (1, 4):
                if level < storeys:
                    _iron_balcony(mesh, facade, 0.3, facade.length - 0.3,
                                  floor + level * step + 0.08)

    roof_top = _mansard_roof(mesh, rect, eaves, max(4, int(rect.length / 8.0)))
    return mesh, {"length": rect.length, "width": rect.width, "eaves": eaves,
                  "height": roof_top + 0.3, "storeys": storeys}
