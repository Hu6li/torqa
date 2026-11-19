class_name WorldStreaming
extends RefCounted
## Upload the rider's visible surroundings, nearest first, and let go of the chunks left far
## behind (R224, ADR 0040): the scene's memory follows the drawn distance, not the course's
## length. Chunks are freed only well beyond where they are uploaded, so turning back on a
## hairpin doesn't read them again.

## Covers visibility fading, camera offset and movement between priority updates.
const LOOK_AHEAD: float = 1000.0
## A background upload may finish its current chunk, but must then yield to rendering.
const FRAME_BUDGET_US: int = 4000
## The land under a chunk (#560) shows from this much before the chunk's ground ends: that ground
## fades out over its last few hundred metres (`RideWorld._mesh_instance`), and there must be
## land below it while it does.
const STAND_IN_OVERLAP: float = 300.0

var _order: PackedInt32Array = PackedInt32Array()
var _required: PackedInt32Array = PackedInt32Array()
## How many chunks within reach are not built yet: the world is ridden while the rest of it is
## built (ADR 0040).
var _missing: int = 0
## Chunk index → its node, or null for a chunk that couldn't be read (it is tried again once it
## has been let go and comes back within reach).
var _uploaded: Dictionary[int, Node3D] = {}
var _cursor: int = 0
var _focus: Vector2 = Vector2(INF, INF)
var _radius: float = 0.0
var _replan: float = 0.0
## The chunks in the scene changed since the land filling in for the others was made.
var _fill_stale: bool = true
## That land: "ground" and "water" → its mesh instance, made with the first fill.
var _fill: Dictionary[String, MeshInstance3D] = {}


func reset() -> void:
	_order.clear()
	_fill_stale = true
	_required.clear()
	_missing = 0
	_uploaded.clear()
	_cursor = 0
	_focus = Vector2(INF, INF)
	_radius = 0.0
	_replan = 0.0


## The core computes spatial order without changing the generated or kept world.
func prioritize(world: RideWorld, focus: Vector2) -> void:
	var radius: float = _upload_radius(world)
	var plan: Dictionary = world._orqwa.world_chunk_plan(focus.x, focus.y, radius)
	if plan.is_empty():
		return
	var indices: PackedInt32Array = plan["indices"]
	var nearby: int = plan["nearby"]
	var kept: int = plan["kept"]
	_required = indices.slice(0, nearby)
	_order.clear()
	_missing = 0
	for index: int in _required:
		if _uploaded.has(index):
			continue
		if world._orqwa.world_chunk_finished(index):
			_order.append(index)
		else:
			_missing += 1
	var reach: Dictionary[int, bool] = {}
	for index: int in indices.slice(0, kept):
		reach[index] = true
	for index: int in _uploaded.keys():
		if not reach.has(index):
			_let_go(world, index)
	_cursor = 0
	_focus = focus
	_radius = radius
	_replan = plan["replan"]


## Plans again once the rider (or the free camera) has moved on, or the drawn distance changed.
## A jump calls this at once, so its surroundings are known before the next frame's upload.
func follow(world: RideWorld) -> void:
	var focus: Vector2 = _current_focus(world)
	if _focus.distance_to(focus) >= _replan or _upload_radius(world) != _radius:
		prioritize(world, focus)


## More of the world was built: the chunks within reach that were missing are uploaded once
## they are there.
func grew(world: RideWorld) -> void:
	if _missing > 0 and _focus.is_finite():
		prioritize(world, _focus)


## Chunks within reach are still being built.
func missing() -> bool:
	return _missing > 0


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


## Chunks in the scene now, or tried and found unreadable.
func uploaded() -> int:
	return _uploaded.size()


## The surroundings of the last plan are all in the scene.
func complete(world: RideWorld) -> bool:
	if world._benchmark != null:
		return not _uploaded.is_empty()
	return (
		world._chunk_count == 0
		or (_focus.is_finite() and _cursor >= _order.size() and _missing == 0)
	)


func upload_some(world: RideWorld) -> void:
	if world._chunk_count == 0:
		return
	if world._benchmark != null:
		if _uploaded.is_empty():
			var density: float = world._quality["tree_density"]
			_uploaded[0] = _add_chunk(world, world._benchmark.chunk(density), -1)
		return
	follow(world)
	var began: int = Time.get_ticks_usec()
	var built: int = 0
	while _cursor < _order.size() and built < RideWorld.CHUNKS_PER_FRAME:
		if not world._orqwa.check_world_memory():
			return
		var index: int = _order[_cursor]
		var density: float = world._quality["tree_density"]
		var chunk: Dictionary = world._orqwa.world_chunk_for_season(
			index, density, world._seasons.season
		)
		# The core drops old geometry as soon as another course is opened. A chunk it cannot
		# read from disk stays undrawn (ADR 0040).
		if chunk.is_empty() and world._orqwa.world_chunk_count() != world._chunk_count:
			world._chunk_count = 0
			reset()
			return
		_uploaded[index] = null if chunk.is_empty() else _add_chunk(world, chunk, index)
		_fill_stale = true
		_cursor += 1
		built += 1
		if world.visible and Time.get_ticks_usec() - began >= FRAME_BUDGET_US:
			break


func _upload_radius(world: RideWorld) -> float:
	return RideWorld.VISIBILITY_RANGE * world._distance + LOOK_AHEAD


## The free camera of a simulated ride takes its surroundings with it; otherwise the rider's
## position leads, as the following camera is only metres away and lags a jump by a frame.
func _current_focus(world: RideWorld) -> Vector2:
	if world._free:
		var camera: Vector3 = world._camera.global_position
		return Vector2(camera.x, -camera.z)
	var state: Dictionary = world._orqwa.ride_state()
	if state.has("rider_x"):
		var east: float = state["rider_x"]
		var north: float = state["rider_y"]
		return Vector2(east, north)
	return _focus if _focus.is_finite() else Vector2.ZERO


## The world's distant land was made again (`OrqwaApp.horizon_meshes`).
func horizon_changed() -> void:
	_fill_stale = true


## Makes the land filling in for the chunks not in the scene again, once they changed: it
## covers the route's corridor where its chunks aren't loaded, are still being built or could
## not be read (#560).
func update_fill(world: RideWorld) -> void:
	if not _fill_stale:
		return
	_fill_stale = false
	var drawn: PackedInt32Array = PackedInt32Array()
	for index: int in _uploaded:
		if _uploaded[index] != null:
			drawn.append(index)
	var land: Dictionary = world._orqwa.horizon_fill(drawn)
	for surface: Array in [["ground", world._terrain_material], ["water", world._water_material]]:
		var kind: String = surface[0]
		if not _fill.has(kind):
			var instance: MeshInstance3D = MeshInstance3D.new()
			# Far away: its shadows would not show.
			instance.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
			world.add_child(instance)
			_fill[kind] = instance
		var arrays: Dictionary = land.get(kind, {})
		_fill[kind].mesh = null if arrays.is_empty() else WorldMeshes.from_arrays(arrays)
		_fill[kind].material_override = surface[1]


## Ground is drawn only so far: beyond, the chunk's distant land stands in for it (#560).
func _add_stand_in(world: RideWorld, node: Node3D, index: int) -> void:
	var land: Dictionary = world._orqwa.horizon_under(index)
	for surface: Array in [["ground", world._terrain_material], ["water", world._water_material]]:
		var arrays: Dictionary = land.get(surface[0], {})
		var vertices: PackedVector3Array = arrays.get("vertices", PackedVector3Array())
		if vertices.is_empty():
			continue
		var stand_in: MeshInstance3D = MeshInstance3D.new()
		stand_in.mesh = WorldMeshes.from_arrays(arrays)
		stand_in.material_override = surface[1]
		# The land is in route coordinates, the chunk's node at its centre.
		stand_in.position = -node.position
		stand_in.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		stand_in.set_meta("stand_in", true)
		follow_distance(stand_in, world._distance)
		node.add_child(stand_in)


## Where the land under a chunk begins to show, for the drawn distance `factor`.
static func stand_in_begin(factor: float) -> float:
	return maxf(RideWorld.VISIBILITY_RANGE * factor - STAND_IN_OVERLAP, 0.0)


## Sets the range of a chunk's stand-in or water for the drawn distance `factor`. The water
## fades out only once the ground under it has: fading over it, it showed the lake bed where the
## land under the chunk has no lake of its own, by the shore (#560).
static func follow_distance(geometry: GeometryInstance3D, factor: float) -> void:
	if geometry.has_meta("stand_in"):
		geometry.visibility_range_begin = stand_in_begin(factor)
	else:
		geometry.visibility_range_end = RideWorld.VISIBILITY_RANGE * factor + STAND_IN_OVERLAP


## Its ground versions still being made are dropped when they finish (`GroundLods.add_ready`).
func _let_go(world: RideWorld, index: int) -> void:
	var node: Node3D = _uploaded[index]
	_uploaded.erase(index)
	_fill_stale = true
	if node != null:
		world._terrain.remove_child(node)
		node.queue_free()


func _add_chunk(world: RideWorld, chunk: Dictionary, index: int) -> Node3D:
	var center: Vector3 = chunk["center"]
	var node: Node3D = Node3D.new()
	node.position = center
	world._terrain.add_child(node)
	var terrain_arrays: Dictionary = chunk["terrain"]
	var building_arrays: Dictionary = chunk["buildings"]
	# Ground and water draw no UVs and buildings' stay below a few hundred metres (centimetre
	# steps): their meshes are compressed. Streets', tracks' and sidewalks' run in metres along
	# whole ways, kilometres long, and keep full precision.
	var ground: MeshInstance3D = world._mesh_instance(terrain_arrays, world._terrain_material, true)
	world._ground_lods.make(ground, terrain_arrays)
	ground.visibility_range_end = RideWorld.VISIBILITY_RANGE * world._distance
	node.add_child(ground)
	if index >= 0:
		_add_stand_in(world, node, index)
	for surface: Array in [
		["streets", world._street_material, false],
		["tracks", world._track_material, false],
		["water", world._water_material, true]
	]:
		var arrays: Dictionary = chunk.get(surface[0], {})
		var vertices: PackedVector3Array = arrays.get("vertices", PackedVector3Array())
		if not vertices.is_empty():
			var material: ShaderMaterial = surface[1]
			var compressed: bool = surface[2]
			var way: MeshInstance3D = world._mesh_instance(arrays, material, compressed)
			way.visibility_range_end = RideWorld.VISIBILITY_RANGE * world._distance
			if surface[0] == "water":
				way.set_meta("chunk_water", true)
				follow_distance(way, world._distance)
			if material == world._track_material:
				way.visibility_range_end = RideWorld.TRACK_RANGE
				way.set_meta("fixed_range", true)
			node.add_child(way)
	UrbanSurfaces.add_to(node, chunk, RideWorld.SIDEWALK_RANGE)
	var buildings: MeshInstance3D = world._mesh_instance(
		building_arrays, world._building_material, true
	)
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
	return node
