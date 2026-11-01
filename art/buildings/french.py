"""French village houses and château inspired keeps."""

import math

from kit import Mesh, Opening, Rect, UP, chimney, door, hipped_roof, wall, window


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
