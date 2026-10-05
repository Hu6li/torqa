"""Prints the main colours of a reference image (k-means), leaving out its background.

Run in the art container (Blender has numpy):
    scripts/art.sh blender --background --factory-startup \
        --python .claude/skills/torqa-look/scripts/palette.py -- <image.png> [colours]

The background is taken to be the colour of the bottom-left corner. Blender gives byte images'
pixels sRGB-encoded, so the printed values are sRGB as seen.
"""

import sys

import bpy
import numpy as np

args = sys.argv[sys.argv.index("--") + 1:]
path, count = args[0], int(args[1]) if len(args) > 1 else 14
image = bpy.data.images.load(path)
width, height = image.size
pixels = np.array(image.pixels[:]).reshape(height, width, 4)[:, :, :3]
data = pixels[::2, ::2].reshape(-1, 3)
background = pixels[5, 5]
data = data[np.abs(data - background).sum(1) > 0.12]

rng = np.random.default_rng(1)
centres = data[rng.choice(len(data), count, replace=False)]
for _ in range(30):
    labels = np.argmin(((data[:, None, :] - centres[None]) ** 2).sum(-1), axis=1)
    centres = np.array(
        [data[labels == k].mean(0) if np.any(labels == k) else centres[k] for k in range(count)]
    )
shares = np.bincount(labels, minlength=count) / len(data)


def hex_of(colour):
    return "#" + "".join(f"{int(round(c * 255)):02x}" for c in colour)


print("background", hex_of(background))
for k in np.argsort(-shares):
    print(hex_of(centres[k]), f"{shares[k] * 100:5.1f}%")
