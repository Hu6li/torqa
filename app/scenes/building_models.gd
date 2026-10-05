class_name BuildingModels
extends RefCounted
## The building models made in Blender (art/buildings, R45): loads them and gives their
## materials the look their names ask for (app/shaders/building_model.gdshader). Every
## building of a chunk is an instance of a MultiMesh: the instance colour is its plaster,
## the custom data its roof colour (rgb) and variant (a, 0–1).

const DIRECTORY: String = "res://assets/models/buildings/"

## Shader pattern, colour (sRGB) and surface of each material name.
const LOOKS: Dictionary = {
	"plaster": [0, Color(0.86, 0.83, 0.77), 0.92, 0.0],
	"stone": [1, Color(0.6, 0.58, 0.55), 0.9, 0.0],
	"wood": [2, Color(0.5, 0.34, 0.2), 0.85, 0.0],
	"wood_dark": [3, Color(0.3, 0.19, 0.11), 0.85, 0.0],
	"frame": [4, Color(0.93, 0.93, 0.91), 0.45, 0.0],
	"glass": [5, Color(0.1, 0.12, 0.14), 0.06, 0.0],
	"leaded": [17, Color(0.12, 0.13, 0.15), 0.06, 0.0],
	"shutter": [6, Color(0.22, 0.38, 0.26), 0.7, 0.0],
	"door": [7, Color(0.4, 0.25, 0.14), 0.7, 0.0],
	"tiles": [8, Color(0.55, 0.26, 0.17), 0.8, 0.0],
	"slate": [9, Color(0.25, 0.26, 0.29), 0.7, 0.0],
	"metal": [10, Color(0.56, 0.57, 0.59), 0.45, 0.6],
	"copper": [11, Color(0.36, 0.56, 0.48), 0.6, 0.2],
	"flowers": [12, Color(0.85, 0.08, 0.1), 0.8, 0.0],
	"leaves": [13, Color(0.14, 0.33, 0.1), 0.85, 0.0],
	"clock": [14, Color(0.1, 0.1, 0.1), 0.4, 0.0],
	"sheet": [15, Color(0.45, 0.46, 0.48), 0.45, 0.5],
	"garage": [16, Color(0.82, 0.82, 0.8), 0.5, 0.2],
	"wood_light": [2, Color(0.74, 0.6, 0.42), 0.85, 0.0],
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
	var shader_material: ShaderMaterial = ShaderMaterial.new()
	shader_material.shader = preload("res://shaders/building_model.gdshader")
	shader_material.set_shader_parameter("pattern", look[0])
	shader_material.set_shader_parameter("color", look[1])
	shader_material.set_shader_parameter("roughness", look[2])
	shader_material.set_shader_parameter("metallic", look[3])
	_materials[name] = shader_material
	return shader_material


static func _find_mesh(node: Node) -> MeshInstance3D:
	if node is MeshInstance3D:
		return node
	for child: Node in node.get_children():
		var found: MeshInstance3D = _find_mesh(child)
		if found != null:
			return found
	return null
