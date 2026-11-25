extends SceneTree
## A world file's textures (ADR 0042): sections carrying the same image draw one texture, no
## larger than the graphics preset allows, so a large world fits a phone's memory.

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var glb: PackedByteArray = _section_with_image(Color(0.8, 0.3, 0.2))
	AuthoredMaterials.texture_cap = 256
	var first: Texture2D = _albedo(glb)
	var second: Texture2D = _albedo(glb)
	_check(first != null, "a section's texture is made")
	_check(first == second, "two sections carrying one image draw one texture")
	_check(
		first.get_width() == 256 and first.get_height() == 128,
		(
			"halved to the preset's size, keeping its shape: %dx%d"
			% [first.get_width(), first.get_height()]
		)
	)
	_check(first.get_image().has_mipmaps(), "with mipmaps")
	# A road's strip, its markings painted in: as narrow as it is, it keeps their width.
	var strip: Texture2D = _albedo(_section_with_image(Color(0.9, 0.8, 0.1), Vector2i(64, 4096)))
	_check(
		strip.get_width() == 32 and strip.get_height() == 2048,
		"a narrow strip keeps its width: %dx%d" % [strip.get_width(), strip.get_height()]
	)
	var other: Texture2D = _albedo(_section_with_image(Color(0.1, 0.5, 0.9)))
	_check(other != first, "another image is a texture of its own")
	AuthoredMaterials.texture_cap = 0
	var full: Texture2D = _albedo(glb)
	_check(full != first and full.get_width() == 1024, "the author's size without a cap")
	AuthoredMaterials.forget_shared()
	_check(_albedo(glb) != full, "a world shown next starts afresh")
	_check(
		AuthoredWorld.QUALITY["low"].x < AuthoredWorld.QUALITY["ultra"].x, "Low reaches less far"
	)
	_check(
		AuthoredWorld.QUALITY["ultra"] == Vector2i(100, 0),
		"Ultra shows the world as its author made it: full reach, textures at their own size"
	)
	_check_shared_by_uri()
	_check_newer_extensions()
	if not _failed:
		print("AUTHORED TEXTURE SMOKE TEST PASSED")
	quit(1 if _failed else 0)


## A world stores an image once (`textures/`) for its sections to name by `uri`: each section
## draws the one texture decoded from it.
func _check_shared_by_uri() -> void:
	AuthoredMaterials.texture_cap = 256
	var glb: PackedByteArray = _naming_shared(
		_section_with_image(Color(0.2, 0.6, 0.3)), "../../textures/wall.png"
	)
	var shared: Dictionary[int, String] = AuthoredTextures.shared("sections/0/lod0.glb", glb)
	_check(shared == {0: "textures/wall.png"}, "a section names its shared image: %s" % shared)
	var made: Dictionary[String, Texture2D] = {}
	var wall: Image = Image.create(1024, 1024, false, Image.FORMAT_RGBA8)
	wall.fill(Color(0.7, 0.5, 0.4))
	AuthoredTextures._decode("textures/wall.png", wall.save_png_to_buffer(), made, Mutex.new())
	var texture: Texture2D = made.get("textures/wall.png")
	_check(
		texture != null and texture.get_width() == 256 and texture.get_image().has_mipmaps(),
		"a shared image is decoded once, within the preset's size, with mipmaps"
	)
	var provided: Dictionary[int, Texture2D] = {0: texture}
	var first: Texture2D = _albedo(glb, provided)
	var second: Texture2D = _albedo(glb, provided)
	_check(first == texture and second == texture, "every section draws that one texture")
	# One the core could not give, or Godot not read, leaves its sections without it.
	var without: Dictionary[int, Texture2D] = {0: null}
	_check(_albedo(glb, without) != null, "and its sections are drawn all the same")
	var leaves: Texture2D = ImageTexture.create_from_image(wall)
	(leaves as ImageTexture).get_image().generate_mipmaps()
	var cutouts: Array[Array] = [[leaves, 0.5], [leaves, 0.5]]
	AuthoredMaterials.finish_cutouts(cutouts)
	_check(
		leaves.has_meta(AuthoredMaterials.CUTOUT_FINISHED) and leaves.get_image().has_mipmaps(),
		"a shared cut-out gets mipmaps keeping its cover, once"
	)
	AuthoredMaterials.texture_cap = 0
	AuthoredMaterials.forget_shared()


## A newer world's section: its texture has a GPU image beside the PNG, from an extension this
## ORQWA does not know, and its material carries that vendor's data. It is drawn from the PNG.
func _check_newer_extensions() -> void:
	var plain: PackedByteArray = _section_with_image(Color(0.9, 0.2, 0.2))
	var gltf: Dictionary = AuthoredMaterials.glb_json(plain)
	gltf["extensionsUsed"] = ["KHR_texture_basisu", "VENDOR_detail"]
	var images: Array = gltf["images"]
	images.append({"bufferView": 0, "mimeType": "image/ktx2"})
	images.append({"uri": "../../ext/ORQWA_gpu_textures/wall.ktx2"})
	var textures: Array = gltf["textures"]
	var texture: Dictionary = textures[0]
	texture["extensions"] = {"KHR_texture_basisu": {"source": 1}}
	var materials: Array = gltf["materials"]
	var material: Dictionary = materials[0]
	material["extensions"] = {"VENDOR_detail": {"layerTexture": {"index": 9}}}
	var newer: PackedByteArray = _with_json(plain, gltf)
	_check(
		AuthoredTextures.shared("sections/0/lod0.glb", newer).is_empty(),
		"an extension's own images are never asked for"
	)
	var albedo: Texture2D = _albedo(newer)
	_check(
		albedo != null and albedo.get_width() == 1024,
		"drawn from its PNG, without the extension this ORQWA does not know"
	)


## `glb` with its one image named by `uri` instead of stored in it, as a world's section
## sharing it does.
func _naming_shared(glb: PackedByteArray, uri: String) -> PackedByteArray:
	var gltf: Dictionary = AuthoredMaterials.glb_json(glb)
	gltf["images"] = [{"uri": uri}]
	return _with_json(glb, gltf)


## `glb` with its JSON chunk replaced by `gltf`.
func _with_json(glb: PackedByteArray, gltf: Dictionary) -> PackedByteArray:
	var json: PackedByteArray = JSON.stringify(gltf, "", false, true).to_utf8_buffer()
	while json.size() % 4 != 0:
		json.append(0x20)
	var rest: PackedByteArray = glb.slice(20 + glb.decode_u32(12))
	var out: PackedByteArray = PackedByteArray()
	out.resize(20)
	out.encode_u32(0, AuthoredMaterials.GLB_MAGIC)
	out.encode_u32(4, 2)
	out.encode_u32(8, 20 + json.size() + rest.size())
	out.encode_u32(12, json.size())
	out.encode_u32(16, AuthoredMaterials.JSON_CHUNK)
	out.append_array(json)
	out.append_array(rest)
	return out


## A section's file: one quad with a texture of `size`.
func _section_with_image(colour: Color, size: Vector2i = Vector2i(1024, 512)) -> PackedByteArray:
	var image: Image = Image.create(size.x, size.y, false, Image.FORMAT_RGBA8)
	image.fill(colour)
	var material: StandardMaterial3D = StandardMaterial3D.new()
	material.albedo_texture = ImageTexture.create_from_image(image)
	var quad: QuadMesh = QuadMesh.new()
	quad.material = material
	var root: Node3D = Node3D.new()
	var mesh: MeshInstance3D = MeshInstance3D.new()
	mesh.mesh = quad
	root.add_child(mesh)
	mesh.owner = root
	var document: GLTFDocument = GLTFDocument.new()
	var state: GLTFState = GLTFState.new()
	document.append_from_scene(root, state)
	var bytes: PackedByteArray = document.generate_buffer(state)
	root.free()
	return bytes


## The albedo texture of the scene made from `glb` as a world's section is, with the world's
## shared textures `provided`.
func _albedo(glb: PackedByteArray, provided: Dictionary[int, Texture2D] = {}) -> Texture2D:
	var made: Dictionary[String, Node3D] = {}
	AuthoredWorld._make_scene("section.glb", glb, made, Mutex.new(), provided)
	var scene: Node3D = made.get("section.glb")
	if scene == null:
		return null
	var found: Array[Node] = scene.find_children("*", "MeshInstance3D", true, false)
	var texture: Texture2D = null
	if not found.is_empty():
		var mesh: Mesh = (found[0] as MeshInstance3D).mesh
		var material: BaseMaterial3D = mesh.surface_get_material(0) as BaseMaterial3D
		if material != null:
			texture = material.albedo_texture
	scene.free()
	return texture


func _check(condition: bool, what: String) -> void:
	if not condition:
		_failed = true
		push_error(what)
