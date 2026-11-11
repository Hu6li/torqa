class_name GroundLods
extends RefCounted
## Coarser versions of the chunks' ground for the distance. In towns every street levels the
## ground across it, in pieces of a few metres that far off are smaller than a pixel; Godot draws
## these versions there instead (`WorldMeshes.simplified`). Making them takes seconds for a city,
## so they are made on worker threads: the ground shows in full until they replace it.

## Task id → the ground and a slot for its versions.
var _tasks: Dictionary[int, Array] = {}


## Starts making the coarser versions of `ground`, a compressed mesh of `arrays`
## (`WorldMeshes.from_arrays`).
func make(ground: MeshInstance3D, arrays: Dictionary) -> void:
	var made: Array[ImporterMesh] = [null]
	var task: int = WorkerThreadPool.add_task(
		func() -> void: made[0] = WorldMeshes.simplified(arrays, true), false, "Ground LODs"
	)
	_tasks[task] = [ground, made]


## Puts the versions made since the last call in place; the meshes are made here, on the main
## thread.
func add_ready() -> void:
	for task: int in _tasks.keys():
		if not WorkerThreadPool.is_task_completed(task):
			continue
		WorkerThreadPool.wait_for_task_completion(task)
		var entry: Array = _tasks[task]
		_tasks.erase(task)
		# Another course or quality may have replaced the chunk meanwhile.
		if not is_instance_valid(entry[0]):
			continue
		var ground: MeshInstance3D = entry[0]
		var made: Array[ImporterMesh] = entry[1]
		if made[0].get_surface_count() > 0:
			ground.mesh = made[0].get_mesh()


## Whether versions are still being made.
func pending() -> bool:
	return not _tasks.is_empty()


## Waits for the versions still being made and drops them: Godot reports tasks never waited for
## when it quits.
func finish() -> void:
	for task: int in _tasks.keys():
		WorkerThreadPool.wait_for_task_completion(task)
	_tasks.clear()
