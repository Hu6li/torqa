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
	if world._authored.active():
		return await _prepare_authored(world, orqwa, focus, progress, requested)
	var revision: int = world._chunk_revision
	var course: String = orqwa.loaded_course()
	var quality: String = orqwa.graphics_quality()
	var graphics: Dictionary = orqwa.graphics_options()
	while true:
		if (
			not requested.call()
			or orqwa.world_failed()
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
		var detail_focus: Vector2 = Vector2(focus.x, -focus.y)
		world._building_detail_streaming.update(
			detail_focus, orqwa.world_building_details, orqwa.let_go_of_building_details
		)
		if (
			world._streaming.nearby_ready()
			and world._building_detail_streaming.ready_at(detail_focus)
		):
			return true
		await world.get_tree().process_frame
	return false


## A world file's world needs no building (R266): the sections around `focus` are loaded.
static func _prepare_authored(
	world: RideWorld, orqwa: OrqwaApp, focus: Vector2, progress: Callable, requested: Callable
) -> bool:
	var course: String = orqwa.loaded_course()
	while requested.call() and course == orqwa.loaded_course() and world._authored.active():
		world._authored.update(Vector3(focus.x, 0.0, -focus.y), world.view_distance())
		if world._authored.ready_around(focus):
			progress.call(1, 1)
			return true
		progress.call(0, 1)
		await world.get_tree().process_frame
	return false


## Invalidates waiters before replacing the scene's terrain chunks.
static func restart(world: RideWorld) -> void:
	world._chunk_revision += 1
	world._streaming.reset()
	world._building_detail_streaming.reset()
	for chunk: Node in world._terrain.get_children():
		world._terrain.remove_child(chunk)
		chunk.queue_free()
