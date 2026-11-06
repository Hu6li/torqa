extends SceneTree
## R220: no ride, clock or recording before the complete course scene is prepared.

var _failed: bool = false
var _main: Control
var _orqwa: OrqwaApp
var _start: StartPage
var _world: RideWorld
var _detail: CourseDetail
var _starts: int = 0


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_main = (load("res://scenes/main.tscn") as PackedScene).instantiate() as Control
	root.add_child(_main)
	await process_frame
	_orqwa = _main.get_node("Orqwa")
	_start = _main.get_node("StartPage")
	_world = _main.get_node("World")
	_detail = _start.get("_detail")
	_start.ride_started.connect(_on_started)
	_orqwa.set_graphics_quality("low")
	_check(_orqwa.connect_fake_trainer(250.0, 85.0), "fake trainer connected")
	var course: Dictionary = await _make_course()
	# Reopen from the file so the first-start check cannot reuse the fixture's generated data.
	var path: String = course["path"]
	_orqwa.open_course(path)
	await _wait_until(_orqwa.has_route, "fixture world released")
	_start.call("_open_course", course)
	await _wait_until(func() -> bool: return not _detail._loading_route, "course opened")
	_check(_orqwa.world_chunk_count() == 0, "opening the course does not build its world")
	_world.set_process(false)
	_detail.call("_ride", false)
	await _wait_until(func() -> bool: return _detail._preparing_scene, "first scene preparation")
	await _still_loading("first start")
	_detail.call("_ride", false)
	_check(_starts == 0, "a duplicate request cannot skip preparation")
	_world.set_process(true)
	await _wait_until(func() -> bool: return _starts == 1, "first ride starts")
	_back_home()

	# Already uploaded scenes need no second preparation pass.
	_detail.call("_ride", false)
	_check(_starts == 2, "a prepared repeat ride starts immediately")
	_back_home()
	await _changed_options()
	await _cancelled_start(course)
	await _workout(course)
	_main.free()
	await process_frame
	if not _failed:
		print("COURSE LOADING SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _changed_options() -> void:
	_world.set_process(false)
	_detail._options.set_options({"season": "Winter"})
	_detail.call("_ride", false)
	await _still_loading("a changed season")
	# The rider can change graphics from Settings even while preparation is running.
	_orqwa.set_graphics_option("tree_density", "ultra")
	await process_frame
	await _still_loading("changed graphics during preparation")
	_world.set_process(true)
	await _wait_until(func() -> bool: return _starts == 3, "winter ride starts")
	var seasons: SeasonMaterials = _world.get("_seasons")
	_check(seasons.season == "Winter", "the selected season is ready before starting")
	var quality: Dictionary = _world.get("_quality")
	var density: float = quality["tree_density"]
	_check(density == 1.0, "the latest graphics are ready before starting")
	_back_home()


func _cancelled_start(course: Dictionary) -> void:
	_world.set_process(false)
	_detail._options.set_options({"season": "Autumn"})
	_detail.call("_ride", false)
	await _still_loading("before returning to the gallery")
	_check(_start.go_back(), "back returns to the gallery")
	await process_frame
	_world.set_process(true)
	await _wait_until(_world.uploads_complete, "abandoned chunks finish")
	_check(_starts == 3 and _orqwa.ride_state().is_empty(), "Back abandons the pending start")
	_check(not _detail._loading_bar.visible, "the abandoned loading page is cleared")
	_start.call("_open_course", course)
	await _wait_until(func() -> bool: return not _detail._loading_route, "course reopened")


func _workout(course: Dictionary) -> void:
	var workouts: WorkoutsTab = _start.get("_workouts")
	workouts.refresh()
	var paths: PackedStringArray = workouts.get("_course_paths")
	var path: String = course["path"]
	workouts._where.select(paths.find(path) + 1)
	_detail._options.set_options({"season": "Summer"})
	_world.set_process(false)
	workouts.call("_on_start_pressed")
	await _wait_until(func() -> bool: return workouts._preparing_scene, "workout scene preparation")
	_check(workouts._start_button.disabled, "workout Start stays disabled while preparing")
	await _still_loading("a course workout")
	_world.set_process(true)
	await _wait_until(func() -> bool: return _starts == 4, "course workout starts")
	_back_home()
	# A HUD-only workout has no 3D scene to wait for.
	workouts._where.select(0)
	_world.set_process(false)
	workouts.call("_on_start_pressed")
	_check(_starts == 5, "HUD-only workouts start without a scene wait")
	_back_home()


func _still_loading(what: String) -> void:
	var history_size: int = _orqwa.history().size()
	await create_timer(0.1).timeout
	_check(_start.visible and not _world.visible, what + ": the loading page stays visible")
	_check(_orqwa.ride_state().is_empty(), what + ": the ride clock has not started")
	_check(_orqwa.history().size() == history_size, what + ": no recorded ride")
	_check(not _world.uploads_complete(), what + ": the scene is still incomplete")


func _on_started(options: Dictionary) -> void:
	_starts += 1
	if options.has("workout") and not options.get("on_course", false):
		return
	_check(_world.uploads_complete(), "the first visible ride frame has every scene mesh")
	var terrain: Node3D = _world.get_node("Terrain")
	_check(
		terrain.get_child_count() == _orqwa.world_chunk_count(),
		"the complete course terrain is present when riding starts"
	)


func _back_home() -> void:
	_orqwa.abort_ride()
	_main.call("_back_home")


func _make_course() -> Dictionary:
	var path: String = OS.get_user_data_dir().path_join("scene-readiness.gpx")
	var file: FileAccess = FileAccess.open(path, FileAccess.WRITE)
	file.store_string(
		(
			'<gpx><trk><name>Scene readiness</name><trkseg><trkpt lat="46" lon="7">'
			+ '<ele>500</ele></trkpt><trkpt lat="46.009" lon="7"><ele>520</ele></trkpt>'
			+ "</trkseg></trk></gpx>"
		)
	)
	file.close()
	var added: Array[String] = []
	_orqwa.course_added.connect(func(saved: String) -> void: added.append(saved), CONNECT_ONE_SHOT)
	_orqwa.load_route(path, true)
	await _wait_until(func() -> bool: return not added.is_empty(), "fixture course saved")
	for course: Dictionary in _orqwa.courses():
		if course["path"] == added[0]:
			return course
	_check(false, "fixture course is in the library")
	return {}


func _wait_until(condition: Callable, what: String) -> void:
	var until: int = Time.get_ticks_msec() + 30000
	while not condition.call() and Time.get_ticks_msec() < until:
		await process_frame
	var satisfied: bool = condition.call()
	_check(satisfied, what)


func _check(ok: bool, what: String) -> void:
	if not ok:
		_failed = true
		push_error("COURSE LOADING SMOKE TEST FAILED: " + what)
