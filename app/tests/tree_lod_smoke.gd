extends SceneTree
## R43/R108: distant geometry remains cheaper without losing trees or changing nearby models.

const TREES: Array[String] = [
	"conifer_tall",
	"conifer_broad",
	"broadleaf_round",
	"broadleaf_cluster",
	"palm_coconut",
	"palm_short",
	"palm_fan",
	"banana"
]

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	for name: String in TREES:
		_model(name)
	_batches()
	_rebatch()
	await _ground()
	if not _failed:
		print("TREE LOD SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _rebatch() -> void:
	var world: RideWorld = (load("res://scenes/world.tscn") as PackedScene).instantiate()
	root.add_child(world)
	world.apply_quality("ultra", {"tree_density": "low"})
	var terrain: Node3D = world.get_node("Terrain")
	var streaming: WorldStreaming = world.get("_streaming")
	var uploaded: Node3D = Node3D.new()
	terrain.add_child(uploaded)
	streaming._uploaded[0] = uploaded
	# Density stays 12.5%; switching to Low must still restore its cheaper batch budget.
	world.apply_quality("low")
	_check(
		terrain.get_child_count() == 0 and streaming.uploaded() == 0,
		"Low refreshes uploaded batches"
	)
	world.free()


func _model(name: String) -> void:
	var original: ArrayMesh = _imported(VegetationModels.DIRECTORY + name + ".glb")
	var combined: ArrayMesh = VegetationModels.mesh(name) as ArrayMesh
	_check(combined != null, name + " loads with its LODs")
	_check(combined.get_surface_count() == original.get_surface_count(), "near materials retained")
	for surface: int in original.get_surface_count():
		var before: Array = original.surface_get_arrays(surface)
		var after: Array = combined.surface_get_arrays(surface)
		var old_vertices: PackedVector3Array = before[Mesh.ARRAY_VERTEX]
		var old_indices: PackedInt32Array = before[Mesh.ARRAY_INDEX]
		var new_vertices: PackedVector3Array = after[Mesh.ARRAY_VERTEX]
		var new_indices: PackedInt32Array = after[Mesh.ARRAY_INDEX]
		_check(old_indices.size() == new_indices.size(), name + " keeps near triangles")
		_lod_indices(combined, surface, new_vertices.size())
		for index: int in old_indices.size():
			if not old_vertices[old_indices[index]].is_equal_approx(
				new_vertices[new_indices[index]]
			):
				_check(false, name + " changes nearby geometry")
				break
	var previous_count: int = _triangles(original)
	for level: String in ["medium", "far"]:
		var mesh: ArrayMesh = _imported(
			VegetationModels.DIRECTORY + "lods/%s_%s.glb" % [name, level]
		)
		var count: int = _triangles(mesh)
		_check(count < previous_count, name + " simplifies at " + level)
		var height_ratio: float = mesh.get_aabb().size.y / original.get_aabb().size.y
		_check(height_ratio > 0.8 and height_ratio < 1.2, name + " keeps its height")
		previous_count = count


func _lod_indices(mesh: ArrayMesh, surface_index: int, vertex_count: int) -> void:
	var surface: Dictionary = RenderingServer.mesh_get_surface(mesh.get_rid(), surface_index)
	var lods: Array = surface["lods"]
	_check(lods.size() == 2, "both authored LODs reach the renderer")
	var base_data: PackedByteArray = surface["index_data"]
	var base_count: int = surface["index_count"]
	var index_bytes: int = base_data.size() / base_count
	var previous_count: int = base_count
	var previous_error: float = 0.0
	for lod: Dictionary in lods:
		var error: float = lod["edge_length"]
		var data: PackedByteArray = lod["index_data"]
		_check(error > previous_error, "detail steps follow projected size")
		_check(data.size() % (3 * index_bytes) == 0, "LOD indices form triangles")
		_check(data.size() / index_bytes <= previous_count, "LOD geometry never increases")
		for offset: int in range(0, data.size(), index_bytes):
			var index: int = (
				data.decode_u16(offset) if index_bytes == 2 else data.decode_u32(offset)
			)
			_check(index < vertex_count, "LOD indices address the combined vertex buffer")
		previous_count = data.size() / index_bytes
		previous_error = error


func _batches() -> void:
	var buffer: PackedFloat32Array = PackedFloat32Array()
	var expected: Array[PackedFloat32Array] = []
	for point: Vector3 in [Vector3(-2, 7, 3), Vector3(170, 9, -4), Vector3(350, 12, 310)]:
		var record: PackedFloat32Array = [
			1, 0, 0, point.x, 0, 1, 0, point.y, 0, 0, 1, point.z, 0.2, 0.4, 0.6, 1
		]
		expected.append(record)
		buffer.append_array(record)
	for cell_m: float in [160.0, 480.0]:
		var parent: Node3D = Node3D.new()
		VegetationModels.add_to(parent, "conifer_tall", buffer, 1200, 600, cell_m)
		var found: Array[PackedFloat32Array] = []
		for child: Node in parent.get_children():
			var placed: MultiMeshInstance3D = child as MultiMeshInstance3D
			_check(placed.has_meta("tree"), "batched trees retain their range metadata")
			_check(placed.visibility_range_end == 1200, "tree range remains independent of LOD")
			# The dummy renderer retains bulk buffers but not per-instance GPU readbacks.
			var uploaded: PackedFloat32Array = placed.multimesh.buffer
			_check(uploaded.size() == placed.multimesh.instance_count * 16, "complete records")
			for index: int in placed.multimesh.instance_count:
				found.append(uploaded.slice(index * 16, (index + 1) * 16))
		_check(found.size() == expected.size(), "batching retains every tree")
		for record: PackedFloat32Array in expected:
			_check(found.has(record), "placement and colour retained across batch boundaries")
		if cell_m == 480:
			_check(parent.get_child_count() == 1, "Low keeps its original draw-call budget")
		parent.free()


func _triangles(mesh: ArrayMesh) -> int:
	var count: int = 0
	for surface: int in mesh.get_surface_count():
		count += mesh.surface_get_array_index_len(surface) / 3
	return count


func _imported(path: String) -> ArrayMesh:
	var scene: PackedScene = load(path)
	var node: Node = scene.instantiate()
	var mesh: ArrayMesh = VegetationModels._find_mesh(node).mesh as ArrayMesh
	node.free()
	return mesh


## A chunk's ground gets coarser versions for the distance, made on a worker thread: the full
## ground shows meanwhile and stays as it was, and every version keeps the ground's edges, where
## neighbouring chunks meet it.
func _ground() -> void:
	var size: int = 41
	var vertices: PackedVector3Array = PackedVector3Array()
	var normals: PackedVector3Array = PackedVector3Array()
	var uvs: PackedVector2Array = PackedVector2Array()
	var colors: PackedColorArray = PackedColorArray()
	var edge: Array[int] = []
	for row: int in size:
		for column: int in size:
			vertices.append(Vector3(column * 3.0, row * 0.4, -row * 3.0))
			normals.append(Vector3(0.0, 1.0, 0.13).normalized())
			uvs.append(Vector2.ZERO)
			colors.append(Color(0.3, 0.6, 0.2))
			if row == 0 or column == 0 or row == size - 1 or column == size - 1:
				edge.append(row * size + column)
	var indices: PackedInt32Array = PackedInt32Array()
	for row: int in size - 1:
		for column: int in size - 1:
			var corner: int = row * size + column
			indices.append_array(
				[corner, corner + 1, corner + size, corner + 1, corner + size + 1, corner + size]
			)
	var arrays: Dictionary = {
		"vertices": vertices, "normals": normals, "uvs": uvs, "colors": colors, "indices": indices
	}
	var ground: MeshInstance3D = MeshInstance3D.new()
	ground.mesh = WorldMeshes.from_arrays(arrays)
	root.add_child(ground)
	var full: Mesh = ground.mesh
	var lods: GroundLods = GroundLods.new()
	lods.make(ground, arrays)
	_check(lods.pending() and ground.mesh == full, "the full ground shows until its versions")
	var until: int = Time.get_ticks_msec() + 10000
	while lods.pending() and Time.get_ticks_msec() < until:
		await process_frame
		lods.add_ready()
	var mesh: ArrayMesh = ground.mesh as ArrayMesh
	_check(not lods.pending() and mesh != full, "its versions replace the full ground")
	_check(
		(
			mesh.surface_get_array_len(0) == vertices.size()
			and mesh.surface_get_array_index_len(0) == indices.size()
		),
		"the full detail stays as it was"
	)
	var versions: Array = RenderingServer.mesh_get_surface(mesh.get_rid(), 0)["lods"]
	_check(not versions.is_empty(), "coarser versions of the ground reach the renderer")
	for version: Dictionary in versions:
		var data: PackedByteArray = version["index_data"]
		var used: Dictionary[int, bool] = {}
		for offset: int in range(0, data.size(), 2):
			used[data.decode_u16(offset)] = true
		_check(data.size() / 2 < indices.size(), "a version draws fewer triangles")
		var whole: bool = edge.all(func(vertex: int) -> bool: return used.has(vertex))
		_check(whole, "a version keeps every vertex of the edges, where chunks meet")
	lods.finish()
	ground.free()


func _check(ok: bool, message: String) -> void:
	if not ok:
		_failed = true
		push_error(message)
