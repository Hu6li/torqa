class_name SegmentScreenshots
extends RefCounted
## The rider's own segments (R302) as riders see them: marked on the bundled course's page, then
## ridden twice on a short offline course with the fake trainer, at a steady pace and then
## faster, so the HUD, the map, the moments and the times in the summary all show.

## Where the sprint on the London fixture lies, in metres from its start.
const SPRINT_START_M: float = 150.0
const SPRINT_END_M: float = 550.0
## The longest wait for the rider to reach a place, in seconds of real time.
const RIDE_TIMEOUT_S: float = 240.0


static func capture(main: Control, out: String) -> void:
	await _course_page(main, out)
	await _rides(main, out)


## The standard run's `course-detail.png`: the course just prepared with one segment marked,
## removed again so the rides after it show none.
static func course_detail(main: Control, page: CourseDetail, out: String) -> void:
	var app: OrqwaApp = main.get_node("Orqwa")
	if not app.save_segment("", "Wabern sprint", 400.0, 1000.0):
		push_error("SCREENSHOTS FAILED: a segment marked")
	page.refresh_records()
	await main.get_tree().create_timer(0.5).timeout
	main.get_viewport().get_texture().get_image().save_png(out.path_join("course-detail.png"))
	print("saved course detail")
	_forget_segments(app)
	page.refresh_records()


## The bundled course's page with two segments, in English, German and miles, then a segment
## being marked by clicks on the profile.
static func _course_page(main: Control, out: String) -> void:
	var tree: SceneTree = main.get_tree()
	var app: OrqwaApp = main.get_node("Orqwa")
	var start: StartPage = main.get_node("StartPage")
	var course: Dictionary = app.courses()[0]
	start._open_course(course)
	while not app.has_route():
		await tree.process_frame
	var length: float = course["length_m"]
	_forget_segments(app)
	app.save_segment("", "Twann sprint", 1200.0, 2400.0)
	app.save_segment("", "Chasseral finale", length - 2500.0, length)
	var page: CourseDetail = start.get("_detail")
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		page.refresh_records()
		await tree.create_timer(0.6).timeout
		_save(main, out, "course-" + locale)
	TranslationServer.set_locale("en")
	_set_units(app, "imperial")
	page.refresh_records()
	await tree.create_timer(0.6).timeout
	_save(main, out, "course-imperial")
	_set_units(app, "metric")
	page.refresh_records()
	var list: SegmentList = page.get("_segments")
	var actions: HBoxContainer = list.get("_actions")
	var add: Button = actions.get_child(0)
	add.pressed.emit()
	var field: LineEdit = list.get("_name")
	field.text = "Ligerz climb"
	field.text_changed.emit(field.text)
	# Dragged across the profile from 6.0 to 8.3 km and let go (R304): it stays, with grips.
	var profile: ElevationProfile = page.get("_profile")
	var points: PackedVector2Array = profile.get("_profile")
	var course_m: float = points[points.size() - 1].x
	var press: InputEventMouseButton = InputEventMouseButton.new()
	press.button_index = MOUSE_BUTTON_LEFT
	press.pressed = true
	press.position = Vector2(6000.0 / course_m * profile.size.x, profile.size.y / 2.0)
	profile.call("_gui_input", press)
	var drag: InputEventMouseMotion = InputEventMouseMotion.new()
	drag.position = Vector2(8300.0 / course_m * profile.size.x, press.position.y)
	profile.call("_gui_input", drag)
	press.pressed = false
	press.position = drag.position
	profile.call("_gui_input", press)
	Input.warp_mouse(profile.get_global_transform() * drag.position)
	await tree.create_timer(1.2).timeout
	_save(main, out, "course-marking")
	list.call("_close_form")
	_forget_segments(app)
	page.refresh_records()


## Two rides through a sprint: the first sets the best time, the second beats it.
static func _rides(main: Control, out: String) -> void:
	var tree: SceneTree = main.get_tree()
	var app: OrqwaApp = main.get_node("Orqwa")
	var start: StartPage = main.get_node("StartPage")
	app.set_graphics_quality("low")
	app.load_route(
		ProjectSettings.globalize_path("res://../core/fixtures/london-cycleway.gpx"), true
	)
	await app.world_ready
	var world: RideWorld = main.get_node("World")
	while not world.chunks_uploaded():
		await tree.process_frame
	_forget_segments(app)
	app.save_segment("", "Birdcage Walk sprint", SPRINT_START_M, SPRINT_END_M)
	for ride: int in [1, 2]:
		# The second ride goes harder, so it beats the first.
		app.connect_fake_trainer(240.0 if ride == 1 else 330.0, 90.0)
		await tree.create_timer(0.5).timeout
		app.start_ride(50.0, false, {"kind": "none"})
		start.ride_started.emit(start.ride_options())
		var name: String = "ride%d-" % ride
		if ride == 2:
			# The heads-up as the sprint comes (R303); the first ride opens with its greeting.
			await _heads_up(main)
			_save(main, out, name + "ahead")
		await _reach(app, SPRINT_START_M - 70.0)
		if ride == 1:
			_save(main, out, name + "approach")
			var screen: RideScreen = main.get_node("RideScreen")
			var minimap: Minimap = screen.get("_minimap")
			minimap.set("_follow", false)
			minimap.call("_redraw_map")
			await tree.create_timer(0.4).timeout
			_save(main, out, name + "approach-route")
			minimap.set("_follow", true)
			minimap.call("_redraw_map")
		await _reach(app, (SPRINT_START_M + SPRINT_END_M) / 2.0)
		_save(main, out, name + "on-segment")
		await _reach(app, SPRINT_END_M + 12.0)
		await tree.create_timer(0.4).timeout
		_save(main, out, name + "moment")
		app.finish_ride()
		await tree.create_timer(1.0).timeout
		var quick: RideSummaryDialog = main.get("_quick_summary")
		# Its segment times, as the card may have gone with the finish (R304).
		_save(main, out, name + "summary")
		quick.custom_action.emit(&"details")
		await tree.create_timer(1.0).timeout
		var summary: HistoryScreen = main.get_node("HistoryScreen")
		var times: Control = summary.get("_climbs")
		var detail: Control = summary.get("_detail")
		var scroll: ScrollContainer = detail.get_parent() as ScrollContainer
		scroll.ensure_control_visible(times)
		await tree.create_timer(0.6).timeout
		_save(main, out, name + "times")
		if ride == 2:
			_set_units(app, "imperial")
			var newest: Dictionary = app.history()[0]
			var newest_path: String = newest["path"]
			summary.open_summary(newest_path)
			await tree.create_timer(0.3).timeout
			scroll.ensure_control_visible(times)
			await tree.create_timer(0.6).timeout
			_save(main, out, name + "times-imperial")
			_set_units(app, "metric")
		summary.closed.emit()
		await tree.create_timer(0.5).timeout
	_forget_segments(app)


## Waits until the rider is `distance_m` along the route.
static func _reach(app: OrqwaApp, distance_m: float) -> void:
	var tree: SceneTree = app.get_tree()
	var until: int = Time.get_ticks_msec() + int(RIDE_TIMEOUT_S * 1000.0)
	while Time.get_ticks_msec() < until:
		var state: Dictionary = app.ride_state()
		var at: float = state.get("position_m", 0.0)
		if at >= distance_m:
			return
		await tree.process_frame
	push_error("SEGMENT SCREENSHOTS FAILED: the rider did not reach %.0f m" % distance_m)


## Waits until the card tells of the segment ahead and has slid in.
static func _heads_up(main: Control) -> void:
	var tree: SceneTree = main.get_tree()
	var screen: RideScreen = main.get_node("RideScreen")
	var card: MomentCard = screen.get("_moment_card")
	var until: int = Time.get_ticks_msec() + int(RIDE_TIMEOUT_S * 1000.0)
	while Time.get_ticks_msec() < until:
		var shown: PackedStringArray = card.current()
		if shown.size() == 2 and shown[1].contains("Birdcage Walk sprint"):
			await tree.create_timer(MomentCard.SLIDE_SECONDS + 0.2).timeout
			return
		await tree.process_frame
	push_error("SEGMENT SCREENSHOTS FAILED: no card told of the segment ahead")


static func _set_units(app: OrqwaApp, units: String) -> void:
	var profile: Dictionary = app.profile()
	var id: String = profile["id"]
	profile["units"] = units
	app.save_profile(id, profile)


static func _forget_segments(app: OrqwaApp) -> void:
	for segment: Dictionary in app.segments():
		var id: String = segment["id"]
		app.remove_segment(id)


static func _save(main: Control, out: String, name: String) -> void:
	main.get_viewport().get_texture().get_image().save_png(
		out.path_join("segment-" + name + ".png")
	)
	print("saved segment ", name)
