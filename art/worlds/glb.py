"""Small tools for world files' glTF binaries (docs/world-files.md), in plain Python: read and
write a `.glb`, embed a PNG as a texture, and give a material ORQWA's terrain layers
(`ORQWA_terrain_layers`), which no glTF exporter writes itself."""

import json
import struct
import zlib

JSON = 0x4E4F534A
BIN = 0x004E4942
TERRAIN_LAYERS = "ORQWA_terrain_layers"


def read(path):
    """The JSON and binary chunk of the `.glb` at `path`."""
    with open(path, "rb") as file:
        data = file.read()
    magic, version, length = struct.unpack_from("<4sII", data, 0)
    assert magic == b"glTF" and version == 2 and length == len(data), path
    json_length, kind = struct.unpack_from("<II", data, 12)
    assert kind == JSON
    gltf = json.loads(data[20:20 + json_length])
    binary = b""
    at = 20 + json_length
    if at < len(data):
        bin_length, kind = struct.unpack_from("<II", data, at)
        assert kind == BIN
        binary = data[at + 8:at + 8 + bin_length]
    return gltf, bytearray(binary)


def write(path, gltf, binary):
    """Writes a `.glb` of `gltf` and `binary`, its one buffer, both padded to four bytes."""
    binary = bytes(binary)
    binary += b"\0" * (-len(binary) % 4)
    if binary:
        gltf["buffers"] = [{"byteLength": len(binary)}]
    else:
        gltf.pop("buffers", None)
    text = json.dumps(gltf, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    length = 12 + 8 + len(text) + (8 + len(binary) if binary else 0)
    with open(path, "wb") as file:
        file.write(struct.pack("<4sII", b"glTF", 2, length))
        file.write(struct.pack("<II", len(text), JSON))
        file.write(text)
        if binary:
            file.write(struct.pack("<II", len(binary), BIN))
            file.write(binary)


def add_texture(gltf, binary, png):
    """Embeds `png` in the binary chunk as a texture; returns the texture's index."""
    binary.extend(b"\0" * (-len(binary) % 4))
    gltf.setdefault("bufferViews", []).append(
        {"buffer": 0, "byteOffset": len(binary), "byteLength": len(png)})
    binary.extend(png)
    gltf.setdefault("images", []).append(
        {"bufferView": len(gltf["bufferViews"]) - 1, "mimeType": "image/png"})
    gltf.setdefault("samplers", []).append({"wrapS": 10497, "wrapT": 10497})
    gltf.setdefault("textures", []).append(
        {"source": len(gltf["images"]) - 1, "sampler": len(gltf["samplers"]) - 1})
    return len(gltf["textures"]) - 1


def add_terrain_layers(gltf, binary, material_name, weights, layers, fade=(40.0, 160.0)):
    """Gives the material named `material_name` ORQWA's terrain layers: `weights` one or two
    PNGs (their channels weigh the layers), `layers` a list of `(albedo png, normal png or None,
    roughness, size in metres)`."""
    material = next(m for m in gltf["materials"] if m.get("name") == material_name)
    extension = {
        "weights": [{"index": add_texture(gltf, binary, png)} for png in weights],
        "layers": [],
        "fade_m": list(fade),
        "strength": 1.0,
    }
    for albedo, normal, roughness, size in layers:
        layer = {"albedo": {"index": add_texture(gltf, binary, albedo)},
                 "roughness": roughness, "size_m": size}
        if normal is not None:
            layer["normal"] = {"index": add_texture(gltf, binary, normal)}
        extension["layers"].append(layer)
    material.setdefault("extensions", {})[TERRAIN_LAYERS] = extension
    used = gltf.setdefault("extensionsUsed", [])
    if TERRAIN_LAYERS not in used:
        used.append(TERRAIN_LAYERS)


def png(pixels):
    """A PNG of `pixels`, a height × width × 3 or 4 array of bytes (numpy or nested lists)."""
    height = len(pixels)
    width = len(pixels[0])
    channels = len(pixels[0][0])
    raw = b"".join(b"\0" + bytes(bytearray(int(v) for px in row for v in px)) for row in pixels) \
        if not hasattr(pixels, "tobytes") else b"".join(
            b"\0" + pixels[y].astype("uint8").tobytes() for y in range(height))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    colour = {3: 2, 4: 6}[channels]
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, colour, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))
