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
	if not _failed:
		print("AUTHORED TEXTURE SMOKE TEST PASSED")
	quit(1 if _failed else 0)


## A section's file: one quad with a 1024 × 512 texture.
func _section_with_image(colour: Color) -> PackedByteArray:
	var image: Image = Image.create(1024, 512, false, Image.FORMAT_RGBA8)
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


## The albedo texture of the scene made from `glb` as a world's section is.
func _albedo(glb: PackedByteArray) -> Texture2D:
	var made: Dictionary[String, Node3D] = {}
	AuthoredWorld._make_scene("section.glb", glb, made, Mutex.new())
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
