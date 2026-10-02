class_name RideWorld
extends Node3D
## The 3D world: terrain and road from Torqa, a rider following the ride state and cameras.

enum CameraMode { CHASE, FIRST_PERSON, DRONE }

## Chunks are turned into meshes gradually, so building the world never stalls a frame.
const CHUNKS_PER_FRAME: int = 6
## Chunks further than this are hidden; fog hides the edge.
const VISIBILITY_RANGE: float = 4500.0
const CAMERA_SMOOTHING: float = 6.0
const HEADING_SMOOTHING: float = 4.0
const JERSEY_COLOR: Color = Color(0.04, 0.61, 0.96)

var _torqa: TorqaApp
var _chunk_count: int = 0
var _next_chunk: int = 0
var _camera_mode: CameraMode = CameraMode.CHASE
var _heading: float = 0.0
var _placed: bool = false

var _terrain_material: ShaderMaterial = ShaderMaterial.new()
var _road_material: ShaderMaterial = ShaderMaterial.new()

@onready var _terrain: Node3D = $Terrain
@onready var _road: MeshInstance3D = $Road
@onready var _rider: Node3D = $Rider
@onready var _camera: Camera3D = $Camera


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_torqa.world_ready.connect(_on_world_ready)


## Cycles chase → first person → drone and returns the new mode's name.
func cycle_camera() -> String:
	_camera_mode = ((_camera_mode + 1) % CameraMode.size()) as CameraMode
	var mode_name: String = CameraMode.keys()[_camera_mode]
	return mode_name.capitalize()


## Snaps rider and camera to the start of a new ride instead of gliding there.
func reset_view() -> void:
	_placed = false


func _ready() -> void:
	_terrain_material.shader = preload("res://shaders/terrain.gdshader")
	_road_material.shader = preload("res://shaders/road.gdshader")
	_build_rider()


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


func _build_some_chunks() -> void:
	var built: int = 0
	while _next_chunk < _chunk_count and built < CHUNKS_PER_FRAME:
		var arrays: Dictionary = _torqa.world_chunk(_next_chunk)
		var chunk: MeshInstance3D = MeshInstance3D.new()
		chunk.mesh = _mesh_from(arrays)
		chunk.material_override = _terrain_material
		chunk.position = arrays["center"]
		chunk.visibility_range_end = VISIBILITY_RANGE
		chunk.visibility_range_end_margin = 300.0
		chunk.visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF
		_terrain.add_child(chunk)
		_next_chunk += 1
		built += 1


func _follow_ride(state: Dictionary, delta: float) -> void:
	var east: float = state["x"]
	var north: float = state["y"]
	var elevation: float = state["elevation_m"]
	var heading: float = state["heading"]
	var grade: float = state["grade"]
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
	var indices: PackedInt32Array = arrays["indices"]
	surface[Mesh.ARRAY_VERTEX] = vertices
	surface[Mesh.ARRAY_NORMAL] = normals
	surface[Mesh.ARRAY_TEX_UV] = uvs
	surface[Mesh.ARRAY_INDEX] = indices
	var mesh: ArrayMesh = ArrayMesh.new()
	if not vertices.is_empty():
		mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, surface)
	return mesh


## A simple stand-in cyclist until the real avatar arrives.
func _build_rider() -> void:
	var dark: StandardMaterial3D = _material(Color(0.08, 0.08, 0.09))
	var jersey: StandardMaterial3D = _material(JERSEY_COLOR)
	var skin: StandardMaterial3D = _material(Color(0.86, 0.68, 0.55))
	for z: float in [-0.52, 0.52]:
		var wheel: TorusMesh = TorusMesh.new()
		wheel.inner_radius = 0.31
		wheel.outer_radius = 0.35
		_add_part(wheel, dark, Vector3(0, 0.35, z), Vector3(0, 0, PI / 2.0))
	var frame: BoxMesh = BoxMesh.new()
	frame.size = Vector3(0.05, 0.05, 1.0)
	_add_part(frame, jersey, Vector3(0, 0.62, 0), Vector3(deg_to_rad(-8.0), 0, 0))
	var body: CapsuleMesh = CapsuleMesh.new()
	body.radius = 0.17
	body.height = 0.75
	_add_part(body, jersey, Vector3(0, 1.12, 0.05), Vector3(deg_to_rad(-55.0), 0, 0))
	var head: SphereMesh = SphereMesh.new()
	head.radius = 0.12
	head.height = 0.24
	_add_part(head, skin, Vector3(0, 1.42, -0.32), Vector3.ZERO)


func _add_part(mesh: Mesh, material: Material, position: Vector3, rotation_rad: Vector3) -> void:
	var part: MeshInstance3D = MeshInstance3D.new()
	part.mesh = mesh
	part.material_override = material
	part.position = position
	part.rotation = rotation_rad
	_rider.add_child(part)


static func _material(color: Color) -> StandardMaterial3D:
	var material: StandardMaterial3D = StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = 0.7
	return material
