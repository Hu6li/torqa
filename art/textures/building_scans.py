"""Convert archived CC0 building scans without authoring-time or runtime downloads."""

from pathlib import Path

import bpy

SOURCES = Path(__file__).resolve().parent.parent / "sources"


def convert(size, out):
    """Preserve sRGB colour and linear OpenGL normal/roughness maps in the shared formats."""
    scene = bpy.context.scene
    scene.view_settings.look = "None"
    scene.render.image_settings.file_format = "WEBP"
    scene.render.image_settings.color_mode = "RGB"
    for source, target in (("painted_plaster_wall", "building_plaster"),
                           ("roof_tiles", "building_tiles")):
        directory = SOURCES / source.replace("_", "-")
        for suffix, kind, ext in (("diff", "albedo", "jpg"), ("nor_gl", "normal", "exr"),
                                   ("rough", "rough", "exr")):
            image = bpy.data.images.load(str(directory / f"{source}_{suffix}_1k.{ext}"))
            if kind != "albedo":
                image.colorspace_settings.name = "Non-Color"
            if tuple(image.size) != (size, size):
                image.scale(size, size)
            if kind == "albedo":
                # Neutral scans retain weathering while regional wall/roof palettes supply hue.
                pixels = list(image.pixels[:])
                luminance = [0.2126 * pixels[i] + 0.7152 * pixels[i + 1] + 0.0722 * pixels[i + 2]
                             for i in range(0, len(pixels), 4)]
                factor = 0.60 / max(sum(luminance) / len(luminance), 0.001)
                for index, value in enumerate(luminance):
                    gray = min(value * factor, 0.95)
                    pixels[index * 4:index * 4 + 3] = [gray, gray, gray]
                image.pixels[:] = pixels
            scene.view_settings.view_transform = "Standard" if kind == "albedo" else "Raw"
            scene.render.image_settings.quality = 100 if kind == "normal" else 90
            path = Path(out).resolve() / f"{target}_{kind}.webp"
            image.save_render(str(path), scene=scene)
            bpy.data.images.remove(image)
            settings = ('[remap]\nimporter="texture"\ntype="CompressedTexture2D"\n'
                        '[params]\ncompress/mode=2\nmipmaps/generate=true\n'
                        f'compress/normal_map={1 if kind == "normal" else 0}\n')
            # Keep existing Godot-generated resource identifiers on subsequent conversions.
            if not Path(str(path) + ".import").exists():
                Path(str(path) + ".import").write_text(settings)
            print("converted", path, flush=True)


if __name__ == "__main__":
    convert(1024, Path(__file__).resolve().parents[2] / "app/assets/textures")
