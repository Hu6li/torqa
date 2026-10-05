class_name BuildingModels
extends RefCounted
## The building models made in Blender (art/buildings, R45): loads them and gives their
## materials the flat palette colours their names ask for (app/shaders/building_model.gdshader,
## ADR 0011). Every building of a chunk is an instance of a MultiMesh: the instance colour is
## its plaster, the custom data its roof colour (rgb) and variant (a, 0–1).

const DIRECTORY: String = "res://assets/models/buildings/"

## Shader pattern and palette colour of each material name.
const LOOKS: Dictionary[String, Array] = {
	"plaster": [0, "buildings.light_walls"],
	"stone": [1, "buildings.stone"],
	"wood": [2, "buildings.timber"],
	"wood_dark": [3, "buildings.wood_dark"],
	"wood_light": [2, "buildings.wood_light"],
	"frame": [4, "buildings.frame"],
	"glass": [5, "buildings.glass"],
	"leaded": [17, "buildings.stained_glass"],
	"shutter": [6, "buildings.shutters"],
	"door": [7, "buildings.door"],
	"tiles": [8, "buildings.tiles"],
	"slate": [9, "buildings.slate"],
	"metal": [10, "buildings.metal"],
	"copper": [11, "buildings.copper"],
	"flowers": [12, "buildings.flowers"],
	"leaves": [13, "buildings.leaves"],
	"clock": [14, "buildings.clock"],
	"sheet": [15, "buildings.sheet"],
	"garage": [16, "buildings.garage"],
}

static var _materials: Dictionary = {}
static var _meshes: Dictionary = {}


## The mesh of model `name` with its materials replaced by the shared looks.
static func mesh(name: String) -> Mesh:
	if _meshes.has(name):
		return _meshes[name]
	var scene: PackedScene = load(DIRECTORY + name + ".glb")
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
