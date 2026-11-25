class_name AuthoredMaterials
extends RefCounted
## The materials of a world file's glTF (ADR 0042), drawn as the author made them: textures get
## mipmaps, which glTF loaded at runtime lacks, and ground carrying ORQWA's terrain layers
## (`ORQWA_terrain_layers`, docs/world-files.md) gets its detail near the camera. Both run on
## the worker thread that makes the scene.

const TERRAIN_LAYERS: String = "ORQWA_terrain_layers"
const DIFFUSE_MATERIAL: String = "ORQWA_diffuse"
const MAX_LAYERS: int = 8
## A layer's average colour is never darker than this, so a dark layer cannot blow up the base.
const DARKEST_MEAN: float = 0.02

## Textures are halved until they hold no more pixels than a square this wide (0: as the
## author made them), set by the graphics preset. A world's sections carry their textures at
## full size each: on a phone they did not fit its memory. By their pixels, not their longest
## side: a road's long, narrow strip, its markings painted in, otherwise shrank until the bars
## of a crossing ran together into one line.
static var texture_cap: int = 0
## Images several sections carry, made once and drawn by all: source bytes and cap → a weak
## reference to the texture, so one goes when the last section using it does. Worlds repeat
## their materials' images across sections; uncompressed per section, Twann – Chasseral asked
## for several gigabytes near its start.
static var _shared: Dictionary[String, WeakRef] = {}
static var _shared_lock: Mutex = Mutex.new()


## Gives the textures of `state` mipmaps, so distant ground and façades do not shimmer, at
## most [member texture_cap] in size, and the same texture to every section carrying the same
## image; returns the cut-out ones (leaves, needles: `alphaMode` MASK) as `[texture, cutoff]`
## pairs without, for `finish_cutouts` on the main thread.
static func prepare(state: GLTFState) -> Array[Array]:
	_diffuse_materials(state)
	var cutoffs: Dictionary[int, float] = _cutout_images(state)
	var cutouts: Array[Array] = []
	var images: Array[Texture2D] = state.get_images()
	var replaced: Dictionary[Texture2D, Texture2D] = {}
	for index: int in images.size():
		var image_texture: ImageTexture = images[index] as ImageTexture
		if image_texture == null:
			continue
		var key: String = _image_key(state, index)
		var shared: ImageTexture = _shared_texture(key)
		if shared != null:
			replaced[image_texture] = shared
			images[index] = shared
			# The section that made it may go before it is shown and its cut-out finished.
			if cutoffs.has(index):
				cutouts.append([shared, cutoffs[index]])
			continue
		var image: Image = image_texture.get_image()
		if image == null or image.has_mipmaps() or image.is_compressed():
			continue
		_cap(image)
		if cutoffs.has(index):
			image_texture.set_image(image)
			cutouts.append([image_texture, cutoffs[index]])
		else:
			image.generate_mipmaps()
			image_texture.set_image(image)
		# Shared only once made: a worker never finds one another is still working on.
		var kept: ImageTexture = _share(key, image_texture)
		if kept != image_texture:
			replaced[image_texture] = kept
			images[index] = kept
			cutouts = cutouts.filter(func(c: Array) -> bool: return c[0] != image_texture)
	if not replaced.is_empty():
		state.set_images(images)
		_use_replaced(state, replaced)
	return cutouts


## Lets go of the textures kept for sharing: a world shown next starts afresh.
static func forget_shared() -> void:
	_shared_lock.lock()
	_shared.clear()
	_shared_lock.unlock()


static func _cap(image: Image) -> void:
	while texture_cap > 0 and image.get_width() * image.get_height() > texture_cap * texture_cap:
		image.shrink_x2()


## What identifies an image's texture: its bytes in the file and the cap it is made with; ""
## for an image not stored in the file's own buffer, which is then not shared.
static func _image_key(state: GLTFState, index: int) -> String:
	var described: Array = state.json.get("images", [])
	var views: Array = state.json.get("bufferViews", [])
	var buffers: Array[PackedByteArray] = state.buffers
	if index >= described.size() or not described[index] is Dictionary:
		return ""
	var entry: Dictionary = described[index]
	var found: Variant = entry.get("bufferView")
	if not (found is int or found is float):
		return ""
	var number: float = found
	var view_index: int = int(number)
	if view_index < 0 or view_index >= views.size() or not views[view_index] is Dictionary:
		return ""
	var view: Dictionary = views[view_index]
	var buffer: int = view.get("buffer", 0)
	if buffer < 0 or buffer >= buffers.size():
		return ""
	var start: int = view.get("byteOffset", 0)
	var length: int = view.get("byteLength", 0)
	var bytes: PackedByteArray = buffers[buffer].slice(start, start + length)
	if bytes.size() != length or length == 0:
		return ""
	var hashing: HashingContext = HashingContext.new()
	hashing.start(HashingContext.HASH_SHA256)
	hashing.update(bytes)
	return "%s@%d" % [hashing.finish().hex_encode(), texture_cap]


static func _shared_texture(key: String) -> ImageTexture:
	if key.is_empty():
		return null
	_shared_lock.lock()
	var reference: WeakRef = _shared.get(key)
	var texture: ImageTexture = reference.get_ref() if reference != null else null
	if reference != null and texture == null:
		_shared.erase(key)
	_shared_lock.unlock()
	return texture


## Keeps `texture` for sharing under `key`; the one kept already if another worker was first.
static func _share(key: String, texture: ImageTexture) -> ImageTexture:
	if key.is_empty():
		return texture
	_shared_lock.lock()
	var reference: WeakRef = _shared.get(key)
	var kept: ImageTexture = reference.get_ref() if reference != null else null
	if kept == null:
		_shared[key] = weakref(texture)
		kept = texture
	_shared_lock.unlock()
	return kept


## Godot makes a file's materials while reading it: they are pointed at the shared textures.
static func _use_replaced(state: GLTFState, replaced: Dictionary[Texture2D, Texture2D]) -> void:
	for material: Material in state.get_materials():
		var standard: BaseMaterial3D = material as BaseMaterial3D
		if standard == null:
			continue
		for parameter: int in BaseMaterial3D.TEXTURE_MAX:
			var texture: Texture2D = standard.get_texture(parameter)
			if texture != null and replaced.has(texture):
				standard.set_texture(parameter, replaced[texture])


## Roughness spreads a reflection rather than removing it. Dry porous surfaces may request
## diffuse lighting explicitly, retaining sunlight, shadows and normal maps at every LOD.
static func _diffuse_materials(state: GLTFState) -> void:
	var described: Array = state.json.get("materials", [])
	var materials: Array[Material] = state.get_materials()
	for index: int in mini(described.size(), materials.size()):
		var entry: Dictionary = described[index]
		var metadata: Variant = entry.get("extras", {})
		var material: BaseMaterial3D = materials[index] as BaseMaterial3D
		if not metadata is Dictionary or material == null:
			continue
		var extras: Dictionary = metadata
		var diffuse: Variant = extras.get(DIFFUSE_MATERIAL, false)
		if diffuse is bool and diffuse and material.metallic == 0.0:
			material.specular_mode = BaseMaterial3D.SPECULAR_DISABLED
			material.metallic_specular = 0.0


## Gives cut-out textures (`prepare`) mipmaps that keep their cover: averaged, the alpha of
## scattered leaves falls below the cutoff as they shrink on screen, until a crown shows only
## its trunk. Calls the core, so on the main thread.
static func finish_cutouts(cutouts: Array[Array]) -> void:
	for cutout: Array in cutouts:
		var texture: ImageTexture = cutout[0]
		var cutoff: float = cutout[1]
		var image: Image = texture.get_image()
		# Shared by another section, which finished it already.
		if image.has_mipmaps():
			continue
		image.convert(Image.FORMAT_RGBA8)
		var width: int = image.get_width()
		var height: int = image.get_height()
		var chain: PackedByteArray = OrqwaCore.cutout_mipmaps(
			image.get_data(), width, height, cutoff
		)
		if chain.is_empty():
			image.generate_mipmaps()
		else:
			image.set_data(width, height, true, Image.FORMAT_RGBA8, chain)
		texture.set_image(image)


## The images cut-out materials take their colour from, and those materials' cutoffs.
static func _cutout_images(state: GLTFState) -> Dictionary[int, float]:
	var found: Dictionary[int, float] = {}
	var textures: Array[GLTFTexture] = state.get_textures()
	var described: Array = state.json.get("materials", [])
	for entry: Variant in described:
		var material: Dictionary = entry
		if material.get("alphaMode", "OPAQUE") != "MASK":
			continue
		var pbr: Dictionary = material.get("pbrMetallicRoughness", {})
		var base: Dictionary = pbr.get("baseColorTexture", {})
		var number: float = base.get("index", -1.0)
		var index: int = int(number)
		var cutoff: float = material.get("alphaCutoff", 0.5)
		if index >= 0 and index < textures.size() and textures[index].src_image >= 0:
			found[textures[index].src_image] = cutoff
	return found


## Lets the vertex colours of `root`'s meshes multiply their base colour, as glTF defines them
## (`COLOR_0`); Godot's runtime glTF loader leaves them out of the material.
static func use_vertex_colours(root: Node) -> void:
	for node: Node in root.find_children("*", "MeshInstance3D", true, false):
		var mesh: ArrayMesh = (node as MeshInstance3D).mesh as ArrayMesh
		if mesh == null:
			continue
		for surface: int in mesh.get_surface_count():
			var material: BaseMaterial3D = mesh.surface_get_material(surface) as BaseMaterial3D
			if material == null or not mesh.surface_get_format(surface) & Mesh.ARRAY_FORMAT_COLOR:
				continue
			material = material.duplicate() as BaseMaterial3D
			material.vertex_color_use_as_albedo = true
			mesh.surface_set_material(surface, material)


## Replaces the materials of `root` that carry terrain layers by ORQWA's terrain material.
static func apply_terrain_layers(state: GLTFState, root: Node) -> void:
	var described: Array = state.json.get("materials", [])
	var materials: Array[Material] = state.get_materials()
	var replaced: Dictionary[Material, Material] = {}
	for index: int in mini(described.size(), materials.size()):
		var material: Dictionary = described[index]
		var extensions: Dictionary = material.get("extensions", {})
		if extensions.has(TERRAIN_LAYERS):
			var layers: Dictionary = extensions[TERRAIN_LAYERS]
			replaced[materials[index]] = _terrain(state, layers, materials[index])
	if replaced.is_empty():
		return
	for node: Node in root.find_children("*", "MeshInstance3D", true, false):
		var mesh: Mesh = (node as MeshInstance3D).mesh
		if mesh == null:
			continue
		for surface: int in mesh.get_surface_count():
			var material: Material = mesh.surface_get_material(surface)
			if material != null and replaced.has(material):
				mesh.surface_set_material(surface, replaced[material])


static func _texture(state: GLTFState, reference: Variant) -> Texture2D:
	if not reference is Dictionary:
		return null
	var info: Dictionary = reference
	var index: int = info.get("index", -1)
	var textures: Array[GLTFTexture] = state.get_textures()
	if index < 0 or index >= textures.size():
		return null
	var images: Array[Texture2D] = state.get_images()
	var source: int = textures[index].src_image
	return images[source] if source >= 0 and source < images.size() else null


static func _terrain(state: GLTFState, layers: Dictionary, base: Material) -> ShaderMaterial:
	var terrain: ShaderMaterial = ShaderMaterial.new()
	terrain.shader = preload("res://shaders/authored_terrain.gdshader")
	var standard: BaseMaterial3D = base as BaseMaterial3D
	if standard != null:
		if standard.specular_mode == BaseMaterial3D.SPECULAR_DISABLED:
			terrain.shader = preload("res://shaders/authored_terrain_diffuse.gdshader")
		terrain.set_shader_parameter("base_color", standard.albedo_color)
		terrain.set_shader_parameter("base_texture", standard.albedo_texture)
		terrain.set_shader_parameter("has_base_texture", standard.albedo_texture != null)
		terrain.set_shader_parameter("base_roughness", standard.roughness)
		terrain.set_shader_parameter("base_specular", standard.metallic_specular)
	var weights: Array = layers.get("weights", [])
	for index: int in mini(weights.size(), 2):
		terrain.set_shader_parameter("weights_%d" % index, _texture(state, weights[index]))
	var described: Array = layers.get("layers", [])
	var count: int = mini(described.size(), MAX_LAYERS)
	var albedos: Array[Texture2D] = []
	var normals: Array[Texture2D] = []
	var sizes: PackedFloat32Array = PackedFloat32Array()
	var roughness: PackedFloat32Array = PackedFloat32Array()
	var means: PackedVector3Array = PackedVector3Array()
	var has_normal: PackedFloat32Array = PackedFloat32Array()
	for index: int in MAX_LAYERS:
		var layer: Dictionary = described[index] if index < count else {}
		var albedo: Texture2D = _texture(state, layer.get("albedo"))
		var normal: Texture2D = _texture(state, layer.get("normal"))
		albedos.append(albedo)
		normals.append(normal)
		has_normal.append(1.0 if normal != null else 0.0)
		var size: float = layer.get("size_m", 1.0)
		var rough: float = layer.get("roughness", 1.0)
		sizes.append(maxf(size, 0.01))
		roughness.append(rough)
		means.append(_mean(albedo))
	terrain.set_shader_parameter("layer_count", count)
	terrain.set_shader_parameter("layer_albedo", albedos)
	terrain.set_shader_parameter("layer_normal", normals)
	terrain.set_shader_parameter("layer_has_normal", has_normal)
	terrain.set_shader_parameter("layer_size", sizes)
	terrain.set_shader_parameter("layer_roughness", roughness)
	terrain.set_shader_parameter("layer_mean", means)
	var fade: Array = layers.get("fade_m", [40.0, 160.0])
	terrain.set_shader_parameter("fade_near", fade[0])
	terrain.set_shader_parameter("fade_far", fade[1])
	terrain.set_shader_parameter("strength", layers.get("strength", 1.0))
	return terrain


## A texture's average colour, linear, for dividing its detail out of the base colour.
static func _mean(texture: Texture2D) -> Vector3:
	if texture == null:
		return Vector3.ONE
	var image: Image = texture.get_image()
	if image == null:
		return Vector3.ONE
	image = image.duplicate() as Image
	if image.is_compressed() and image.decompress() != OK:
		return Vector3.ONE
	image.clear_mipmaps()
	while image.get_width() > 1 or image.get_height() > 1:
		image.shrink_x2()
	var colour: Color = image.get_pixel(0, 0).srgb_to_linear()
	return Vector3(
		maxf(colour.r, DARKEST_MEAN), maxf(colour.g, DARKEST_MEAN), maxf(colour.b, DARKEST_MEAN)
	)
