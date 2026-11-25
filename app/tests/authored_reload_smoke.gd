extends SceneTree
## A second authored ride prepares its entry, and scene workers finish before their owner exits.

var _failed: bool = false
var _main: Control
var _orqwa: OrqwaApp
var _start: StartPage
var _world: RideWorld
var _detail: CourseDetail
var _added: String = ""
var _starts: int = 0
var _saved: String = ""
var _updates: bool = true


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	await _worker_exit()
	_main = current_scene as Control
	if _main == null:
		_main = (load("res://scenes/main.tscn") as PackedScene).instantiate() as Control
		root.add_child(_main)
	await process_frame
	_orqwa = _main.get_node("Orqwa")
	_start = _main.get_node("StartPage")
	_world = _main.get_node("World")
	_detail = _start.get("_detail")
	_start.ride_started.connect(func(_options: Dictionary) -> void: _starts += 1)
	_updates = _orqwa.checks_for_updates()
	_orqwa.set_checks_for_updates(false)
	_orqwa.set_graphics_quality("low")
	_orqwa.connect_fake_trainer(150.0, 85.0)
	_orqwa.course_added.connect(func(path: String) -> void: _added = path)
	_orqwa.ride_saved.connect(func(path: String) -> void: _saved = path)
	var fixture: String = OS.get_environment("RELOAD_FIXTURE")
	if fixture.is_empty():
		fixture = ProjectSettings.globalize_path("res://../core/fixtures/worlds/test-loop.oqx")
	_orqwa.import_course(fixture.simplify_path())
	await _wait(func() -> bool: return not _added.is_empty(), "fixture imported")
	var selected: Dictionary = {}
	for course: Dictionary in _orqwa.courses():
		if course["path"] == _added:
			selected = course
	_check(not selected.is_empty(), "fixture is in the library")
	if not selected.is_empty():
		_start.call("_open_course", selected)
		await _wait(func() -> bool: return not _detail._loading_route, "first course opened")
		_detail.call("_ride", false)
		await _wait(func() -> bool: return _starts == 1, "first authored ride starts")
		await create_timer(1.2).timeout
		_main.get_node("RideScreen").call("_finish_ride")
		await _wait(func() -> bool: return not _saved.is_empty(), "first ride saved")
		await process_frame
		_main.call("_back_home")
		# The old course can finish kilometres from the next course's entry.
		_world._camera.global_position = Vector3(15000.0, 800.0, 15000.0)
		var second: Dictionary = selected.duplicate(true)
		var first_path: String = selected["path"]
		second["path"] = fixture.simplify_path() + "#" + first_path.get_slice("#", 1)
		_start.call("_open_course", second)
		await _wait(func() -> bool: return not _detail._loading_route, "second course opened")
		_detail.call("_ride", false)
		await _wait(func() -> bool: return _starts == 2, "second authored ride starts")
		_check(_start._pending_ride.is_empty(), "second start leaves no loading coroutine")
		_orqwa.abort_ride()
		_main.call("_back_home")
		await _mixed_courses(selected)
		if not _failed:
			await _close_during_preparation(second)
			return
	_start.call("_cancel_preparation")
	await process_frame
	await process_frame
	if _failed:
		_world._authored.stop()
	_orqwa.set_checks_for_updates(_updates)
	_main.queue_free()
	await process_frame
	await process_frame
	if not _failed:
		print("AUTHORED RELOAD SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _worker_exit() -> void:
	var done: Array[bool] = [false]
	var owned: AuthoredWorld = AuthoredWorld.new()
	root.add_child(owned)
	var task: int = WorkerThreadPool.add_task(
		func() -> void:
			OS.delay_msec(80)
			done[0] = true
	)
	owned._parsing["pending.glb"] = task
	owned.free()
	_check(done[0], "leaving the tree drains the owner's pending scene task")
	# Baseline failures still clean up the test's own task before quitting.
	if not done[0]:
		WorkerThreadPool.wait_for_task_completion(task)
	await process_frame


func _close_during_preparation(course: Dictionary) -> void:
	_main.call("_back_home")
	_start.call("_open_course", course)
	await _wait(func() -> bool: return not _detail._loading_route, "closing course opened")
	_detail.call("_ride", false)
	await _wait(func() -> bool: return _detail._preparing_scene, "closing during preparation")
	var done: Array[bool] = [false]
	_world._authored._parsing["shutdown-check"] = WorkerThreadPool.add_task(
		_closing_task.bind(done)
	)
	_orqwa.set_checks_for_updates(_updates)
	_main.tree_exiting.connect(
		func() -> void:
			_check(done[0], "window close finishes the worker before tree teardown")
			_check(_world._authored._parsing.is_empty(), "window close releases worker tasks")
			_check(_start._pending_ride.is_empty(), "window close cancels scene preparation")
			if not _failed:
				print("AUTHORED RELOAD SMOKE TEST PASSED")
	)
	_main.notification(Node.NOTIFICATION_WM_CLOSE_REQUEST)
	var closing: bool = _main.get("_quitting")
	_check(closing, "window close is handled before automatic tree shutdown")


static func _closing_task(done: Array[bool]) -> void:
	OS.delay_msec(200)
	done[0] = true


func _mixed_courses(authored: Dictionary) -> void:
	var path: String = OS.get_user_data_dir().path_join("reload-check.gpx")
	var file: FileAccess = FileAccess.open(path, FileAccess.WRITE)
	file.store_string(
		(
			'<gpx><trk><name>Reload GPX</name><trkseg><trkpt lat="46" lon="7">'
			+ '<ele>500</ele></trkpt><trkpt lat="46.002" lon="7"><ele>510</ele></trkpt>'
			+ "</trkseg></trk></gpx>"
		)
	)
	file.close()
	_added = ""
	_orqwa.load_route(path, true)
	await _wait(func() -> bool: return not _added.is_empty(), "GPX course saved")
	var generated: Dictionary = {}
	for course: Dictionary in _orqwa.courses():
		if course["path"] == _added:
			generated = course
	_check(not generated.is_empty(), "GPX course is in the library")
	if generated.is_empty():
		return
	_start.call("_open_course", generated)
	await _wait(func() -> bool: return not _detail._loading_route, "GPX course opened")
	_detail.call("_ride", false)
	await _wait(func() -> bool: return _starts == 3, "authored to GPX ride starts")
	_check(not _world._authored.active(), "GPX ride releases the authored world")
	_orqwa.abort_ride()
	_main.call("_back_home")
	_world._camera.global_position = Vector3(-15000.0, 800.0, -15000.0)
	_start.call("_open_course", authored)
	await _wait(func() -> bool: return not _detail._loading_route, "authored course reopened")
	_detail.call("_ride", false)
	await _wait(func() -> bool: return _starts == 4, "GPX to authored ride starts")
	_check(_start._pending_ride.is_empty(), "mixed starts leave no loading coroutine")
	_orqwa.abort_ride()


func _wait(condition: Callable, message: String) -> void:
	var until: int = Time.get_ticks_msec() + 20000
	var frames: int = 0
	var inspected: bool = false
	while not condition.call() and Time.get_ticks_msec() < until:
		if (
			message == "second authored ride starts"
			and frames > 0
			and not inspected
			and _detail._preparing_scene
		):
			var entry: Vector2 = _orqwa.world_start_position(false)
			var planned: Vector3 = _world._authored._focus
			_check(
				Vector2(planned.x, -planned.z).distance_to(entry) < 20.0,
				"preparing the new entry never reloads the previous camera's location"
			)
			inspected = true
		frames += 1
		await process_frame
	var satisfied: bool = condition.call()
	_check(satisfied, message)


func _check(ok: bool, message: String) -> void:
	if not ok:
		_failed = true
		push_error(message)
