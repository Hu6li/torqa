class_name BuildingDetails
extends RefCounted
## Close Ultra façades and model variants share a short range independently of view distance.

const RANGE: float = 160.0
const FADE: float = 20.0

static var _material: ShaderMaterial


static func material() -> ShaderMaterial:
	if _material == null:
		_material = ShaderMaterial.new()
		_material.shader = preload("res://shaders/building_detail.gdshader")
		BuildingModels.configure_interiors(_material)
	return _material


## The core has already grouped these surfaces into small cells for distance culling.
static func add_to(parent: Node3D, arrays: Array) -> void:
	for surface: Dictionary in arrays:
		var mesh: ArrayMesh = WorldMeshes.from_arrays(surface)
		if mesh.get_surface_count() == 0:
			continue
		var placed: MeshInstance3D = MeshInstance3D.new()
		placed.mesh = mesh
		placed.material_override = material()
		placed.visibility_range_end = RANGE
		placed.visibility_range_end_margin = FADE
		placed.visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF
		placed.set_meta("building_detail", true)
		placed.set_meta("fixed_range", true)
		parent.add_child(placed)
