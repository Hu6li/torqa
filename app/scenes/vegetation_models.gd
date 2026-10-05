class_name VegetationModels
extends RefCounted
## The vegetation models made in Blender (art/vegetation, ADR 0011): trees, bushes and rocks.
## Loads them and gives their materials the faceted look (app/shaders/vegetation.gdshader):
## leaves and rocks take each plant's instance colour, which the world picks from the palette,
## trunks share the palette's trunk colour.

const DIRECTORY: String = "res://assets/models/vegetation/"
## Per material name: colour by instance, and sway in the wind (metres at 10 m height).
const LOOKS: Dictionary[String, Array] = {
	"leaves": [true, 0.12],
	"trunk": [false, 0.12],
	"rock": [true, 0.0],
}

static var _materials: Dictionary[String, ShaderMaterial] = {}
static var _meshes: Dictionary[String, Mesh] = {}


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
static func material(name: String) -> ShaderMaterial:
	if _materials.has(name):
		return _materials[name]
	var look: Array = LOOKS.get(name, LOOKS["leaves"])
	var by_instance: bool = look[0]
	var shader_material: ShaderMaterial = ShaderMaterial.new()
	shader_material.shader = preload("res://shaders/vegetation.gdshader")
	shader_material.set_shader_parameter("albedo", Palette.color("plants.trunk"))
	shader_material.set_shader_parameter("instance_tint", 1.0 if by_instance else 0.0)
	shader_material.set_shader_parameter("sway", look[1])
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
