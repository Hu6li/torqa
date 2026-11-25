class_name WorldLibraryScreenshots
extends RefCounted
## World-library controls and previews, using the imported test world without preparing scenery.


static func capture(main: Control, out: String) -> void:
	var tree: SceneTree = main.get_tree()
	var app: OrqwaApp = main.get_node("Orqwa")
	var start: StartPage = main.get_node("StartPage")
	var courses: CoursesTab = start.get("_courses")
	var imported: Array[bool] = [false]
	app.course_added.connect(func(_path: String) -> void: imported[0] = true, CONNECT_ONE_SHOT)
	app.import_course(ProjectSettings.globalize_path("res://../core/fixtures/worlds/test-loop.oqx"))
	while not imported[0]:
		await tree.process_frame
	var course: Dictionary = {}
	for entry: Dictionary in app.courses():
		var world: Dictionary = entry.get("world", {})
		if not world.is_empty():
			course = entry
			break
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		start._show_gallery()
		courses.clear_filters()
		var chips: GridContainer = courses.get("_chips")
		for chip: Node in chips.find_children("*", "Button", true, false):
			if (chip as Button).text == "World":
				(chip as Button).button_pressed = true
		var kinds: PackedStringArray = courses.query()["kinds"]
		assert(kinds == PackedStringArray(["world"]))
		await tree.create_timer(0.4).timeout
		_save(main, out, "world-courses-" + locale)
		var filters: Button = courses.get("_filters")
		filters.pressed.emit()
		await tree.create_timer(0.3).timeout
		_save(main, out, "world-filters-" + locale)
		var panel: PopupPanel = courses.get("_filter_panel")
		panel.hide()
		start._open_course(course)
		while not app.has_route():
			await tree.process_frame
		await tree.create_timer(0.4).timeout
		_save(main, out, "world-detail-" + locale)
	TranslationServer.set_locale("en")


static func _save(main: Control, out: String, name: String) -> void:
	main.get_viewport().get_texture().get_image().save_png(out.path_join(name + ".png"))
	print("saved ", name)
