"""Draws the foliage cards of the trees and bushes (ADR 0020): clusters of leaves and sprays of
needles on transparent ground, in grey — the app tints each plant from the palette — so a
crown of many such cards reads as foliage, not as a solid shape.

Run in the art container:
    scripts/art.sh blender --background --factory-startup --python art/vegetation/foliage.py \
        -- [--out DIR] [--blossoms-only]

Writes leaves_albedo.webp, needles_albedo.webp and blossoms_albedo.png (RGBA, lossless: edges are
in the alpha channel) into app/assets/textures. Shapes are drawn with numpy from fixed seeds,
so rebuilds give the same files.
"""

import math
import os
import sys

import bpy
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.normpath(os.path.join(HERE, "..", "..", "app", "assets", "textures"))
SIZE = 1024


def canvas():
    rgb = np.zeros((SIZE, SIZE, 3), dtype=np.float64)
    alpha = np.zeros((SIZE, SIZE), dtype=np.float64)
    return rgb, alpha


GRID_Y, GRID_X = np.mgrid[0:SIZE, 0:SIZE].astype(np.float64) / SIZE


def leaf(rgb, alpha, centre, angle, length, width, tone, rng):
    """One leaf: a pointed ellipse along `angle` from its stalk at `centre`, lighter along the
    midrib, darker at the edges, with faint side veins."""
    cx, cy = centre
    dx, dy = math.cos(angle), math.sin(angle)
    # Coordinates along the leaf (0 at the stalk, 1 at the tip) and across it.
    rx, ry = GRID_X - cx, GRID_Y - cy
    along = (rx * dx + ry * dy) / length
    across = (-rx * dy + ry * dx) / width
    # Widest a third of the way, pointed at the tip.
    half = np.sin(np.clip(along, 0, 1) ** 0.75 * math.pi) * 0.5
    inside = (along > 0) & (along < 1) & (np.abs(across) < half)
    edge = np.clip((half - np.abs(across)) * width * SIZE / 1.5, 0, 1)
    coverage = np.where(inside, edge, 0.0)
    shade = 0.75 + 0.25 * (1 - np.abs(across) / np.maximum(half, 1e-3))
    midrib = np.exp(-(across * width * SIZE / 1.2) ** 2) * 0.12
    veins = 0.04 * np.cos((along * 9 + np.abs(across) * 6) * math.pi)
    value = tone * (shade + midrib + veins) * (0.95 + 0.1 * rng.random())
    for c, k in enumerate((0.92, 1.0, 0.85)):
        rgb[..., c] = np.where(coverage > 0, rgb[..., c] * (1 - coverage) + value * k * coverage,
                               rgb[..., c])
    np.maximum(alpha, coverage, out=alpha)


def twig(rgb, alpha, start, end, width, tone):
    """A thin brown twig from `start` to `end`."""
    (sx, sy), (ex, ey) = start, end
    vx, vy = ex - sx, ey - sy
    length2 = vx * vx + vy * vy
    t = np.clip(((GRID_X - sx) * vx + (GRID_Y - sy) * vy) / length2, 0, 1)
    distance = np.hypot(GRID_X - (sx + t * vx), GRID_Y - (sy + t * vy))
    coverage = np.clip((width * (1 - 0.6 * t) - distance) * SIZE, 0, 1)
    for c, k in enumerate((0.55, 0.45, 0.38)):
        rgb[..., c] = rgb[..., c] * (1 - coverage) + tone * k * coverage
    np.maximum(alpha, coverage, out=alpha)


def leaves():
    """A cluster of broad leaves on a few twigs, filling the card loosely."""
    rgb, alpha = canvas()
    rng = np.random.default_rng(7)
    twigs = []
    for k in range(5):
        angle = -math.pi / 2 + (k - 2) * 0.45 + rng.uniform(-0.15, 0.15)
        start = (0.5, 0.98)
        end = (0.5 + math.cos(angle) * 0.75, 0.98 + math.sin(angle) * 0.75)
        twig(rgb, alpha, start, end, 0.006, 0.6)
        twigs.append((start, end))
    for _ in range(70):
        start, end = twigs[rng.integers(len(twigs))]
        t = rng.uniform(0.2, 1.0)
        base = (start[0] + (end[0] - start[0]) * t, start[1] + (end[1] - start[1]) * t)
        angle = math.atan2(end[1] - start[1], end[0] - start[0]) + rng.uniform(-1.2, 1.2)
        leaf(rgb, alpha, base, angle, rng.uniform(0.09, 0.15), rng.uniform(0.07, 0.1),
             rng.uniform(0.55, 0.95), rng)
    return rgb, alpha


def needles():
    """A spray of a conifer: a branch with side twigs, each covered in short needles."""
    rgb, alpha = canvas()
    rng = np.random.default_rng(11)
    start, end = (0.5, 0.98), (0.5, 0.04)
    twig(rgb, alpha, start, end, 0.008, 0.55)
    for k in range(16):
        t = 0.08 + k * 0.055
        base = (0.5, 0.98 - 0.94 * t)
        for side in (-1, 1):
            reach = 0.42 * (1 - t) ** 0.6 + 0.05
            angle = -math.pi / 2 + side * rng.uniform(0.8, 1.1)
            tip = (base[0] + math.cos(angle) * reach, base[1] + math.sin(angle) * reach)
            twig(rgb, alpha, base, tip, 0.004, 0.5)
            for n in range(int(reach * 140)):
                s = rng.uniform(0.0, 1.0)
                at = (base[0] + (tip[0] - base[0]) * s, base[1] + (tip[1] - base[1]) * s)
                direction = angle + rng.choice((-1, 1)) * rng.uniform(0.5, 1.1)
                leaf(rgb, alpha, at, direction, rng.uniform(0.03, 0.045), 0.012,
                     rng.uniform(0.45, 0.8), rng)
    return rgb, alpha


def blossom(rgb, alpha, centre, radius, angle, tone):
    """Five rounded petals and a warm stamen centre, at a few centimetres on the real card."""
    cx, cy = centre
    left, right = max(0, int((cx - radius) * SIZE) - 2), min(SIZE, int((cx + radius) * SIZE) + 3)
    top, bottom = max(0, int((cy - radius) * SIZE) - 2), min(SIZE, int((cy + radius) * SIZE) + 3)
    if right <= left or bottom <= top:
        return
    rx, ry = GRID_X[top:bottom, left:right] - cx, GRID_Y[top:bottom, left:right] - cy
    distance = np.hypot(rx, ry)
    phase = np.arctan2(ry, rx) - angle
    boundary = radius * (0.78 + 0.22 * np.cos(phase * 5))
    coverage = np.clip((boundary - distance) * SIZE, 0, 1)
    shade = tone * (0.76 + 0.19 * np.clip(distance / radius, 0, 1)
                    + 0.05 * np.cos(phase * 5))
    centre_mask = np.exp(-(distance / (radius * 0.22)) ** 4)
    for channel, warm in enumerate((0.65, 0.46, 0.14)):
        value = shade * (1 - centre_mask) + warm * centre_mask
        patch = rgb[top:bottom, left:right, channel]
        patch[:] = patch * (1 - coverage) + value * coverage
    np.maximum(alpha[top:bottom, left:right], coverage, out=alpha[top:bottom, left:right])


def blossoms():
    """Loose clusters of real petal silhouettes along the same twig fan as the leaf cards."""
    rgb, alpha = canvas()
    rng = np.random.default_rng(350)
    branches = []
    for index in range(5):
        angle = -math.pi / 2 + (index - 2) * 0.45
        start = (0.5, 0.98)
        end = (0.5 + math.cos(angle) * 0.75, 0.98 + math.sin(angle) * 0.75)
        twig(rgb, alpha, start, end, 0.005, 0.6)
        branches.append((start, end))
    for _ in range(75):
        start, end = branches[rng.integers(len(branches))]
        along = rng.uniform(0.2, 1.0)
        cluster = (start[0] + (end[0] - start[0]) * along,
                   start[1] + (end[1] - start[1]) * along)
        for _ in range(rng.integers(3, 6)):
            centre = (cluster[0] + rng.uniform(-0.05, 0.05),
                      cluster[1] + rng.uniform(-0.05, 0.05))
            blossom(rgb, alpha, centre, rng.uniform(0.014, 0.024),
                    rng.uniform(0, math.tau), rng.uniform(0.78, 0.98))
    return rgb, alpha


def save(rgb, alpha, path):
    image = bpy.data.images.new(os.path.basename(path), SIZE, SIZE, alpha=True)
    image.colorspace_settings.name = "Non-Color"
    pixels = np.dstack([rgb, alpha])[::-1]  # Blender's rows run bottom up.
    image.pixels.foreach_set(pixels.astype(np.float32).ravel())
    scene = bpy.context.scene
    scene.view_settings.view_transform = "Raw"
    settings = scene.render.image_settings
    settings.file_format = "PNG" if path.endswith(".png") else "WEBP"
    settings.color_mode = "RGBA"
    settings.quality = 100
    image.save_render(path, scene=scene)
    print("drew", path, flush=True)


def main():
    args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    out = args[args.index("--out") + 1] if "--out" in args else OUT
    os.makedirs(out, exist_ok=True)
    recipes = (("blossoms", blossoms),) if "--blossoms-only" in args else (
        ("leaves", leaves), ("needles", needles), ("blossoms", blossoms))
    for name, draw in recipes:
        rgb, alpha = draw()
        # Keep the small blossom mask in PNG: the emulated WebP encoder damaged its alpha.
        suffix = "png" if name == "blossoms" else "webp"
        save(rgb, alpha, os.path.join(out, f"{name}_albedo.{suffix}"))


main()
