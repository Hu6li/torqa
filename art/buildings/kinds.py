"""The kinds of building, each built from a catalogue entry (see catalogue.json).

Every builder returns the mesh and the footprint its walls stand on (length along x, width
along y), which the app fits to the map's outlines.
"""

import math

from kit import (
    Facade, Mesh, Opening, Rect, balcony, chimney, clock, door, finial, gable_roof,
    half_hipped_roof, hipped_roof, log_corners, louvres, needle, wall, window,
)

# Floor-to-floor height.
STOREY = 2.9
# The ground floor sits this far above the ground.
FLOOR = 0.3
# Walls reach this far below the ground, so slopes never show a gap (the world places models
# at the highest ground of their plot and keeps shells on steeper plots).
BASEMENT = 3.0
# Walls end this far above the top storey's floor plus its height.
PLATE = 0.25


def columns(length, spacing, margin):
    """Centres of window columns spread evenly along a wall."""
    usable = length - 2.0 * margin
    count = max(1, int(usable / spacing) + 1)
    if count == 1:
        return [length / 2.0]
    return [margin + usable * i / (count - 1) for i in range(count)]


def eaves_height(storeys):
    return FLOOR + storeys * STOREY + PLATE


def footprint(length, width, eaves, top, **more):
    return {"length": length, "width": width, "eaves": eaves, "height": top, **more}


def attic_openings(span, rise, eaves, width, height, base=None):
    """Windows that fit into a gable `span` wide rising `rise` above the eaves: two side by
    side where there is room, else one, else none."""
    sill, top = 0.55, 0.55 + height
    if rise < top + 0.5:
        return []
    half_at_top = span / 2.0 * (1.0 - top / rise)
    centre = span / 2.0
    if half_at_top >= 1.25 + width / 2.0 + 0.3:
        offsets = (-1.25, 1.25)
    elif half_at_top >= width / 2.0 + 0.3:
        offsets = (0.0,)
    else:
        return []
    return [
        Opening(centre + d - width / 2.0, centre + d + width / 2.0, eaves + sill, eaves + top)
        for d in offsets
    ]


def house(spec):
    """A plastered family house: windows with shutters on every storey, a door with a canopy,
    a gable or hipped tiled roof with a chimney and windows in the gables."""
    rect = Rect(spec["length"], spec["width"])
    storeys = spec["storeys"]
    eaves = eaves_height(storeys)
    mesh = Mesh()
    width, height, sill = 1.1, 1.4, 0.85
    south, east, north, west = rect.facades()
    for facade in (south, east, north, west):
        long_side = facade in (south, north)
        centres = columns(facade.length, 2.9 if long_side else 2.7, 1.5)
        openings, doors = [], []
        for storey in range(storeys):
            floor = FLOOR + storey * STOREY
            for k, u in enumerate(centres):
                if facade is south and storey == 0 and k == len(centres) // 3:
                    doors.append(Opening(u - 0.55, u + 0.55, floor, floor + 2.15, depth=0.2))
                    continue
                openings.append(Opening(u - width / 2, u + width / 2, floor + sill,
                                        floor + sill + height))
        wall(mesh, facade, -BASEMENT, FLOOR, "stone")
        wall(mesh, facade, FLOOR, eaves, "plaster", openings + doors)
        for o in openings:
            window(mesh, facade, o, shutters="shutter")
        for o in doors:
            door(mesh, facade, o, canopy="tiles")

    pitch = spec.get("pitch", 40.0)
    slope = math.tan(math.radians(pitch))
    if spec["roof"] == "gable":
        rise = rect.width / 2.0 * slope
        ridge = gable_roof(
            mesh, rect, eaves, pitch, overhang=0.65, verge=0.5,
            gable_openings=attic_openings(rect.width, rise, eaves, 0.9, 1.15),
            gable_window=lambda facade, o: window(mesh, facade, o, shutters="shutter"),
        )
        inset = rect.width * 0.3
    else:
        ridge = hipped_roof(mesh, rect, eaves, pitch, overhang=0.65)
        inset = min(rect.width, rect.length) * 0.3
    chimney(mesh, rect.length * 0.22, -(rect.width / 2.0 - inset), eaves + inset * slope - 0.4,
            ridge + 0.7)
    return mesh, footprint(rect.length, rect.width, eaves, ridge + 1.0, storeys=storeys)


def chalet(spec):
    """A mountain chalet: a plastered ground floor with log-built storeys above, log ends at
    the corners, balconies with geraniums across the front gable, a shallow roof with deep
    eaves and purlins."""
    rect = Rect(spec["length"], spec["width"])
    storeys = spec["storeys"]
    eaves = eaves_height(storeys)
    base = FLOOR + STOREY
    mesh = Mesh()
    south, east, north, west = rect.facades()
    width, height, sill = 0.9, 1.25, 0.8
    for facade in (south, east, north, west):
        front = facade is east
        centres = columns(facade.length, 1.9 if front else 2.5, 1.2)
        lower, upper, glazed, doors = [], [], [], []
        for storey in range(storeys):
            floor = FLOOR + storey * STOREY
            for k, u in enumerate(centres):
                if facade is south and storey == 0 and k == len(centres) // 2:
                    doors.append(Opening(u - 0.5, u + 0.5, floor, floor + 2.05, depth=0.25))
                elif front and storey > 0 and k % 2 == 1:
                    # Doors out onto the balcony.
                    glazed.append(Opening(u - 0.45, u + 0.45, floor + 0.05, floor + 2.15,
                                          depth=0.18))
                elif storey == 0:
                    lower.append(Opening(u - width / 2, u + width / 2, floor + sill,
                                         floor + sill + height, depth=0.22))
                else:
                    upper.append(Opening(u - width / 2, u + width / 2, floor + sill,
                                         floor + sill + height, depth=0.18))
        wall(mesh, facade, -BASEMENT, FLOOR, "stone")
        wall(mesh, facade, FLOOR, base, "plaster", lower + doors)
        wall(mesh, facade, base, eaves, "wood", upper + glazed)
        for o in lower:
            window(mesh, facade, o, shutters="shutter", flowers=True)
        for o in upper:
            window(mesh, facade, o, sill="wood", shutters="shutter", flowers=not front)
        for o in glazed:
            window(mesh, facade, o, sill=None, panes=(2, 3))
        for o in doors:
            door(mesh, facade, o)
    log_corners(mesh, rect, base, eaves)
    for storey in range(1, storeys):
        balcony(mesh, east, 0.25, east.length - 0.25, FLOOR + storey * STOREY)

    pitch = spec.get("pitch", 22.0)
    slope = math.tan(math.radians(pitch))
    rise = rect.width / 2.0 * slope
    ridge = gable_roof(
        mesh, rect, eaves, pitch, overhang=1.3, verge=1.8, roof=spec.get("cover", "slate"),
        gable="wood", gable_openings=attic_openings(rect.width, rise, eaves, 0.8, 1.0),
        gable_window=lambda facade, o: window(mesh, facade, o, sill="wood", shutters="shutter"),
        purlins=True,
    )
    inset = rect.width * 0.32
    chimney(mesh, -rect.length * 0.18, rect.width / 2.0 - inset, eaves + inset * slope - 0.4,
            ridge + 0.8, size=0.7)
    return mesh, footprint(rect.length, rect.width, eaves, ridge + 1.1, storeys=storeys)


def farmhouse(spec):
    """A Bernese farmhouse: house and barn under one huge half-hipped roof reaching low, rows
    of windows with geraniums in the living part at the front, the Ründi arch under the
    front hip, boarded barn walls and a barn door at the back."""
    rect = Rect(spec["length"], spec["width"])
    l = rect.length / 2.0
    eaves = FLOOR + STOREY + 1.0
    mesh = Mesh()
    south, east, north, west = rect.facades()
    living = rect.length * 0.42

    def split(facade, at):
        """Two facades: up to `at` metres along, and the rest."""
        middle = facade.point(at, 0.0)
        return Facade(facade.a, middle), Facade(middle, facade.b)

    def window_band(length, start, z0):
        """A row of windows close together, as Bernese houses have them."""
        count = max(2, int((length - 1.6) / 1.15))
        first = start + (length - count * 1.15) / 2.0
        return [Opening(first + k * 1.15 + 0.12, first + k * 1.15 + 1.03, z0, z0 + 1.25,
                        depth=0.15) for k in range(count)]

    # South: barn then living part (x grows along it); north: living part then barn.
    barn_south, living_south = split(south, rect.length - living)
    living_north, barn_north = split(north, living)
    for facade in (living_south, living_north):
        openings = window_band(facade.length, 0.0, FLOOR + 0.85)
        wall(mesh, facade, -BASEMENT, FLOOR, "stone")
        wall(mesh, facade, FLOOR, eaves, "plaster", openings)
        for o in openings:
            window(mesh, facade, o, shutters=None, flowers=True)
    for facade, gate in ((barn_south, False), (barn_north, True)):
        openings = []
        if gate:
            middle = facade.length / 2.0
            openings.append(Opening(middle - 1.7, middle + 1.7, FLOOR, FLOOR + 3.3, depth=0.2))
        else:
            for u in columns(facade.length, 3.5, 2.0):
                openings.append(Opening(u - 0.35, u + 0.35, FLOOR + 1.6, FLOOR + 2.2, depth=0.15))
        wall(mesh, facade, -BASEMENT, FLOOR, "stone")
        wall(mesh, facade, FLOOR, eaves, "wood", openings)
        for o in openings:
            if gate:
                door(mesh, facade, o, leaf="wood_dark", step="stone")
            else:
                window(mesh, facade, o, sill="wood", panes=(1, 1))
    front_openings = window_band(east.length, 0.0, FLOOR + 0.85)
    wall(mesh, east, -BASEMENT, FLOOR, "stone")
    wall(mesh, east, FLOOR, eaves, "plaster", front_openings)
    for o in front_openings:
        window(mesh, east, o, flowers=True)
    wall(mesh, west, -BASEMENT, FLOOR, "stone")
    wall(mesh, west, FLOOR, eaves, "wood")

    pitch = spec.get("pitch", 45.0)
    slope = math.tan(math.radians(pitch))
    w = rect.width / 2.0
    knee = eaves + 0.6 * w * slope
    gable_rows = []
    for z0 in (eaves + 0.35, eaves + 0.35 + 2.6):
        z1 = z0 + 1.15
        if z1 > knee - 0.4:
            continue
        half = w - (z1 - eaves) / slope - 0.4
        if half < 1.2:
            continue
        gable_rows += window_band(2.0 * half, w - half, z0)
    ridge = half_hipped_roof(
        mesh, rect, eaves, pitch, overhang=1.2, verge=1.5, roof=spec.get("cover", "tiles"),
        gable_openings={1.0: gable_rows},
        gable_window=lambda facade, o: window(mesh, facade, o, sill="wood", flowers=True),
    )
    chimney(mesh, l * 0.45, -w * 0.25, eaves + w * 0.75 * slope - 0.5, ridge + 0.6, size=0.8)
    return mesh, footprint(rect.length, rect.width, eaves, ridge + 0.9, storeys=1)


def church(spec):
    """A village church: a high nave with tall arched windows and a choir behind under steep
    roofs, a tower in front with quoins, clocks, a louvred belfry and a needle spire or a
    saddle roof, and the door at its foot."""
    nave_length, width = spec["length"], spec["width"]
    side = spec["tower"]
    choir_length = width * 0.55
    eaves = spec.get("eaves", 8.4)
    tower_top = spec.get("tower_height", 22.0)
    total = side - 0.4 + nave_length + choir_length - 0.3
    # Everything is placed along x from the tower's front, then centred.
    nave_x = side - 0.4 + nave_length / 2.0
    choir_x = side - 0.4 + nave_length + choir_length / 2.0 - 0.3
    mesh = Mesh()

    # Nave.
    nave = Mesh()
    rect = Rect(nave_length, width)
    for facade in rect.facades():
        openings = []
        if abs(facade.out.y) > 0.5:
            for u in columns(nave_length, 4.6, 3.4):
                openings.append(Opening(u - 0.75, u + 0.75, 2.6, 6.9, depth=0.35, arch=True))
        wall(nave, facade, -BASEMENT, 0.4, "stone")
        wall(nave, facade, 0.4, eaves, "plaster", openings)
        for o in openings:
            window(nave, facade, o, frame="metal", sill="stone", panes=(2, 5))
    ridge = gable_roof(nave, rect, eaves, spec.get("pitch", 52.0), overhang=0.45, verge=0.35,
                       roof=spec.get("cover", "tiles"), under="plaster", fascia="plaster",
                       gable="plaster", rafters=False)
    mesh.add(nave, (nave_x, 0.0, 0.0))

    # Choir: narrower and lower, a hipped roof against the nave.
    choir = Mesh()
    choir_rect = Rect(choir_length, width * 0.65)
    choir_eaves = eaves - 1.4
    for facade in choir_rect.facades():
        openings = []
        # Not the end against the nave.
        if facade.out.x > -0.5:
            u = facade.length / 2.0
            openings.append(Opening(u - 0.6, u + 0.6, 2.4, 5.6, depth=0.35, arch=True))
        wall(choir, facade, -BASEMENT, 0.4, "stone")
        wall(choir, facade, 0.4, choir_eaves, "plaster", openings)
        for o in openings:
            window(choir, facade, o, frame="metal", sill="stone", panes=(2, 4))
    hipped_roof(choir, choir_rect, choir_eaves, 50.0, overhang=0.4,
                roof=spec.get("cover", "tiles"), under="plaster", fascia="plaster", rafters=False)
    mesh.add(choir, (choir_x, 0.0, 0.0))

    # Tower.
    tower = Mesh()
    half = side / 2.0
    tower_rect = Rect(side, side)
    belfry_bottom, belfry_top = tower_top - 4.6, tower_top - 1.2
    clock_z = belfry_bottom - 1.7
    for facade in tower_rect.facades():
        openings = [Opening(half - 0.6, half + 0.6, belfry_bottom, belfry_top, depth=0.4,
                            arch=True)]
        doorway = facade.out.x < -0.5
        if doorway:
            openings.append(Opening(half - 0.85, half + 0.85, 0.4, 3.6, depth=0.5, arch=True))
        else:
            openings.append(Opening(half - 0.2, half + 0.2, 7.0, 8.4, depth=0.3, arch=True))
        wall(tower, facade, -BASEMENT, 0.4, "stone")
        wall(tower, facade, 0.4, tower_top, "plaster", openings)
        louvres(tower, facade, openings[0])
        if doorway:
            door(tower, facade, openings[1], leaf="door", step="stone")
        else:
            window(tower, facade, openings[1], frame="metal", sill=None, panes=(1, 2))
        clock(tower, facade, half, clock_z, min(1.0, half * 0.45))
    # Quoins: stone corner strips standing a little proud of the walls.
    for sx in (-1.0, 1.0):
        for sy in (-1.0, 1.0):
            x0, x1 = sorted((sx * (half - 0.37), sx * (half + 0.03)))
            y0, y1 = sorted((sy * (half - 0.37), sy * (half + 0.03)))
            tower.box((x0, y0, 0.4), (x1, y1, tower_top), "stone", skip=("-z",))
    for z in (belfry_bottom - 0.35, tower_top - 0.3):
        tower.box((-half - 0.12, -half - 0.12, z), (half + 0.12, half + 0.12, z + 0.3), "stone")
    if spec.get("spire", "needle") == "needle":
        tower.box((-half - 0.15, -half - 0.15, tower_top), (half + 0.15, half + 0.15,
                  tower_top + 0.25), "metal", skip=("-z",))
        spire_height = side * spec.get("needle", 2.6)
        needle(tower, (0.0, 0.0), half + 0.1, tower_top + 0.25, spire_height,
               spec.get("spire_cover", "copper"))
        top = tower_top + 0.25 + spire_height + 1.7
    else:
        saddle = Mesh()
        roof_top = gable_roof(saddle, Rect(side, side), tower_top, 58.0, overhang=0.35,
                              verge=0.35, roof=spec.get("spire_cover", "tiles"),
                              under="plaster", fascia="plaster", gable="plaster", rafters=False,
                              gutters=False)
        tower.add(saddle, turn=90.0)
        # The ridge runs across the nave once turned.
        finial(tower, (0.0, half - 0.4, roof_top))
        finial(tower, (0.0, -half + 0.4, roof_top))
        top = roof_top + 1.7
    mesh.add(tower, (half, 0.0, 0.0))

    centre = total / 2.0
    centred = Mesh()
    centred.add(mesh, (-centre, 0.0, 0.0))
    return centred, footprint(total, width, eaves, top, tower=side, spire=spec.get("spire", "needle"))


def chapel(spec):
    """A chapel: a small nave under a steep roof with arched windows, the door in the front
    gable and a slender turret with a spire on the ridge."""
    rect = Rect(spec["length"], spec["width"])
    eaves = spec.get("eaves", 4.8)
    mesh = Mesh()
    south, east, north, west = rect.facades()
    for facade in (south, east, north, west):
        openings = []
        if facade in (south, north):
            for u in columns(facade.length, 3.2, 2.2):
                openings.append(Opening(u - 0.5, u + 0.5, 1.9, 4.1, depth=0.3, arch=True))
        elif facade is west:
            u = facade.length / 2.0
            openings.append(Opening(u - 0.65, u + 0.65, 0.3, 2.9, depth=0.35, arch=True))
        wall(mesh, facade, -BASEMENT, 0.3, "stone")
        wall(mesh, facade, 0.3, eaves, "plaster", openings)
        for o in openings:
            if o.z0 < 1.0:
                door(mesh, facade, o)
            else:
                window(mesh, facade, o, frame="metal", sill="stone", panes=(2, 3))
    pitch = spec.get("pitch", 50.0)
    slope = math.tan(math.radians(pitch))
    ridge = gable_roof(mesh, rect, eaves, pitch, overhang=0.4, verge=0.3,
                       roof=spec.get("cover", "slate"), under="plaster", fascia="plaster",
                       gable="plaster", rafters=False)
    # Ridge turret near the front.
    x = -rect.length / 2.0 + 1.4
    half = 0.7
    base = ridge - half * slope - 0.4
    top = ridge + 2.0
    mesh.box((x - half, -half, base), (x + half, half, top), "wood", skip=("-z",))
    for facade in Rect(2 * half, 2 * half, (x, 0.0)).facades():
        o = Opening(half - 0.3, half + 0.3, top - 1.4, top - 0.4, depth=0.1)
        louvres(mesh, facade, o)
    spire = 3.6
    needle(mesh, (x, 0.0), half + 0.12, top, spire, spec.get("spire_cover", "slate"))
    return mesh, footprint(rect.length, rect.width, eaves, top + spire + 1.7)


def shed(spec):
    """A wooden shed or a plastered garage: low walls, a door, a small window, a light roof."""
    rect = Rect(spec["length"], spec["width"])
    garage = spec.get("garage", False)
    eaves = 2.5 if garage else 2.3
    mesh = Mesh()
    south, east, north, west = rect.facades()
    material = "plaster" if garage else "wood"
    for facade in (south, east, north, west):
        openings = []
        if facade is south:
            u = facade.length / 2.0
            width = min(2.6, facade.length - 1.0) if garage else 1.0
            openings.append(Opening(u - width / 2, u + width / 2, 0.1, 2.1, depth=0.12))
        elif facade is east:
            u = facade.length / 2.0
            openings.append(Opening(u - 0.35, u + 0.35, 1.2, 1.8, depth=0.1))
        wall(mesh, facade, -BASEMENT, 0.1, "stone")
        wall(mesh, facade, 0.1, eaves, material, openings)
        for o in openings:
            if o.z0 < 0.5:
                door(mesh, facade, o, leaf="garage" if garage else "wood_dark", step="stone")
            else:
                window(mesh, facade, o, sill="wood", panes=(1, 1))
    pitch = spec.get("pitch", 22.0)
    ridge = gable_roof(mesh, rect, eaves, pitch, overhang=0.35, verge=0.3,
                       roof=spec.get("cover", "sheet"), gable=material, rafters=not garage,
                       gutters=garage)
    return mesh, footprint(rect.length, rect.width, eaves, ridge)
