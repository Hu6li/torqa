class_name WorldPreparation
extends RefCounted
## Scene uploads are a separate preparation stage from the core's world generation (R223).


## False means the course or its pending start was replaced while waiting.
static func prepare(
	world: RideWorld, orqwa: OrqwaApp, options: Dictionary, progress: Callable, requested: Callable
) -> bool:
	world.apply_quality(orqwa.graphics_quality(), orqwa.graphics_options())
	world.apply_options(options)
	world.reset_view()
	var continuing: bool = options.get("continue_course", false)
	var focus: Vector2 = orqwa.world_start_position(continuing)
	world._streaming.prioritize(world, focus)
	var revision: int = world._chunk_revision
	var course: String = orqwa.loaded_course()
	var quality: String = orqwa.graphics_quality()
	var graphics: Dictionary = orqwa.graphics_options()
	while true:
		if (
			not requested.call()
			or revision != world._chunk_revision
			or course != orqwa.loaded_course()
			or orqwa.world_chunk_count() == 0
		):
			return false
		# Graphics can be changed from Settings while the loading page is open.
		if quality != orqwa.graphics_quality() or graphics != orqwa.graphics_options():
			quality = orqwa.graphics_quality()
			graphics = orqwa.graphics_options()
			world.apply_quality(quality, graphics)
			revision = world._chunk_revision
			world._streaming.prioritize(world, focus)
		var nearby: Vector2i = world._streaming.nearby_progress()
		progress.call(nearby.x, nearby.y)
		if world._streaming.nearby_ready():
			return true
		await world.get_tree().process_frame
	return false


## Invalidates waiters before replacing the scene's terrain chunks.
static func restart(world: RideWorld) -> void:
	world._chunk_revision += 1
	world._streaming.reset()
	for chunk: Node in world._terrain.get_children():
		world._terrain.remove_child(chunk)
		chunk.queue_free()
