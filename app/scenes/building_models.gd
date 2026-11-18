class_name BuildingModels
extends RefCounted
## The building models made in Blender (art/buildings, R45): loads them and gives their
## materials the textures and palette colours their names ask for
## (app/shaders/building_model.gdshader, ADR 0020). Every building of a chunk is an instance of
## a MultiMesh: the instance colour is its plaster, the custom data its roof colour — or for
## flat-roofed offices, hotels and public buildings their accent colour — (rgb) and variant
## (a, 0–1).

const DIRECTORY: String = "res://assets/models/buildings/"
const MODEL_FADE: float = 40.0

## Shader pattern, palette colour, texture (art/textures, ADR 0020) and its size in metres of
## each material name; materials without a texture are drawn by the shader alone.
const LOOKS: Dictionary[String, Array] = {
	"plaster": [0, "buildings.light_walls", "building_plaster", 2.0],
	"nordic_wood": [20, "buildings.timber", "planks", 1.5],
	"stone": [1, "buildings.stone", "masonry", 2.4],
	"castle_stone": [1, "buildings.stone", "rock", 2.4],
	"wood": [2, "buildings.timber", "planks", 2.0],
	"wood_dark": [3, "buildings.wood_dark", "planks", 2.0],
	"wood_light": [2, "buildings.wood_light", "planks", 2.0],
	"frame": [4, "buildings.frame", "", 1.0],
	"glass": [5, "buildings.glass", "", 1.0],
	"glass_detail": [21, "buildings.glass", "", 1.0],
	"leaded": [17, "buildings.stained_glass", "", 1.0],
	"shutter": [6, "buildings.shutters", "planks", 1.0],
	"door": [7, "buildings.door", "planks", 1.0],
	"tiles": [8, "buildings.tiles", "building_tiles", 2.0],
	"slate": [9, "buildings.slate", "slate", 2.0],
	"metal": [10, "buildings.metal", "", 1.0],
	"copper": [11, "buildings.copper", "", 1.0],
	"flowers": [12, "buildings.flowers", "", 1.0],
	"leaves": [13, "buildings.leaves", "", 1.0],
	"clock": [14, "buildings.clock", "", 1.0],
	"sheet": [15, "buildings.sheet", "", 1.0],
	"garage": [16, "buildings.garage", "plaster", 2.0],
	"accent": [18, "buildings.accents", "plaster", 2.0],
	"roof_flat": [19, "buildings.flat_roofs", "gravel", 1.5],
}

static var _materials: Dictionary = {}
static var _meshes: Dictionary = {}
static var _detail_meshes: Dictionary = {}


## Preset-specific model uploads retain exactly the transforms and colours of the saved world.
static func add_to(
	parent: Node3D,
	cell: Dictionary,
	shell_material: Material,
	model_range: float,
	shell_range: float,
	detailed: bool
) -> void:
	var models: Dictionary = cell["models"]
	# Two dithered copies fading over the same distances leave holes where neither is drawn, so
	# the farther copy starts opaque where the nearer one starts fading, wherever Godot puts
	# its fade band around a threshold, and only the copy going away dithers.
	for name: String in models:
		var buffer: PackedFloat32Array = models[name]
		var ordinary: MultiMeshInstance3D = _instance(mesh(name), buffer)
		ordinary.visibility_range_end = model_range
		ordinary.visibility_range_end_margin = MODEL_FADE
		# Quality sets this range, not View distance, as for models uploaded later.
		ordinary.set_meta("fixed_range", true)
		if detailed:
			var close_mesh: Mesh = mesh(name, true)
			if close_mesh != ordinary.multimesh.mesh:
				var close: MultiMeshInstance3D = _instance(close_mesh, buffer)
				close.visibility_range_end = minf(BuildingDetails.RANGE, model_range)
				close.visibility_range_end_margin = BuildingDetails.FADE
				close.set_meta("building_detail", true)
				close.set_meta("fixed_range", true)
				parent.add_child(close)
				ordinary.visibility_range_begin = maxf(
					close.visibility_range_end - BuildingDetails.FADE, 0.0
				)
		parent.add_child(ordinary)
	var shells: MeshInstance3D = MeshInstance3D.new()
	var shell_arrays: Dictionary = cell["shells"]
	shells.mesh = WorldMeshes.from_arrays(shell_arrays, true)
	shells.material_override = shell_material
	shells.visibility_range_begin = maxf(model_range - MODEL_FADE, 0.0)
	shells.visibility_range_end = shell_range
	shells.visibility_range_end_margin = 300.0
	shells.visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF
	parent.add_child(shells)


static func _instance(geometry: Mesh, buffer: PackedFloat32Array) -> MultiMeshInstance3D:
	var multi: MultiMesh = MultiMesh.new()
	multi.transform_format = MultiMesh.TRANSFORM_3D
	multi.use_colors = true
	multi.use_custom_data = true
	multi.mesh = geometry
	multi.instance_count = buffer.size() / 20
	multi.buffer = buffer
	var placed: MultiMeshInstance3D = MultiMeshInstance3D.new()
	placed.multimesh = multi
	placed.visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF
	return placed


## The mesh of model `name` with its materials replaced by the shared looks.
static func mesh(name: String, detailed: bool = false) -> Mesh:
	var cache: Dictionary = _detail_meshes if detailed else _meshes
	if cache.has(name):
		return cache[name]
	var path: String = DIRECTORY + ("detail/" if detailed else "") + name + ".glb"
	# A world kept from an older ORQWA may name a model this one no longer has (R101): it is
	# left out rather than breaking its chunk.
	if not ResourceLoader.exists(path):
		return mesh(name) if detailed else null
	var scene: PackedScene = load(path)
	var root: Node = scene.instantiate()
	var found: ArrayMesh = _find_mesh(root).mesh
	root.free()
	for surface: int in found.get_surface_count():
		var imported: Material = found.surface_get_material(surface)
		found.surface_set_material(surface, material(imported.resource_name))
	cache[name] = found
	return found


## Release Ultra mesh references when the rider returns to a lighter preset.
static func clear_detail_cache() -> void:
	_detail_meshes.clear()


## Shared palette values keep model and shell interiors consistent.
static func configure_interiors(shader_material: ShaderMaterial) -> void:
	shader_material.set_shader_parameter("interior_dark", Palette.color("buildings.interior_dark"))
	shader_material.set_shader_parameter(
		"interior_light", Palette.color("buildings.interior_light")
	)
	shader_material.set_shader_parameter("curtain_color", Palette.color("buildings.curtains"))


## The world and controlled review fixtures share the same shell material setup.
static func configure_shell(shader_material: ShaderMaterial) -> void:
	shader_material.shader = preload("res://shaders/building.gdshader")
	for uniform: String in ["glass", "frame", "stained_glass", "stone"]:
		shader_material.set_shader_parameter(uniform, Palette.color("buildings." + uniform))
	var textures: Dictionary[String, String] = {
		"plaster_texture": "building_plaster_albedo",
		"boards_texture": "planks_albedo",
		"tiles_texture": "building_tiles_albedo",
		"gravel_texture": "gravel_albedo",
		"plaster_rough_texture": "building_plaster_rough",
		"tiles_rough_texture": "building_tiles_rough",
	}
	for uniform: String in textures:
		shader_material.set_shader_parameter(
			uniform, load("res://assets/textures/%s.webp" % textures[uniform])
		)


## The shared material for a material name.
static func material(name: String) -> Material:
	if _materials.has(name):
		return _materials[name]
	var look: Array = LOOKS.get(name, LOOKS["plaster"])
	var colour_name: String = look[1]
	var shader_material: ShaderMaterial = ShaderMaterial.new()
	# Imported subresources can outlive the preset cache and be styled again after a switch.
	shader_material.resource_name = name
	shader_material.shader = preload("res://shaders/building_model.gdshader")
	shader_material.set_shader_parameter("pattern", look[0])
	shader_material.set_shader_parameter("color", _first(colour_name))
	var texture_name: String = look[2]
	if not texture_name.is_empty():
		shader_material.set_shader_parameter(
			"albedo_texture", load("res://assets/textures/%s_albedo.webp" % texture_name)
		)
		shader_material.set_shader_parameter(
			"normal_texture", load("res://assets/textures/%s_normal.webp" % texture_name)
		)
		shader_material.set_shader_parameter("textured", true)
		shader_material.set_shader_parameter("tile", look[3])
		if texture_name in ["building_plaster", "building_tiles"]:
			shader_material.set_shader_parameter(
				"roughness_texture", load("res://assets/textures/%s_rough.webp" % texture_name)
			)
			shader_material.set_shader_parameter("roughness_mapped", true)
	shader_material.set_shader_parameter("shutter_colors", Palette.colors("buildings.shutters"))
	shader_material.set_shader_parameter("flower_colors", Palette.colors("buildings.flowers"))
	shader_material.set_shader_parameter("gold", Palette.color("buildings.gold"))
	configure_interiors(shader_material)
	_materials[name] = shader_material
	return shader_material


## A palette colour, or the first of a palette list.
static func _first(path: String) -> Color:
	if Palette.is_list(path):
		return Palette.colors(path)[0]
	return Palette.color(path)


static func _find_mesh(node: Node) -> MeshInstance3D:
	if node is MeshInstance3D:
		return node
	for child: Node in node.get_children():
		var found: MeshInstance3D = _find_mesh(child)
		if found != null:
			return found
	return null
