"""Builds the parts buildings are assembled from on their mapped outlines (ADR 0041, step 7)
and exports them for Godot as `parts/<name>.glb` beside the building models.

Run in the art container:
    scripts/art.sh blender --background --factory-startup --python art/buildings/parts.py

The world lays a part where the generated walls draw a window (`orqwa_world::buildings`,
`lay_parts`; `app/shaders/building.gdshader`): its opening, the casing, runs 1.48 m along the
wall from 0.95 m to 2.44 m over its storey's floor, so a part covers the window drawn behind
it. Each part is built on such a wall in the x–z plane, facing −y (out of the wall); its
origin lies on the wall under the middle of the opening, at the floor of its storey. The
world stretches parts along the wall as the wall stretches its windows' spacing. Materials
are named as on the models (`frame`, `glass`, `shutter`, …), so the app styles them alike:
shutters and flower boxes show on some buildings only, as on the models.

Pass `--preview DIR` after `--` to render the parts there instead of exporting them.
"""

import os
import sys

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.dirname(HERE))

import kit  # noqa: E402  (needs the path above)
import preview  # noqa: E402
from blender_io import export, to_object  # noqa: E402

OUT = os.path.normpath(
    os.path.join(HERE, "..", "..", "app", "assets", "models", "buildings", "parts")
)

# The casing the walls draw: along the wall from its middle, and up from the storey's floor.
HALF, BOTTOM, TOP = 0.74, 0.95, 2.44
# How far the frame stands out of the wall, and the glass.
PROUD, GLASS = 0.04, 0.015
FRAME = 0.08


def wall():
    """The wall a part stands on, `u` running from the casing's left edge."""
    return kit.Facade((-HALF, 0.0), (HALF, 0.0))


def ring(mesh, facade, outer, inner, depth, material):
    """A flat frame between rectangles `outer` and `inner` (u0, u1, z0, z1) standing `depth`
    out of the wall, with its outer edge back to the wall."""
    p = facade.point
    (a0, a1, b0, b1), (c0, c1, d0, d1) = outer, inner
    out = -depth
    for points in (
        [(a0, b0), (a1, b0), (c1, d0), (c0, d0)],
        [(a1, b0), (a1, b1), (c1, d1), (c1, d0)],
        [(a1, b1), (a0, b1), (c0, d1), (c1, d1)],
        [(a0, b1), (a0, b0), (c0, d0), (c0, d1)],
    ):
        mesh.facing([p(u, z, out) for u, z in points], facade.out, material)
    edges = [((a0, b0), (a1, b0), -kit.UP), ((a1, b0), (a1, b1), facade.along),
             ((a1, b1), (a0, b1), kit.UP), ((a0, b1), (a0, b0), -facade.along)]
    for (u0, z0), (u1, z1), normal in edges:
        mesh.facing([p(u0, z0, 0.0), p(u1, z1, 0.0), p(u1, z1, out), p(u0, z0, out)],
                    normal, material)


def window():
    """A window with casement frames, a mullion and a transom, a stone sill, shutters folded
    back and a box of geraniums."""
    mesh, facade = kit.Mesh(), wall()
    u0, u1 = 0.0, 2 * HALF
    outer = (u0, u1, BOTTOM, TOP)
    inner = (u0 + FRAME, u1 - FRAME, BOTTOM + FRAME, TOP - FRAME)
    ring(mesh, facade, outer, inner, PROUD, "frame")
    p = facade.point
    mesh.facing([p(inner[0], inner[2], -GLASS), p(inner[1], inner[2], -GLASS),
                 p(inner[1], inner[3], -GLASS), p(inner[0], inner[3], -GLASS)],
                facade.out, "glass")
    middle = (u0 + u1) / 2.0
    transom = inner[2] + (inner[3] - inner[2]) * 0.68
    mesh.beam(p(middle, inner[2], -0.03), p(middle, inner[3], -0.03), 0.05, 0.04, "frame",
              up=facade.out)
    mesh.beam(p(inner[0], transom, -0.03), p(inner[1], transom, -0.03), 0.05, 0.04, "frame",
              up=facade.out)
    opening = kit.Opening(u0, u1, BOTTOM, TOP, depth=0.0)
    kit.mesh_sill(mesh, facade, opening, "stone")
    width = (u1 - u0) / 2.0
    for s0, s1, side in ((u0 - width - 0.03, u0 - 0.03, -1.0), (u1 + 0.03, u1 + width + 0.03, 1.0)):
        kit.shutter(mesh, facade, s0, s1, BOTTOM, TOP, "shutter", side)
    kit.flower_box(mesh, facade, opening)
    return mesh


def door():
    """A front door in a stone surround as wide as a window's casing and as high, on a step,
    under a small tiled canopy on wooden brackets."""
    mesh, facade = kit.Mesh(), wall()
    p = facade.point
    u0, u1 = 0.0, 2 * HALF
    leaf = (0.24, 2 * HALF - 0.24, 0.0, 2.25)
    ring(mesh, facade, (u0, u1, 0.0, TOP + 0.06), leaf, PROUD, "stone")
    mesh.facing([p(leaf[0], leaf[2], -0.01), p(leaf[1], leaf[2], -0.01),
                 p(leaf[1], leaf[3], -0.01), p(leaf[0], leaf[3], -0.01)],
                facade.out, "door", uvs=[(0, 0), (1, 0), (1, 1), (0, 1)])
    # The step, sunk a little into the ground on slopes.
    mesh.box((-HALF - 0.2, -0.45, -0.3), (HALF + 0.2, 0.0, 0.0), "stone", skip=("+y",))
    c0, c1 = u0 - 0.35, u1 + 0.35
    low, high, reach = TOP + 0.3, TOP + 0.62, -1.0
    mesh.facing([p(c0, low, reach), p(c1, low, reach), p(c1, high, 0.0), p(c0, high, 0.0)],
                kit.UP + facade.out, "tiles")
    mesh.facing([p(c0, low - 0.08, reach), p(c0, high - 0.08, 0.0), p(c1, high - 0.08, 0.0),
                 p(c1, low - 0.08, reach)], -kit.UP, "wood")
    mesh.facing([p(c0, low - 0.08, reach), p(c1, low - 0.08, reach), p(c1, low, reach),
                 p(c0, low, reach)], facade.out, "wood")
    for u in (c0 + 0.1, c1 - 0.1):
        mesh.beam(p(u, high - 0.6, 0.0), p(u, low - 0.04, reach + 0.1), 0.08, 0.1, "wood")
    return mesh


PARTS = {"window": window, "door": door}


def main():
    wanted, preview_dir = preview.options(list(sys.argv))
    os.makedirs(OUT, exist_ok=True)
    for name, build in PARTS.items():
        if wanted and name not in wanted:
            continue
        bpy.ops.wm.read_factory_settings(use_empty=True)
        mesh = build()
        obj = to_object(mesh, name)
        if preview_dir:
            preview.render(obj, preview_dir, name)
            continue
        export(obj, os.path.join(OUT, name + ".glb"))
        triangles = sum(len(face) - 2 for face in mesh.faces)
        print(f"built part {name}: {len(mesh.faces)} faces, {triangles} triangles")


main()
