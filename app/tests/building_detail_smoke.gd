extends SceneTree
## Ultra is an upload/presentation choice: lighter presets and kept worlds retain their data.

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var benchmark: OrqwaGraphicsBenchmark = OrqwaGraphicsBenchmark.new()
	var ordinary: Dictionary = benchmark.chunk(1.0)
	var detailed: Dictionary = benchmark.chunk_with_building_detail(1.0, true)
	_check(not ordinary.has("building_details"), "ordinary upload allocates no façade details")
	_check(detailed.has("building_details"), "Ultra upload derives façade details")
	var saved_buildings: Dictionary = ordinary["buildings"]
	var uploaded_buildings: Dictionary = detailed["buildings"]
	var saved_models: Array = ordinary["modelled"]
	var uploaded_models: Array = detailed["modelled"]
	_check(saved_buildings == uploaded_buildings, "saved shell data stays unchanged")
	_check(saved_models == uploaded_models, "saved model names and buffers stay unchanged")
	var world: BenchmarkWorld = (
		(load("res://scenes/benchmark_world.tscn") as PackedScene).instantiate()
	)
	root.add_child(world)
	var terrain: Node3D = world.get_node("Terrain")
	for preset: String in ["low", "medium", "high", "ultra"]:
		world.apply_quality(preset, {"view_distance": "ultra", "tree_density": "low"})
		world.show_benchmark(benchmark)
		world.call("_build_some_chunks")
		var detail_count: int = _details(terrain)
		_check(
			detail_count > 0 if preset == "ultra" else detail_count == 0,
			"only Ultra uploads additional geometry, independently of View distance"
		)
	world.apply_quality("ultra", {"view_distance": "low", "tree_density": "low"})
	_check(_details(terrain) > 0, "Ultra retains its close detail under a shorter view range")
	world.apply_quality("ultra", {"view_distance": "ultra", "tree_density": "low"})
	_check(_details(terrain) > 0, "increasing View distance cannot extend close detail")
	world.apply_quality("medium", {"tree_density": "low"})
	_check(terrain.get_child_count() == 0, "downgrading refreshes already uploaded Ultra chunks")
	var streaming: WorldStreaming = world.get("_streaming")
	_check(streaming.uploaded() == 0, "the same kept course can upload with its new detail budget")
	world.free()
	_crossfades(uploaded_models)
	_models()
	_streaming()
	if not _failed:
		print("BUILDING DETAIL SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _details(node: Node) -> int:
	var count: int = 0
	if node.has_meta("building_detail"):
		count = 1
		var instance: GeometryInstance3D = node as GeometryInstance3D
		_check(
			instance.visibility_range_end <= BuildingDetails.RANGE,
			"extended View distance cannot extend close building details"
		)
	for child: Node in node.get_children():
		count += _details(child)
	return count


## A building is never see-through between its appearances: whichever appearance takes over is
## opaque, with no fade of its own, before the one it replaces starts dithering away.
func _crossfades(cells: Array) -> void:
	var model_range: float = 600.0
	var checked: int = 0
	for cell: Dictionary in cells:
		var parent: Node3D = Node3D.new()
		BuildingModels.add_to(parent, cell, StandardMaterial3D.new(), model_range, 1800.0, true)
		for child: Node in parent.get_children():
			var geometry: GeometryInstance3D = child as GeometryInstance3D
			if geometry.has_meta("building_detail") or geometry.visibility_range_begin <= 0.0:
				continue
			checked += 1
			_check(
				is_zero_approx(geometry.visibility_range_begin_margin),
				"an appearance taking over does not fade in"
			)
			var replaced: float = (
				BuildingDetails.RANGE - BuildingDetails.FADE
				if geometry is MultiMeshInstance3D
				else model_range - BuildingModels.MODEL_FADE
			)
			_check(
				geometry.visibility_range_begin <= replaced + 0.001,
				"an appearance taking over is drawn before the nearer one fades"
			)
		parent.free()
	_check(checked > 0, "the calibration scene has buildings changing appearance")


func _models() -> void:
	var manifest: Dictionary = JSON.parse_string(
		FileAccess.get_file_as_string(BuildingModels.DIRECTORY + "models.json")
	)
	var models: Dictionary = manifest["models"]
	for name: String in models:
		_check(
			ResourceLoader.exists(BuildingModels.DIRECTORY + "detail/" + name + ".glb"),
			"every saved model name has its Ultra appearance: " + name
		)
	var house: ArrayMesh = BuildingModels.mesh("house_gable_2_m")
	var close: ArrayMesh = BuildingModels.mesh("house_gable_2_m", true)
	_check(house != close, "Ultra selects an independent variant")
	_check(_triangles(close) > _triangles(house), "the close house contains extra geometry")
	var glass: bool = false
	var looks: Array[String] = []
	var room_seeds: Dictionary = {}
	for surface: int in close.get_surface_count():
		var material: ShaderMaterial = close.surface_get_material(surface)
		var pattern: int = material.get_shader_parameter("pattern")
		looks.append("%s:%d" % [material.resource_name, pattern])
		if pattern == 21:
			glass = true
			var arrays: Array = close.surface_get_arrays(surface)
			var coordinates: PackedVector2Array = arrays[Mesh.ARRAY_TEX_UV2]
			_check(not coordinates.is_empty(), "Ultra windows retain their stable room UV channel")
			for coordinate: Vector2 in coordinates:
				room_seeds[coordinate.x] = true
	_check(glass, "simulated window interiors belong to the Ultra model: " + ", ".join(looks))
	_check(room_seeds.size() > 1, "different windows can show different interiors")
	for surface: int in house.get_surface_count():
		var material: ShaderMaterial = house.surface_get_material(surface)
		var pattern: int = material.get_shader_parameter("pattern")
		_check(pattern != 21, "ordinary glass stays inexpensive")
	var plaster: ShaderMaterial = BuildingModels.material("plaster")
	var mapped: bool = plaster.get_shader_parameter("roughness_mapped")
	_check(mapped, "shared scanned plaster has its physical roughness map")


func _triangles(mesh: ArrayMesh) -> int:
	var count: int = 0
	for surface: int in mesh.get_surface_count():
		count += mesh.surface_get_array_index_len(surface) / 3
	return count


func _streaming() -> void:
	var streaming: BuildingDetailStreaming = BuildingDetailStreaming.new()
	var nearby: Node3D = Node3D.new()
	var distant: Node3D = Node3D.new()
	distant.position.x = 2000.0
	var bounds: AABB = AABB(Vector3(-20, 0, -20), Vector3(40, 20, 40))
	streaming.register(0, nearby, bounds)
	streaming.register(1, distant, bounds)
	var loaded: Array[int] = []
	var known: Array[PackedInt32Array] = []
	var let_go: Array[int] = []
	var empty: Dictionary = {
		"vertices": PackedVector3Array(),
		"normals": PackedVector3Array(),
		"uvs": PackedVector2Array(),
		"colors": PackedColorArray(),
		"indices": PackedInt32Array()
	}
	# The core's answer: the first refresh of a chunk adds cells 0 and 1, the next drops 0 and
	# adds 2.
	var loader: Callable = func(
		index: int, _x: float, _z: float, _radius: float, cells: PackedInt32Array
	) -> Dictionary:
		loaded.append(index)
		known.append(cells)
		if cells.is_empty():
			return {"added": {Vector2i(0, 0): empty, Vector2i(1, 0): empty}, "dropped": []}
		return {"added": {Vector2i(2, 0): empty}, "dropped": [Vector2i(0, 0)]}
	var releaser: Callable = func(index: int) -> void: let_go.append(index)
	streaming.update(Vector2.ZERO, loader, releaser)
	_check(loaded == [0], "distant course chunks never allocate façade additions")
	_check(nearby.get_child_count() == 2, "nearby cells are uploaded one node each")
	_check(streaming.ready_at(Vector2.ZERO), "nearby additions are ready before riding")
	var kept: Node = nearby.get_child(1)
	streaming.update(Vector2(1, 0), loader, releaser)
	_check(loaded.size() == 1, "the movement margin avoids rebuilding every frame")
	streaming.update(Vector2(70, 0), loader, releaser)
	_check(loaded.size() == 2, "moving beyond the margin refreshes the nearby subset")
	_check(known[1].size() == 4, "a refresh tells the core which cells are uploaded already")
	_check(is_instance_valid(kept) and not kept.is_queued_for_deletion(), "cells in reach stay")
	_check(nearby.get_child(0).is_queued_for_deletion(), "cells left behind are let go of")
	streaming.update(Vector2(2000, 0), loader, releaser)
	_check(loaded == [0, 0, 1], "moving or jumping streams the new surroundings")
	_check(let_go == [0], "the core forgets the façades of chunks left behind")
	var remaining: int = 0
	for child: Node in nearby.get_children():
		if not child.is_queued_for_deletion():
			remaining += 1
	_check(remaining == 0, "far additions are released")
	streaming.unregister(1)
	_check(let_go == [0, 1], "chunks the world lets go of release their façades")
	streaming.reset()
	nearby.free()
	distant.free()


func _check(ok: bool, message: String) -> void:
	if not ok:
		_failed = true
		push_error(message)
