class_name WorldPreparation
extends RefCounted
## Scene uploads are a separate preparation stage from the core's world generation (R220).


## False means the course or its pending start was replaced while waiting.
static func prepare(
	world: RideWorld, orqwa: OrqwaApp, options: Dictionary, progress: Callable, requested: Callable
) -> bool:
	world.apply_quality(orqwa.graphics_quality(), orqwa.graphics_options())
	world.apply_options(options)
	world.reset_view()
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
		progress.call(world._next_chunk, world._chunk_count)
		if world.uploads_complete():
			return true
		await world.get_tree().process_frame
	return false


## Invalidates waiters before replacing the scene's terrain chunks.
static func restart(world: RideWorld) -> void:
	world._chunk_revision += 1
	for chunk: Node in world._terrain.get_children():
		world._terrain.remove_child(chunk)
		chunk.queue_free()
	world._next_chunk = 0
