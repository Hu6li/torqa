class_name ContinuationScreenshots
extends RefCounted
## Real continuation controls with illustrative progress, using the bundled course (R220).


static func capture(main: Control, out: String) -> void:
	var tree: SceneTree = main.get_tree()
	var filters_only: bool = OS.get_environment("SCREENSHOT_CONTINUATION_FILTER_ONLY") == "1"
	var app: OrqwaApp = main.get_node("Orqwa")
	var start: StartPage = main.get_node("StartPage")
	var course: Dictionary = app.courses()[0]
	start._open_course(course)
	while not app.has_route():
		await tree.process_frame
	var length: float = course["length_m"]
	var distance: float = length * 0.63
	var point: Dictionary = {
		"distance_m": distance, "remaining_m": length - distance, "length_m": length
	}
	var page: CourseDetail = start.get("_detail")
	var courses: CoursesTab = start.get("_courses")
	var gallery: VBoxContainer = courses.get("_gallery")
	course["in_progress"] = true
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		start._show_gallery()
		for child: Node in gallery.get_children():
			gallery.remove_child(child)
			child.queue_free()
		courses.call("_add_group", {"group": "", "label": "", "courses": [course]})
		await tree.create_timer(0.4).timeout
		_save(main, out, "courses-" + locale)
		var filters: Button = courses.get("_filters")
		var filter_panel: PopupPanel = courses.get("_filter_panel")
		filters.pressed.emit()
		await tree.create_timer(0.3).timeout
		_save(main, out, "courses-filters-" + locale)
		filter_panel.hide()
		if filters_only:
			continue
		start._open_course(course)
		page.call("_show_continuation_point", point)
		await tree.create_timer(0.4).timeout
		_save(main, out, "course-detail-" + locale)
	TranslationServer.set_locale("en")
	if filters_only:
		return
	app.set_graphics_quality("low")
	# A short offline course keeps repeated dialog reviews quick.
	app.load_route(
		ProjectSettings.globalize_path("res://../core/fixtures/london-cycleway.gpx"), true
	)
	await app.world_ready
	var world: RideWorld = main.get_node("World")
	while true:
		var next: int = world.get("_next_chunk")
		var count: int = world.get("_chunk_count")
		if next >= count:
			break
		await tree.process_frame
	app.connect_fake_trainer(250.0, 90.0)
	app.start_ride(50.0, false, {"kind": "none"})
	start.ride_started.emit(start.ride_options())
	app.jump_to_distance(700.0)
	app.set_paused(true)
	var screen: RideScreen = main.get_node("RideScreen")
	var dialog: ConfirmationDialog = screen.get("_continuation_dialog")
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		UiScale.popup(dialog, Vector2i(600, 150))
		await tree.create_timer(0.6).timeout
		_save(main, out, "save-point-" + locale)
		dialog.hide()
	app.abort_ride()


static func _save(main: Control, out: String, name: String) -> void:
	main.get_viewport().get_texture().get_image().save_png(out.path_join(name + ".png"))
	print("saved continuation ", name)
