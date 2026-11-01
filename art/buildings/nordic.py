"""Nordic painted timber houses and stone keeps."""

import math

from kit import Mesh, Opening, Rect, UP, chimney, door, gable_roof, wall, window


def nordic_house(spec):
    """A steep-gabled timber house with painted plank walls and bright window trim."""
    rect = Rect(spec["length"], spec["width"])
    mesh = Mesh()
    floor, storey_height = 0.35, 2.9
    storeys = spec["storeys"]
    eaves = floor + storeys * storey_height + 0.25
    for side, facade in enumerate(rect.facades()):
        long_side = side in (0, 2)
        count = max(2, int(facade.length / (3.0 if long_side else 2.7)))
        spacing = facade.length / count
        openings, doors = [], []
        for storey in range(storeys):
            bottom = floor + storey * storey_height
            for bay in range(count):
                centre = (bay + 0.5) * spacing
                if side == 0 and storey == 0 and bay == count // 3:
                    doors.append(Opening(centre - 0.52, centre + 0.52,
                                         bottom, bottom + 2.1, depth=0.22))
                else:
                    openings.append(Opening(centre - 0.52, centre + 0.52,
                                            bottom + 0.82, bottom + 2.1, depth=0.18))
        wall(mesh, facade, -3.0, floor, "stone")
        wall(mesh, facade, floor, eaves, "nordic_wood", openings + doors)
        # Pale corner boards and storey rails frame the painted siding.
        for u in (0.07, facade.length - 0.07):
            mesh.beam(facade.point(u, floor, -0.04), facade.point(u, eaves, -0.04),
                      0.14, 0.14, "frame")
        for z in (floor + 0.18, eaves - 0.15):
            mesh.beam(facade.point(0.0, z, -0.04), facade.point(facade.length, z, -0.04),
                      0.11, 0.11, "frame")
        for opening in openings:
            window(mesh, facade, opening, frame="frame", sill="frame")
        for opening in doors:
            door(mesh, facade, opening, leaf="wood_dark", step="stone", canopy="tiles")

    pitch = spec.get("pitch", 40)
    roof_top = gable_roof(
        mesh, rect, eaves, pitch, overhang=0.8, verge=0.65,
        roof="tiles", under="wood_dark", fascia="frame", gable="nordic_wood",
    )
    # A simple chimney sits clear of the ridge on the rear roof plane.
    chimney(mesh, -rect.length * 0.24, rect.width * 0.2,
            eaves + rect.width * 0.3 * math.tan(math.radians(pitch)),
            roof_top + 0.55, size=0.58, material="stone")
    return mesh, {"length": rect.length, "width": rect.width, "eaves": eaves,
                  "height": roof_top + 0.7, "storeys": storeys}


def _pyramid(mesh, x, y, width, z, height):
    half = width / 2.0
    base = [(x - half, y - half, z), (x + half, y - half, z),
            (x + half, y + half, z), (x - half, y + half, z)]
    tip = (x, y, z + height)
    for i in range(4):
        mesh.facing([base[i], base[(i + 1) % 4], tip], UP, "tiles")
    mesh.facing(base, -UP, "wood_dark")


def nordic_castle(spec):
    """A plastered stone keep with square corner towers under steep tile roofs."""
    rect = Rect(spec["length"], spec["width"])
    mesh = Mesh()
    platform, step = 3.0, 3.3
    corners = rect.corners()
    low = [(x, y, -8.0) for x, y in corners]
    ground = [(x, y, 0.0) for x, y in rect.corners()]
    top = [(x * 0.9, y * 0.9, platform) for x, y in rect.corners()]
    mesh.facing(low, -UP, "castle_stone")
    mesh.facing(top, UP, "castle_stone")
    for i in range(4):
        j = (i + 1) % 4
        dx, dy = corners[j][0] - corners[i][0], corners[j][1] - corners[i][1]
        outward = (dy, -dx, 0.0)
        mesh.facing([low[i], low[j], ground[j], ground[i]], outward, "castle_stone")
        mesh.facing([ground[i], ground[j], top[j], top[i]], outward, "castle_stone")

    storeys = spec["storeys"]
    eaves = platform + storeys * step
    keep = Rect(rect.length * 0.76, rect.width * 0.76)
    for side, facade in enumerate(keep.facades()):
        bays = max(2, int(facade.length / 4.0))
        openings = []
        for storey in range(storeys):
            bottom = platform + storey * step
            for bay in range(bays):
                centre = (bay + 0.5) * facade.length / bays
                openings.append(Opening(centre - 0.46, centre + 0.46,
                                        bottom + 0.9, bottom + 2.2, depth=0.2))
        entrance = None
        if side == 0:
            centre = facade.length * 0.72
            entrance = Opening(centre - 0.65, centre + 0.65,
                               platform, platform + 2.25, depth=0.24)
        wall(mesh, facade, platform, eaves, "plaster", openings + ([entrance] if entrance else []))
        for opening in openings:
            window(mesh, facade, opening, frame="wood_dark", sill="stone")
        if entrance:
            door(mesh, facade, entrance, leaf="wood_dark", step="stone")

    # Four square towers project beyond the keep. Their pyramidal tiled roofs echo Nordic
    # brick and stone fortresses without borrowing the Asian castle silhouette.
    for x, y in rect.corners():
        width = min(3.2, rect.width * 0.22)
        mesh.box((x - width / 2, y - width / 2, platform),
                 (x + width / 2, y + width / 2, eaves + 0.8), "plaster")
        for z in range(int(platform + 1.0), int(eaves), 2):
            mesh.box((x - 0.34, y - width / 2 - 0.03, z),
                     (x + 0.34, y - width / 2 + 0.05, z + 0.75), "glass")
        _pyramid(mesh, x, y, width * 1.24, eaves + 0.8, 2.6)

    roof_top = gable_roof(mesh, keep, eaves, spec.get("pitch", 42),
                          overhang=0.55, verge=0.5, roof="tiles",
                          under="wood_dark", fascia="frame", gable="plaster")
    return mesh, {"length": rect.length, "width": rect.width,
                  "eaves": eaves, "height": max(eaves + 3.4, roof_top),
                  "storeys": storeys}
