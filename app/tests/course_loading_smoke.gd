extends SceneTree
## R223/R224: prepare the visible start area, then ride while distant chunks stream in; chunks
## left far behind leave the scene and come back with the rider (ADR 0040). Rides along a
## course's video (R17) have no scene to prepare.

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
	var course: Dictionary = await _make_course("Scene readiness")
	_unstarted_focus()
	# Reopen from the file so the first-start check cannot reuse the fixture's generated data.
	var path: String = course["path"]
	_orqwa.open_course(path)
	_start.call("_open_course", course)
	_check(_detail._loading_route, "course selection starts loading its route")
	await _switch_tabs("a loading course")
	await _wait_until(func() -> bool: return not _detail._loading_route, "course opened")
	await _switch_tabs("a loaded course")
	_check(_orqwa.world_chunk_count() == 0, "opening the course does not build its world")
	_world.set_process(false)
	_detail.call("_ride", false)
	await _wait_until(func() -> bool: return _detail._preparing_scene, "first scene preparation")
	await _still_loading("first start")
	await _switch_tabs("scene preparation")
	_check(not _start._pending_ride.is_empty(), "switching tabs preserves the pending ride")
	_check(_detail._continue_button.disabled, "continuation actions also wait for the scene")
	_detail.call("_ride", false)
	_check(_starts == 0, "a duplicate request cannot skip preparation")
	_world.set_process(true)
	await _wait_until(func() -> bool: return _starts == 1, "first ride starts")
	var uploaded: int = _world._streaming.uploaded()
	_check(uploaded < _world._chunk_count, "riding starts before distant chunks are uploaded")
	print("Ride started with %d / %d chunks uploaded" % [uploaded, _world._chunk_count])
	_check(not _world.waiting_for_world(), "the start's surroundings are built before riding")
	# The rest of the world is built while riding (ADR 0040); jumps below land where it is.
	await _wait_until(_orqwa.world_complete, "the rest of the world is built while riding")
	await _moving_priority()
	await _wait_until(_world.uploads_complete, "distant chunks finish while riding")
	var elapsed: float = _orqwa.ride_state().get("elapsed_s", 0.0)
	_check(elapsed > 0.0, "the ride clock runs while distant chunks load")
	_check(
		_world._streaming.uploaded() < _world._chunk_count, "a long course is never all uploaded"
	)
	_back_home()

	# Already uploaded scenes need no second preparation pass.
	_detail.call("_ride", false)
	_check(_starts == 2, "a prepared repeat ride starts immediately")
	_back_home()
	await _changed_options()
	await _cancelled_start(course)
	await _workout(course)
	await _along_video()
	_main.free()
	await process_frame
	if not _failed:
		print("COURSE LOADING SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _switch_tabs(what: String) -> void:
	var path: String = _detail._path
	var options: Dictionary = _detail.ride_options()
	var revision: int = _start._preparation_revision
	# Polling is paused so the loading case cannot finish before the clicks reach Courses.
	_orqwa.set_process(false)
	await process_frame
	for tab: int in [StartPage.Tab.DEVICES, StartPage.Tab.COURSES]:
		var press: InputEventMouseButton = InputEventMouseButton.new()
		press.button_index = MOUSE_BUTTON_LEFT
		press.position = (
			_start._tab_bar.get_global_transform() * _start._tab_bar.get_tab_rect(tab).get_center()
		)
		press.global_position = press.position
		press.pressed = true
		root.push_input(press, true)
		var release: InputEventMouseButton = press.duplicate()
		release.pressed = false
		root.push_input(release, true)
	_check(_detail.is_visible_in_tree(), what + ": returning shows the selected course")
	_check(_detail._path == path, what + ": the selected course is unchanged")
	_check(_detail.ride_options() == options, what + ": ride options are kept")
	_check(_start._preparation_revision == revision, what + ": preparation is not cancelled")
	_orqwa.set_process(true)


func _unstarted_focus() -> void:
	_world.set_process(false)
	WorldPreparation.restart(_world)
	var focus: Vector2 = Vector2(0.0, 9000.0)
	_world._streaming.prioritize(_world, focus)
	_world._build_some_chunks()
	var first: Node3D = _world.get_node("Terrain").get_child(0) as Node3D
	var centre: Vector2 = Vector2(first.position.x, -first.position.z)
	_check(centre.distance_to(focus) < 1000.0, "the requested entry point is loaded before a ride")
	_check(_orqwa.ride_state().is_empty(), "entry-point preparation does not start the ride clock")
	_world.set_process(true)


func _moving_priority() -> void:
	var terrain: Node3D = _world.get_node("Terrain")
	var start: Vector2 = _rider()
	var before: Array[Node] = terrain.get_children()
	_world.set_process(false)
	_check(_orqwa.jump_to_distance(9000.0), "move the fake rider to a distant part of the course")
	var far: Vector2 = _rider()
	_world.set_process(true)
	await _wait_until(
		func() -> bool: return _first_added(terrain, before) != null, "uploads follow the rider"
	)
	_check(
		_centre(_first_added(terrain, before)).distance_to(far) < 1000.0,
		"the next uploaded chunk is near the new position, rather than continuing the old queue"
	)
	await _wait_until(_world.chunks_uploaded, "the new surroundings are uploaded")
	# Let go beyond the upload radius, the distance moved before the next plan and a chunk.
	# 480 m is the core's CHUNK_SIZE. The ride goes on while uploading, so the focus may have moved.
	var reach: float = _world._streaming._radius + _world._streaming._replan + 480.0
	var focus: Vector2 = _world._streaming._focus
	var within: bool = true
	var behind: bool = false
	for chunk: Node in terrain.get_children():
		within = within and _centre(chunk).distance_to(focus) <= reach
		behind = behind or _centre(chunk).distance_to(start) < 1000.0
	_check(within, "no chunk beyond reach of the rider stays in the scene")
	_check(not behind, "the start's chunks, 9 km behind, left the scene")
	# Back through the elevation profile, as a rider would: the clock waits for the scene.
	var ride_screen: RideScreen = _main.get_node("RideScreen")
	ride_screen._profile.jump_requested.emit(0.0)
	_check(_orqwa.is_paused(), "a jump into an unloaded place pauses the ride")
	# Pause pressed while waiting keeps the ride paused, as the rider's own pause.
	ride_screen._toggle_pause()
	_check(_orqwa.is_paused(), "pause during the wait keeps the ride paused")
	await _wait_until(_world.surroundings_ready, "the start's surroundings come back")
	await process_frame
	_check(_orqwa.is_paused(), "the rider's own pause outlasts the wait")
	ride_screen._toggle_pause()
	_check(not _orqwa.is_paused(), "the rider resumes")
	# Without the rider's pause, the ride goes on by itself once the scene is there.
	ride_screen._profile.jump_requested.emit(9000.0)
	_check(_orqwa.is_paused(), "a jump back to the far place waits again")
	await _wait_until(func() -> bool: return not _orqwa.is_paused(), "the ride goes on by itself")
	_check(_world._streaming.nearby_ready(), "the far place's surroundings came first")
	_check(_orqwa.jump_to_distance(0.0), "return to the starting area")


func _rider() -> Vector2:
	var state: Dictionary = _orqwa.ride_state()
	var east: float = state["rider_x"]
	var north: float = state["rider_y"]
	return Vector2(east, north)


func _first_added(terrain: Node3D, before: Array[Node]) -> Node:
	for chunk: Node in terrain.get_children():
		if not before.has(chunk):
			return chunk
	return null


func _centre(chunk: Node) -> Vector2:
	var position: Vector3 = (chunk as Node3D).position
	return Vector2(position.x, -position.z)


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


## Chosen with "Along the video", the ride starts at once with the video instead of a world.
func _along_video() -> void:
	var course: Dictionary = await _make_course("Video readiness")
	var path: String = course["path"]
	_orqwa.open_course(path)
	await _wait_until(_orqwa.has_route, "video fixture reopened")
	_start.call("_open_course", course)
	await _wait_until(func() -> bool: return not _detail._loading_route, "video course opened")
	# A course never ridden in 3D keeps no world to fall back on.
	_check(_orqwa.world_chunk_count() == 0, "the video course has no world")
	var video: String = ProjectSettings.globalize_path("res://../core/fixtures/av1-8bit.mp4")
	var length: float = course["length_m"]
	var marks: PackedVector2Array = PackedVector2Array([Vector2(0.0, 0.0), Vector2(length, 2.0)])
	_check(_orqwa.add_video(video, marks), "the course gets a video")
	_world.set_process(false)
	_detail.call("_ride", true)
	await _wait_until(func() -> bool: return _starts == 6, "a ride along the video starts")
	_check(_orqwa.riding_along_video(), "the started ride plays the video")
	_check(not _world.visible, "the 3D world stays hidden behind the video")
	_back_home()
	_world.set_process(true)


func _still_loading(what: String) -> void:
	var history_size: int = _orqwa.history().size()
	await create_timer(0.1).timeout
	_check(_start.visible and not _world.visible, what + ": the loading page stays visible")
	_check(_orqwa.ride_state().is_empty(), what + ": the ride clock has not started")
	_check(_orqwa.history().size() == history_size, what + ": no recorded ride")
	_check(not _world._streaming.nearby_ready(), what + ": visible surroundings are incomplete")


func _on_started(options: Dictionary) -> void:
	_starts += 1
	if options.has("workout") and not options.get("on_course", false):
		return
	if _orqwa.riding_along_video():
		return
	_check(_world._streaming.nearby_ready(), "the first ride frame has its visible surroundings")
	var terrain: Node3D = _world.get_node("Terrain")
	_check(
		terrain.get_child_count() == _world._streaming.uploaded(),
		"every completed chunk is present when riding starts"
	)


func _back_home() -> void:
	_orqwa.abort_ride()
	_main.call("_back_home")


func _make_course(name: String) -> Dictionary:
	var path: String = OS.get_user_data_dir().path_join(name.to_lower().replace(" ", "-") + ".gpx")
	var file: FileAccess = FileAccess.open(path, FileAccess.WRITE)
	file.store_string(
		(
			'<gpx><trk><name>%s</name><trkseg><trkpt lat="46" lon="7">' % name
			+ '<ele>500</ele></trkpt><trkpt lat="46.108" lon="7"><ele>520</ele></trkpt>'
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
