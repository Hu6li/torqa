class_name AuthoredMaterials
extends RefCounted
## The materials of a world file's glTF (ADR 0042), drawn as the author made them: textures get
## mipmaps, which glTF loaded at runtime lacks, and ground carrying ORQWA's terrain layers
## (`ORQWA_terrain_layers`, docs/world-files.md) gets its detail near the camera. Both run on
## the worker thread that makes the scene.

const TERRAIN_LAYERS: String = "ORQWA_terrain_layers"
const MAX_LAYERS: int = 8
## A layer's average colour is never darker than this, so a dark layer cannot blow up the base.
const DARKEST_MEAN: float = 0.02


## Gives the textures of `state` mipmaps, so distant ground and façades do not shimmer; and
## returns the cut-out ones (leaves, needles: `alphaMode` MASK) as `[texture, cutoff]` pairs
## without, for `finish_cutouts` on the main thread.
static func prepare(state: GLTFState) -> Array[Array]:
	var cutoffs: Dictionary[int, float] = _cutout_images(state)
	var cutouts: Array[Array] = []
	var images: Array[Texture2D] = state.get_images()
	for index: int in images.size():
		var image_texture: ImageTexture = images[index] as ImageTexture
		if image_texture == null:
			continue
		var image: Image = image_texture.get_image()
		if image == null or image.has_mipmaps() or image.is_compressed():
			continue
		if cutoffs.has(index):
			cutouts.append([image_texture, cutoffs[index]])
			continue
		image.generate_mipmaps()
		image_texture.set_image(image)
	return cutouts


## Gives cut-out textures (`prepare`) mipmaps that keep their cover: averaged, the alpha of
## scattered leaves falls below the cutoff as they shrink on screen, until a crown shows only
## its trunk. Calls the core, so on the main thread.
static func finish_cutouts(cutouts: Array[Array]) -> void:
	for cutout: Array in cutouts:
		var texture: ImageTexture = cutout[0]
		var cutoff: float = cutout[1]
		var image: Image = texture.get_image()
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
		terrain.set_shader_parameter("base_color", standard.albedo_color)
		terrain.set_shader_parameter("base_texture", standard.albedo_texture)
		terrain.set_shader_parameter("has_base_texture", standard.albedo_texture != null)
		terrain.set_shader_parameter("base_roughness", standard.roughness)
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
