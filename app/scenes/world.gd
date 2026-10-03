class_name RideWorld
extends Node3D
## The 3D world: terrain and road from Torqa, a rider following the ride state and cameras.

enum CameraMode { CHASE, FIRST_PERSON, DRONE }

## Sun elevation and azimuth (degrees, azimuth clockwise from north), colour and energy, and
## sky top/horizon colours per time of day.
const TIMES: Dictionary[String, Dictionary] = {
	"Morning":
	{
		"elevation": 14.0,
		"azimuth": 110.0,
		"sun": Color(1.0, 0.82, 0.66),
		"energy": 0.95,
		"top": Color(0.36, 0.55, 0.82),
		"horizon": Color(0.92, 0.78, 0.66),
	},
	"Midday":
	{
		"elevation": 55.0,
		"azimuth": 190.0,
		"sun": Color(1.0, 0.98, 0.94),
		"energy": 1.2,
		"top": Color(0.32, 0.52, 0.82),
		"horizon": Color(0.68, 0.78, 0.88),
	},
	"Evening":
	{
		"elevation": 9.0,
		"azimuth": 265.0,
		"sun": Color(1.0, 0.62, 0.38),
		"energy": 0.85,
		"top": Color(0.28, 0.36, 0.62),
		"horizon": Color(0.98, 0.62, 0.42),
	},
}
const WEATHERS: Array[String] = ["Clear", "Cloudy", "Hazy", "Rain"]

## Chunks are turned into meshes gradually, so building the world never stalls a frame.
const CHUNKS_PER_FRAME: int = 6
## Chunks further than this are hidden; fog hides the edge.
const VISIBILITY_RANGE: float = 4500.0
## Trees and buildings are small; beyond this the land-cover colours carry the scene.
const DETAIL_RANGE: float = 1800.0
const CAMERA_SMOOTHING: float = 6.0
const HEADING_SMOOTHING: float = 4.0

var _torqa: TorqaApp
var _chunk_count: int = 0
var _next_chunk: int = 0
var _camera_mode: CameraMode = CameraMode.CHASE
var _heading: float = 0.0
var _placed: bool = false
var _avatar: RiderAvatar = RiderAvatar.new()

var _terrain_material: ShaderMaterial = ShaderMaterial.new()
var _road_material: ShaderMaterial = ShaderMaterial.new()
var _water_material: ShaderMaterial = ShaderMaterial.new()
var _building_material: StandardMaterial3D = StandardMaterial3D.new()
var _conifer_mesh: ArrayMesh
var _broadleaf_mesh: ArrayMesh

@onready var _terrain: Node3D = $Terrain
@onready var _road: MeshInstance3D = $Road
@onready var _water: MeshInstance3D = $Water
@onready var _structures: MeshInstance3D = $Structures
@onready var _rider: Node3D = $Rider
@onready var _camera: Camera3D = $Camera
@onready var _sun: DirectionalLight3D = $Sun
@onready var _environment: Environment = ($Environment as WorldEnvironment).environment
@onready var _rain: GPUParticles3D = $Camera/Rain


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_torqa.world_ready.connect(_on_world_ready)


## Cycles chase → first person → drone and returns the new mode's name.
func cycle_camera() -> String:
	_camera_mode = ((_camera_mode + 1) % CameraMode.size()) as CameraMode
	var mode_name: String = CameraMode.keys()[_camera_mode]
	return mode_name.capitalize()


## Sets the light, sky, fog and precipitation for a time of day and a weather.
func apply_conditions(time_of_day: String, weather: String) -> void:
	var time: Dictionary = TIMES.get(time_of_day, TIMES["Midday"])
	var elevation: float = time["elevation"]
	var azimuth: float = time["azimuth"]
	# The light shines along its −z axis: from the sun's direction towards the ground.
	_sun.rotation = Vector3(deg_to_rad(-elevation), deg_to_rad(180.0 - azimuth), 0.0)
	var sky: ProceduralSkyMaterial = _environment.sky.sky_material as ProceduralSkyMaterial
	var top: Color = time["top"]
	var horizon: Color = time["horizon"]
	var sun_color: Color = time["sun"]
	var energy: float = time["energy"]
	var overcast: float = 0.0
	var fog: float = 0.00035
	match weather:
		"Cloudy":
			overcast = 0.75
		"Hazy":
			overcast = 0.3
			fog = 0.0016
		"Rain":
			overcast = 1.0
			fog = 0.0012
	var grey: Color = Color(0.6, 0.62, 0.65)
	sky.sky_top_color = top.lerp(grey * 0.8, overcast)
	sky.sky_horizon_color = horizon.lerp(grey, overcast)
	sky.ground_horizon_color = sky.sky_horizon_color
	sky.sun_angle_max = lerpf(25.0, 0.0, overcast)
	_sun.light_color = sun_color.lerp(Color.WHITE, overcast * 0.5)
	_sun.light_energy = energy * lerpf(1.0, 0.35, overcast)
	_sun.shadow_blur = lerpf(1.0, 4.0, overcast)
	_environment.ambient_light_energy = lerpf(0.9, 1.25, overcast)
	_environment.fog_density = fog
	_environment.fog_light_color = sky.sky_horizon_color
	_rain.emitting = weather == "Rain"
	_road_material.set_shader_parameter("wetness", 1.0 if weather == "Rain" else 0.0)


## Snaps rider and camera to the start of a new ride instead of gliding there.
func reset_view() -> void:
	_placed = false


func _ready() -> void:
	_terrain_material.shader = preload("res://shaders/terrain.gdshader")
	_road_material.shader = preload("res://shaders/road.gdshader")
	_water_material.shader = preload("res://shaders/water.gdshader")
	_building_material.vertex_color_use_as_albedo = true
	_building_material.vertex_color_is_srgb = true
	_building_material.roughness = 0.9
	_conifer_mesh = _tree_mesh(true)
	_broadleaf_mesh = _tree_mesh(false)
	_rider.add_child(_avatar)
	apply_conditions("Midday", "Clear")


func _process(delta: float) -> void:
	_build_some_chunks()
	if not visible or _torqa == null:
		return
	var state: Dictionary = _torqa.ride_state()
	if state.is_empty():
		return
	_follow_ride(state, delta)


func _on_world_ready(_info: Dictionary) -> void:
	for chunk: Node in _terrain.get_children():
		chunk.queue_free()
	_chunk_count = _torqa.world_chunk_count()
	_next_chunk = 0
	_road.mesh = _mesh_from(_torqa.road_mesh())
	_road.material_override = _road_material
	_water.mesh = _mesh_from(_torqa.water_mesh())
	_water.material_override = _water_material
	_structures.mesh = _mesh_from(_torqa.structures_mesh())
	_structures.material_override = _building_material


func _build_some_chunks() -> void:
	var built: int = 0
	while _next_chunk < _chunk_count and built < CHUNKS_PER_FRAME:
		var chunk: Dictionary = _torqa.world_chunk(_next_chunk)
		var center: Vector3 = chunk["center"]
		var node: Node3D = Node3D.new()
		node.position = center
		_terrain.add_child(node)
		var terrain_arrays: Dictionary = chunk["terrain"]
		var building_arrays: Dictionary = chunk["buildings"]
		var ground: MeshInstance3D = _mesh_instance(terrain_arrays, _terrain_material)
		ground.visibility_range_end = VISIBILITY_RANGE
		node.add_child(ground)
		var buildings: MeshInstance3D = _mesh_instance(building_arrays, _building_material)
		buildings.visibility_range_end = DETAIL_RANGE
		node.add_child(buildings)
		for kind: String in ["conifers", "broadleaves"]:
			var buffer: PackedFloat32Array = chunk[kind]
			if buffer.is_empty():
				continue
			var mesh: ArrayMesh = _conifer_mesh if kind == "conifers" else _broadleaf_mesh
			node.add_child(_trees(buffer, mesh))
		_next_chunk += 1
		built += 1


func _mesh_instance(arrays: Dictionary, material: Material) -> MeshInstance3D:
	var instance: MeshInstance3D = MeshInstance3D.new()
	instance.mesh = _mesh_from(arrays)
	instance.material_override = material
	instance.visibility_range_end_margin = 300.0
	instance.visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF
	return instance


func _trees(buffer: PackedFloat32Array, mesh: ArrayMesh) -> MultiMeshInstance3D:
	var multimesh: MultiMesh = MultiMesh.new()
	multimesh.transform_format = MultiMesh.TRANSFORM_3D
	multimesh.mesh = mesh
	multimesh.instance_count = buffer.size() / 12
	multimesh.buffer = buffer
	var instance: MultiMeshInstance3D = MultiMeshInstance3D.new()
	instance.multimesh = multimesh
	instance.visibility_range_end = DETAIL_RANGE
	instance.visibility_range_end_margin = 300.0
	instance.visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF
	return instance


func _follow_ride(state: Dictionary, delta: float) -> void:
	var east: float = state["x"]
	var north: float = state["y"]
	var elevation: float = state["elevation_m"]
	var heading: float = state["heading"]
	var grade: float = state["grade"]
	var speed_kmh: float = state["speed_kmh"]
	var cadence: Variant = state["cadence"]
	var cadence_rpm: float = cadence if cadence != null else 0.0
	_avatar.animate(delta, cadence_rpm, speed_kmh)
	var weight: float = 1.0 - exp(-delta * HEADING_SMOOTHING)
	_heading = heading if not _placed else lerp_angle(_heading, heading, weight)

	# Godot looks along −z (north); headings run clockwise from north, rotations anticlockwise.
	var yaw: Basis = Basis(Vector3.UP, -_heading)
	var pitch: Basis = Basis(Vector3.RIGHT, atan(grade / 100.0))
	_rider.transform = Transform3D(yaw * pitch, Vector3(east, elevation, -north))

	var target: Transform3D = _camera_target(_rider.transform)
	if _placed:
		var camera_weight: float = 1.0 - exp(-delta * CAMERA_SMOOTHING)
		_camera.transform = _camera.transform.interpolate_with(target, camera_weight)
	else:
		_camera.transform = target
		_placed = true


func _camera_target(rider: Transform3D) -> Transform3D:
	var forward: Vector3 = -rider.basis.z
	var flat_forward: Vector3 = Vector3(forward.x, 0.0, forward.z).normalized()
	var origin: Vector3 = rider.origin
	var eye: Vector3
	var look_at: Vector3
	match _camera_mode:
		CameraMode.FIRST_PERSON:
			eye = origin + Vector3.UP * 1.55 + forward * 0.4
			look_at = eye + forward * 20.0
		CameraMode.DRONE:
			eye = origin - flat_forward * 28.0 + Vector3.UP * 20.0
			look_at = origin + flat_forward * 10.0
		_:
			eye = origin - flat_forward * 6.5 + Vector3.UP * 2.6
			look_at = origin + flat_forward * 10.0 + Vector3.UP * 1.0
	return Transform3D(Basis.IDENTITY, eye).looking_at(look_at, Vector3.UP)


func _mesh_from(arrays: Dictionary) -> ArrayMesh:
	var surface: Array = []
	surface.resize(Mesh.ARRAY_MAX)
	var vertices: PackedVector3Array = arrays["vertices"]
	var normals: PackedVector3Array = arrays["normals"]
	var uvs: PackedVector2Array = arrays["uvs"]
	var colors: PackedColorArray = arrays["colors"]
	var indices: PackedInt32Array = arrays["indices"]
	surface[Mesh.ARRAY_VERTEX] = vertices
	surface[Mesh.ARRAY_NORMAL] = normals
	surface[Mesh.ARRAY_TEX_UV] = uvs
	if not colors.is_empty():
		surface[Mesh.ARRAY_COLOR] = colors
	surface[Mesh.ARRAY_INDEX] = indices
	var mesh: ArrayMesh = ArrayMesh.new()
	if not vertices.is_empty():
		mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, surface)
	return mesh


## A low-poly tree standing on its origin: a trunk with a cone (conifer) or ball crown.
func _tree_mesh(conifer: bool) -> ArrayMesh:
	var mesh: ArrayMesh = ArrayMesh.new()
	var trunk: CylinderMesh = CylinderMesh.new()
	trunk.top_radius = 0.15
	trunk.bottom_radius = 0.25
	trunk.height = 3.0
	trunk.radial_segments = 6
	trunk.rings = 1
	_add_surface(mesh, trunk, Vector3(0, 1.5, 0), _material(Color(0.33, 0.24, 0.17)))
	if conifer:
		var crown: CylinderMesh = CylinderMesh.new()
		crown.top_radius = 0.0
		crown.bottom_radius = 2.3
		crown.height = 10.0
		crown.radial_segments = 8
		crown.rings = 1
		_add_surface(mesh, crown, Vector3(0, 7.0, 0), _material(Color(0.1, 0.24, 0.13)))
	else:
		var crown: SphereMesh = SphereMesh.new()
		crown.radius = 3.0
		crown.height = 5.5
		crown.radial_segments = 8
		crown.rings = 4
		_add_surface(mesh, crown, Vector3(0, 5.5, 0), _material(Color(0.2, 0.36, 0.14)))
	return mesh


func _add_surface(
	mesh: ArrayMesh, shape: PrimitiveMesh, offset: Vector3, material: Material
) -> void:
	var arrays: Array = shape.get_mesh_arrays()
	var vertices: PackedVector3Array = arrays[Mesh.ARRAY_VERTEX]
	for i: int in range(vertices.size()):
		vertices[i] += offset
	arrays[Mesh.ARRAY_VERTEX] = vertices
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	mesh.surface_set_material(mesh.get_surface_count() - 1, material)


static func _material(color: Color) -> StandardMaterial3D:
	var material: StandardMaterial3D = StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = 0.7
	return material
