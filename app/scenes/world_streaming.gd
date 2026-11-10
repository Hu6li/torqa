class_name WorldStreaming
extends RefCounted
## Upload the rider's visible surroundings first, then the remaining scene (R224).

## Covers visibility fading, camera offset and movement between priority updates.
const LOOK_AHEAD: float = 1000.0
const REORDER_DISTANCE: float = 240.0
## A background upload may finish its current chunk, but must then yield to rendering.
const FRAME_BUDGET_US: int = 4000

var _order: PackedInt32Array = PackedInt32Array()
var _required: PackedInt32Array = PackedInt32Array()
var _uploaded: Dictionary[int, bool] = {}
var _cursor: int = 0
var _focus: Vector2 = Vector2(INF, INF)
var _radius: float = 0.0


func reset() -> void:
	_order.clear()
	_required.clear()
	_uploaded.clear()
	_cursor = 0
	_focus = Vector2(INF, INF)
	_radius = 0.0


## The core computes spatial order without changing the generated or kept world.
func prioritize(world: RideWorld, focus: Vector2) -> void:
	var radius: float = RideWorld.VISIBILITY_RANGE * world._distance + LOOK_AHEAD
	var plan: Dictionary = world._orqwa.world_chunk_plan(focus.x, focus.y, radius)
	if plan.is_empty():
		return
	var indices: PackedInt32Array = plan["indices"]
	var nearby: int = plan["nearby"]
	_required = indices.slice(0, nearby)
	_order.clear()
	for index: int in indices:
		if not _uploaded.has(index):
			_order.append(index)
	_cursor = 0
	_focus = focus
	_radius = radius


## Full-resolution ground already draws while its optional LODs are made in the background.
func nearby_ready() -> bool:
	var progress: Vector2i = nearby_progress()
	return progress.y > 0 and progress.x == progress.y


func nearby_progress() -> Vector2i:
	var done: int = 0
	for index: int in _required:
		if _uploaded.has(index):
			done += 1
	return Vector2i(done, _required.size())


func upload_some(world: RideWorld) -> void:
	if world._next_chunk >= world._chunk_count:
		return
	if world._benchmark == null:
		var state: Dictionary = world._orqwa.ride_state()
		var focus: Vector2 = _focus if _focus.is_finite() else Vector2.ZERO
		if state.has("rider_x"):
			var east: float = state["rider_x"]
			var north: float = state["rider_y"]
			focus = Vector2(east, north)
		var radius: float = RideWorld.VISIBILITY_RANGE * world._distance + LOOK_AHEAD
		if _focus.distance_to(focus) >= REORDER_DISTANCE or radius != _radius:
			prioritize(world, focus)
		if _cursor >= _order.size():
			return
	var began: int = Time.get_ticks_usec()
	var built: int = 0
	while world._next_chunk < world._chunk_count and built < RideWorld.CHUNKS_PER_FRAME:
		var index: int = 0 if world._benchmark != null else _order[_cursor]
		var density: float = world._quality["tree_density"]
		var chunk: Dictionary = (
			world._benchmark.chunk(density)
			if world._benchmark != null
			else world._orqwa.world_chunk_for_season(index, density, world._seasons.season)
		)
		# The core drops old geometry as soon as another course is opened. A chunk it cannot
		# read from disk stays undrawn (ADR 0040).
		if chunk.is_empty() and world._orqwa.world_chunk_count() != world._chunk_count:
			world._chunk_count = 0
			reset()
			return
		if not chunk.is_empty():
			_add_chunk(world, chunk)
		_uploaded[index] = true
		_cursor += 1
		world._next_chunk += 1
		built += 1
		if world.visible and Time.get_ticks_usec() - began >= FRAME_BUDGET_US:
			break


func _add_chunk(world: RideWorld, chunk: Dictionary) -> void:
	var center: Vector3 = chunk["center"]
	var node: Node3D = Node3D.new()
	node.position = center
	world._terrain.add_child(node)
	var terrain_arrays: Dictionary = chunk["terrain"]
	var building_arrays: Dictionary = chunk["buildings"]
	var ground: MeshInstance3D = world._mesh_instance(terrain_arrays, world._terrain_material)
	world._ground_lods.make(ground, terrain_arrays)
	ground.visibility_range_end = RideWorld.VISIBILITY_RANGE * world._distance
	node.add_child(ground)
	for surface: Array in [
		["streets", world._street_material],
		["tracks", world._track_material],
		["water", world._water_material]
	]:
		var arrays: Dictionary = chunk.get(surface[0], {})
		var vertices: PackedVector3Array = arrays.get("vertices", PackedVector3Array())
		if not vertices.is_empty():
			var material: ShaderMaterial = surface[1]
			var way: MeshInstance3D = world._mesh_instance(arrays, material)
			way.visibility_range_end = RideWorld.VISIBILITY_RANGE * world._distance
			if material == world._track_material:
				way.visibility_range_end = RideWorld.TRACK_RANGE
				way.set_meta("fixed_range", true)
			node.add_child(way)
	UrbanSurfaces.add_to(node, chunk, RideWorld.SIDEWALK_RANGE)
	var buildings: MeshInstance3D = world._mesh_instance(building_arrays, world._building_material)
	buildings.visibility_range_end = RideWorld.DETAIL_RANGE * world._distance
	node.add_child(buildings)
	var modelled: Array = chunk.get("modelled", [])
	for cell: Dictionary in modelled:
		world._add_modelled(node, cell)
	var plants: Dictionary = chunk.get("plants", {})
	var tree_range: float = world._quality["tree_range"]
	var tree_cell: float = world._quality["tree_cell"]
	for model: String in plants:
		var buffer: PackedFloat32Array = plants[model]
		VegetationModels.add_to(
			node,
			model,
			buffer,
			tree_range,
			RideWorld.SMALL_PLANT_RANGE * world._distance,
			tree_cell
		)
	var props: Dictionary = chunk.get("props", {})
	for model: String in props:
		var buffer: PackedFloat32Array = props[model]
		node.add_child(world._props(model, buffer))
	var wires: Dictionary = chunk.get("wires", {})
	var wire_vertices: PackedVector3Array = wires.get("vertices", PackedVector3Array())
	if not wire_vertices.is_empty():
		var wire_mesh: MeshInstance3D = world._mesh_instance(wires, PropModels.wire_material())
		wire_mesh.visibility_range_end = RideWorld.DETAIL_RANGE * world._distance
		node.add_child(wire_mesh)
	var labels: Array = chunk.get("labels", [])
	for entry: Dictionary in labels:
		var text: Label3D = PropModels.label(entry)
		text.visibility_range_end = RideWorld.SMALL_PLANT_RANGE * world._distance
		node.add_child(text)
	var grass_range: float = world._quality["grass_range"]
	if grass_range > 0.0:
		for kind: String in ["grass", "flowers"]:
			var tufts: PackedFloat32Array = chunk.get(kind, PackedFloat32Array())
			if not tufts.is_empty():
				node.add_child(world._plants(tufts, kind == "flowers", grass_range))
