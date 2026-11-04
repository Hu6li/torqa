"""Authored tree LODs, preserving the original species, crown volumes and ground origins.

Run with scripts/art.sh blender --background --factory-startup --python art/vegetation/lods.py.
These are rendering assets, not world-placement models: keep them out of models.json.
"""

import math
import os
import random
import sys

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import build as trees  # noqa: E402


def conifer(spec, far):
    mesh = trees.Mesh()
    rng = random.Random(spec["seed"])
    height, base, radius = spec["height"], spec["crown_base"], spec["radius"]
    trees.trunk(mesh, height - 0.2, 0.28 * radius / 2.5, 0.015,
                sides=4 if far else 6, segments=2 if far else 3)
    layers, spokes = (6, 4) if far else (12, 6)
    for level in range(layers):
        t = level / (layers - 1)
        z = base + (height - base - 0.8) * t
        reach = trees.conifer_reach(spec, z)
        drop = min((height - base) / layers * 1.5, z - 0.1)
        for spoke in range(spokes):
            heading = level * 0.9 + 2 * math.pi * spoke / spokes + rng.uniform(-0.2, 0.2)
            normal = trees.unit((math.cos(heading), math.sin(heading), 0.6))
            # Span the removed whorls vertically, including at the narrow top of the crown;
            # horizontal plates would expose long bare stretches of trunk at each LOD.
            trees.card(mesh, (0, 0, z), (math.cos(heading) * reach, math.sin(heading) * reach, -drop),
                       (0, 0, 1), math.hypot(reach, drop),
                       reach * (1.7 if far else 1.25), "needles", normal)
    trees.conifer_leader(mesh, spec)
    return mesh


def broadleaf(spec, far):
    mesh = trees.Mesh()
    rng = random.Random(spec["seed"])
    top = trees.trunk(mesh, spec["trunk"], 0.3, 0.2,
                      sides=4 if far else 6, segments=2 if far else 3)
    for x, y, z, radius, squash in spec["crowns"]:
        trees.branch(mesh, top, (x, y, z), 0.14, 0.04, sides=3 if far else 4)
        count = max(12, int(radius * radius * (1.8 if far else 4.0)))
        trees.crown(mesh, (x, y, z), (radius * 0.9, radius * 0.9, radius * squash * 0.9),
                    count, 2.9 if far else 2.2, "leaves", rng)
    return mesh


def tropical(spec, far):
    mesh = trees.Mesh()
    rng = random.Random(spec["seed"])
    banana = spec["kind"] == "banana"
    height = spec["stem"] + 0.2 if banana else spec["height"]
    top = trees.trunk(mesh, height, spec.get("base", 0.2), spec.get("top", 0.13),
                      sides=4 if far else 6, segments=2 if far else 3,
                      lean=spec.get("lean", 0))
    count = spec["leaves"] if banana else spec["fronds"]
    for k in range(count):
        heading = 2 * math.pi * k / count + rng.uniform(-0.2, 0.2)
        if spec.get("fans"):
            tilt = rng.uniform(-0.3, 0.6)
            hub = (top[0] + math.cos(heading) * 0.9,
                   top[1] + math.sin(heading) * 0.9, top[2] + tilt * 0.9)
            fan = []
            segments = 2 if far else 3
            for i in range(segments + 1):
                spread = i / segments - 0.5
                angle = heading + spread * 2
                fan.append((hub[0] + math.cos(angle) * spec["leaf"],
                            hub[1] + math.sin(angle) * spec["leaf"],
                            hub[2] + spec["leaf"] * (0.45 + tilt * 0.5) * math.cos(spread)))
            for a, b in zip(fan, fan[1:]):
                trees.both_sides(mesh, [hub, a, b], "frond")
        else:
            base = (0, 0, spec["stem"] - 0.3 * (k % 3)) if banana else top
            segments = (2 if far else 3) if banana else (1 if far else 2)
            trees.leaf(mesh, base, heading, spec["leaf"] * rng.uniform(0.85, 1.1),
                       1.0 if banana else 1.4, 1.3 if banana else 0.7,
                       rng.uniform(0.8, 1.4) if banana else rng.uniform(2.8, 3.6),
                       segments, "frond", fold=0.15, tip_width=0.6 if banana else 0.2)
    if not banana and not spec.get("fans"):
        tuft = spec.get("tuft", 3)
        for k in range(tuft):
            trees.leaf(mesh, top, 2 * math.pi * k / tuft + 0.5,
                       spec["leaf"] * 0.45, 0.5, 1.4, 0.2, 1, "frond")
    return mesh


def main():
    out = os.path.join(trees.OUT, "lods")
    os.makedirs(out, exist_ok=True)
    wanted = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    for spec in trees.CATALOGUE:
        if spec["kind"] not in ("conifer", "broadleaf", "palm", "banana"):
            continue
        if wanted and spec["name"] not in wanted:
            continue
        for level, far in (("medium", False), ("far", True)):
            bpy.ops.wm.read_factory_settings(use_empty=True)
            builder = {"conifer": conifer, "broadleaf": broadleaf}.get(spec["kind"], tropical)
            mesh = builder(spec, far)
            name = spec["name"] + "_" + level
            trees.export(trees.to_object(mesh, name), os.path.join(out, name + ".glb"))
            print(f"built {name}: {sum(len(face) - 2 for face in mesh.faces)} triangles", flush=True)


if __name__ == "__main__":
    main()
