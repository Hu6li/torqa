class_name BuildingModels
extends RefCounted
## The building models made in Blender (art/buildings, R45): loads them and gives their
## materials the textures and palette colours their names ask for
## (app/shaders/building_model.gdshader, ADR 0020). Every building of a chunk is an instance of
## a MultiMesh: the instance colour is its plaster, the custom data its roof colour — or for
## flat-roofed offices, hotels and public buildings their accent colour — (rgb) and variant
## (a, 0–1).

const DIRECTORY: String = "res://assets/models/buildings/"

## Shader pattern, palette colour, texture (art/textures, ADR 0020) and its size in metres of
## each material name; materials without a texture are drawn by the shader alone.
const LOOKS: Dictionary[String, Array] = {
	"plaster": [0, "buildings.light_walls", "plaster", 2.0],
	"nordic_wood": [20, "buildings.timber", "planks", 1.5],
	"stone": [1, "buildings.stone", "masonry", 2.4],
	"castle_stone": [1, "buildings.stone", "rock", 2.4],
	"wood": [2, "buildings.timber", "planks", 2.0],
	"wood_dark": [3, "buildings.wood_dark", "planks", 2.0],
	"wood_light": [2, "buildings.wood_light", "planks", 2.0],
	"frame": [4, "buildings.frame", "", 1.0],
	"glass": [5, "buildings.glass", "", 1.0],
	"leaded": [17, "buildings.stained_glass", "", 1.0],
	"shutter": [6, "buildings.shutters", "planks", 1.0],
	"door": [7, "buildings.door", "planks", 1.0],
	"tiles": [8, "buildings.tiles", "roof_tiles", 2.0],
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
	for surface: int in found.get_surface_count():
		var imported: Material = found.surface_get_material(surface)
		found.surface_set_material(surface, material(imported.resource_name))
	_meshes[name] = found
	return found


## The shared material for a material name.
static func material(name: String) -> Material:
	if _materials.has(name):
		return _materials[name]
	var look: Array = LOOKS.get(name, LOOKS["plaster"])
	var colour_name: String = look[1]
	var shader_material: ShaderMaterial = ShaderMaterial.new()
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
	shader_material.set_shader_parameter("shutter_colors", Palette.colors("buildings.shutters"))
	shader_material.set_shader_parameter("flower_colors", Palette.colors("buildings.flowers"))
	shader_material.set_shader_parameter("gold", Palette.color("buildings.gold"))
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
