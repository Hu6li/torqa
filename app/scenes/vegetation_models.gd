class_name VegetationModels
extends RefCounted
## The vegetation models made in Blender (art/vegetation, ADR 0020): trees, bushes, ferns and rocks.
## Loads them and gives each material name its look: bark textured on trunks
## (app/shaders/vegetation.gdshader), foliage cards of leaves or needles cut out and tinted per
## plant (app/shaders/foliage.gdshader), palm and fern fronds and rocks tinted per plant.

const DIRECTORY: String = "res://assets/models/vegetation/"
## Per material name: shader, its `kind` (vegetation.gdshader), texture, texture size in metres,
## how much the plant's own colour tints it, and sway in the wind (metres at 10 m height).
const LOOKS: Dictionary[String, Array] = {
	"trunk": ["vegetation", 0, "bark", 1.2, 0.0, 0.12],
	"leaves": ["foliage", 0, "leaves", 1.0, 1.0, 0.12],
	"needles": ["foliage", 0, "needles", 1.0, 1.0, 0.1],
	"frond": ["vegetation", 2, "", 1.0, 1.0, 0.12],
	"rock": ["vegetation", 3, "rock", 1.6, 1.0, 0.0],
}
## Plants are batched in cells of this size (metres): Godot chooses one level of detail per
## batch from its closest point, so in a whole 480 m chunk one close tree kept every tree at full
## detail. Low too: on the Galaxy Tab S7 the far levels' fewer, larger cards save 5–10 ms of GPU
## time per frame for about 1 ms of render CPU. Ferns and bushes have no coarser levels, yet in
## cells they save that tablet about 2–3 ms per frame in forest (nothing in open country) at the
## same range, which shorter ranges did not (docs/benchmarks/tablet-frame-rate.md).
const CELL_M: float = 160.0

static var _materials: Dictionary[String, ShaderMaterial] = {}
static var _meshes: Dictionary[String, Mesh] = {}


## One model's uploaded plants of a chunk, in cells of `CELL_M`.
static func add_to(
	parent: Node3D,
	name: String,
	buffer: PackedFloat32Array,
	tree_range: float,
	small_range: float,
	tree_lod_bias: float = 1.0
) -> void:
	if buffer.is_empty():
		return
	var cells: Dictionary[Vector2i, PackedFloat32Array] = {}
	for offset: int in range(0, buffer.size(), 16):
		# Plants sit relative to the chunk's centre, so a 480 m chunk has 4 × 4 cells, 80 m wide
		# at its edges. Nine whole cells drew the tablet's trees slower: their far levels took
		# over later (docs/benchmarks/tablet-frame-rate.md).
		var key: Vector2i = Vector2i(
			floori(buffer[offset + 3] / CELL_M), floori(buffer[offset + 11] / CELL_M)
		)
		var values: PackedFloat32Array = cells.get(key, PackedFloat32Array())
		values.append_array(buffer.slice(offset, offset + 16))
		cells[key] = values
	for values: PackedFloat32Array in cells.values():
		parent.add_child(instance(name, values, tree_range, small_range, tree_lod_bias))


## One model's uploaded plants. Tree ranges follow the preset or View distance override;
## smaller plants stay nearby. A short fade keeps Low from drawing well past its budget.
## `tree_lod_bias` below 1 takes trees' coarser levels sooner (Low).
static func instance(
	name: String,
	buffer: PackedFloat32Array,
	tree_range: float,
	small_range: float,
	tree_lod_bias: float = 1.0
) -> MultiMeshInstance3D:
	var multimesh: MultiMesh = MultiMesh.new()
	multimesh.transform_format = MultiMesh.TRANSFORM_3D
	multimesh.use_colors = true
	multimesh.mesh = mesh(name)
	multimesh.instance_count = buffer.size() / 16
	multimesh.buffer = buffer
	var placed: MultiMeshInstance3D = MultiMeshInstance3D.new()
	placed.multimesh = multimesh
	var tree: bool = _is_tree(name)
	if tree:
		placed.set_meta("tree", true)
		placed.lod_bias = tree_lod_bias
	placed.visibility_range_end = tree_range if tree else small_range
	placed.visibility_range_end_margin = minf(tree_range * 0.15, 300.0) if tree else 100.0
	placed.visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF
	return placed


## The mesh of model `name` with its materials replaced by the shared looks.
static func mesh(name: String) -> Mesh:
	if _meshes.has(name):
		return _meshes[name]
	var path: String = DIRECTORY + name + ".glb"
	# A world kept from an older ORQWA may name a model this one no longer has (R101): it is
	# left out rather than breaking its chunk.
	if not ResourceLoader.exists(path):
		return null
	var scene: PackedScene = load(path)
	var root: Node = scene.instantiate()
	var found: ArrayMesh = _find_mesh(root).mesh
	root.free()
	if _is_tree(name):
		var levels: Array[ArrayMesh] = []
		for level: String in ["medium", "far"]:
			var lod_path: String = DIRECTORY + "lods/%s_%s.glb" % [name, level]
			if not ResourceLoader.exists(lod_path):
				break
			var lod_scene: PackedScene = load(lod_path)
			var lod_root: Node = lod_scene.instantiate()
			levels.append(_find_mesh(lod_root).mesh)
			lod_root.free()
		if levels.size() == 2:
			found = TreeLodMesh.combine(found, levels)
	for surface: int in found.get_surface_count():
		var imported: Material = found.surface_get_material(surface)
		found.surface_set_material(surface, material(imported.resource_name))
	_meshes[name] = found
	return found


static func _is_tree(name: String) -> bool:
	return name.get_slice("_", 0) in ["conifer", "broadleaf", "palm", "banana"]


## The shared material for a material name.
static func material(name: String) -> ShaderMaterial:
	if _materials.has(name):
		return _materials[name]
	var look: Array = LOOKS.get(name, LOOKS["leaves"])
	var shader_name: String = look[0]
	var texture_name: String = look[2]
	var shader_material: ShaderMaterial = ShaderMaterial.new()
	shader_material.shader = load("res://shaders/%s.gdshader" % shader_name)
	if not texture_name.is_empty():
		shader_material.set_shader_parameter(
			"albedo_texture", load("res://assets/textures/%s_albedo.webp" % texture_name)
		)
		var normal_path: String = "res://assets/textures/%s_normal.webp" % texture_name
		if ResourceLoader.exists(normal_path):
			shader_material.set_shader_parameter("normal_texture", load(normal_path))
	if shader_name == "vegetation":
		shader_material.set_shader_parameter("kind", look[1])
		shader_material.set_shader_parameter("tile", look[3])
		shader_material.set_shader_parameter("tint_strength", look[4])
	shader_material.set_shader_parameter("sway", look[5])
	_materials[name] = shader_material
	return shader_material


## How strongly the wind moves trees (0 calm, 1 storm), for the weather.
static func set_wind(strength: float) -> void:
	for name: String in LOOKS:
		material(name).set_shader_parameter("wind_strength", strength)


static func _find_mesh(node: Node) -> MeshInstance3D:
	if node is MeshInstance3D:
		return node
	for child: Node in node.get_children():
		var found: MeshInstance3D = _find_mesh(child)
		if found != null:
			return found
	return null
