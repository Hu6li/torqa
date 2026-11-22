extends SceneTree
## Map and world-cover corners must round at the image's bounds, whatever its aspect ratio.

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var viewport: SubViewport = SubViewport.new()
	viewport.size = Vector2i(240, 144)
	viewport.transparent_bg = true
	viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	root.add_child(viewport)
	var card: PathCard = PathCard.new()
	card.size = Vector2(viewport.size)
	viewport.add_child(card)
	for dimensions: Vector2i in [Vector2i(200, 120), Vector2i(100, 100), Vector2i(100, 200)]:
		var image: Image = Image.create(dimensions.x, dimensions.y, false, Image.FORMAT_RGBA8)
		image.fill(Color.WHITE)
		card.set_map(image.save_png_to_buffer())
		var fitted: Vector2 = Vector2(dimensions) * minf(240.0 / dimensions.x, 144.0 / dimensions.y)
		var origin: Vector2i = Vector2i((card.size - fitted) / 2.0)
		await _frame()
		var rendered: Image = viewport.get_texture().get_image()
		_check(rendered.get_pixelv(origin).a < 0.1, "the image's top-left corner is rounded")
		var bottom: Vector2i = origin + Vector2i(fitted) - Vector2i.ONE
		_check(rendered.get_pixelv(bottom).a < 0.1, "the image's bottom-right corner is rounded")
		_check(rendered.get_pixel(120, 72).a > 0.95, "the image stays visible")
	card.set_map(PackedByteArray())
	card.set_track(PackedVector2Array([Vector2.ZERO, Vector2(100.0, 100.0)]))
	await _frame()
	var fallback: Image = viewport.get_texture().get_image()
	_check(fallback.get_pixel(0, 0).a < 0.1, "the route-only card stays rounded")
	_check(fallback.get_pixel(120, 72).a > 0.95, "the route-only track stays visible")
	viewport.queue_free()
	await process_frame
	if not _failed:
		print("PATH CARD RENDER SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _frame() -> void:
	await process_frame
	await process_frame
	await RenderingServer.frame_post_draw


func _check(condition: bool, what: String) -> void:
	if not condition:
		_failed = true
		push_error("FAILED: " + what)
