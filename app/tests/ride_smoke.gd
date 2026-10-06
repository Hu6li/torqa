extends SceneTree
## Headless end-to-end check of the Torqa node API: load a route, save it as a course, ride it
## with the fake trainer, save a FIT file and reopen the course.
## Run: godot --headless --path app -s res://tests/ride_smoke.gd

const TIMEOUT_S: float = 20.0

var _torqa: TorqaApp
var _saved_path: String = ""
var _failure: String = ""


func _initialize() -> void:
	_torqa = TorqaApp.new()
	root.add_child(_torqa)
	_torqa.failed.connect(func(message: String) -> void: _failure = message)
	_torqa.ride_saved.connect(func(path: String) -> void: _saved_path = path)
	_run.call_deferred()


func _run() -> void:
	var gpx_path: String = OS.get_user_data_dir().path_join("smoke.gpx")
	_write_route(gpx_path)

	_torqa.load_route(gpx_path, true)
	var route: Array = await _wait_for(_torqa.route_loaded)
	_check(not route.is_empty(), "route loaded")
	var profile: PackedVector2Array = _torqa.elevation_profile(100)
	_check(profile.size() >= 2, "elevation profile available")
	await _wait_for(_torqa.world_ready)

	_check(_torqa.save_course(), "course saving started")
	var added: Array = await _wait_for(_torqa.course_added)
	var course_path: String = added[0]
	_check(course_path.ends_with(".tqc"), "course saved: %s" % course_path)
	var listed: bool = false
	for course: Dictionary in _torqa.courses():
		var path: String = course["path"]
		listed = listed or path == course_path
	_check(listed, "course listed in the library")

	_check(_torqa.connect_fake_trainer(250.0, 90.0), "fake trainer connected")
	_check(_torqa.start_ride(50.0, false, {"kind": "none"}), "ride started")
	await create_timer(2.0).timeout
	var state: Dictionary = _torqa.ride_state()
	var distance_m: float = state.get("distance_m", 0.0)
	var power: float = state.get("power", 0.0)
	_check(distance_m > 0.5, "rider moves: %s" % state)
	_check(is_equal_approx(power, 250.0), "power arrives: %s" % state)

	_torqa.finish_ride()
	_check(_saved_path.ends_with(".fit"), "ride saved: %s" % _saved_path)
	_check(_failure.is_empty(), "no failure: %s" % _failure)
	var history: Array = _torqa.history()
	_check(not history.is_empty(), "ride in the history")
	var newest: Dictionary = history[0]
	var newest_path: String = newest["path"]
	_check(newest_path == _saved_path, "newest ride first: %s" % newest)
	_check(_torqa.rename_ride(newest_path, "Smoke spin"), "ride renamed")
	var renamed: Dictionary = _torqa.history()[0]
	var renamed_name: String = renamed["name"]
	_check(renamed_name == "Smoke spin", "name kept: %s" % renamed)
	var detail: Dictionary = _torqa.ride_detail(newest_path, 100)
	var power_chart: PackedVector2Array = detail.get("power", PackedVector2Array())
	_check(not power_chart.is_empty(), "power chart: %s" % detail)

	await _workout()

	_torqa.open_course(course_path)
	var reopened: Array = await _wait_for(_torqa.route_loaded)
	var reopened_route: Dictionary = reopened[0]
	var reopened_name: String = reopened_route["name"]
	_check(reopened_name == "Smoke", "course reopened: %s" % reopened_route)
	_check(not _torqa.build_world(), "the world is built on request")
	# Leaving and re-entering the course page while its world is being built.
	_torqa.open_course(course_path)
	_torqa.build_world()
	_torqa.open_course(course_path)
	await _wait_for(_torqa.route_loaded)
	_torqa.build_world()
	await _wait_for(_torqa.world_ready)
	_check(_torqa.world_chunk_count() > 0, "world of the course opened last")
	# A workout on the course (R58): the trainer holds its power along the route.
	_check(_torqa.start_workout({"kind": "power", "power_w": 180.0}, true, false), "on a course")
	await create_timer(1.0).timeout
	var on_course: Dictionary = _torqa.ride_state()
	_check(
		on_course.has("x") and on_course.get("workout") != null, "rides the course: %s" % on_course
	)
	var held: float = on_course.get("power", 0.0)
	_check(is_equal_approx(held, 180.0), "holds 180 W: %s" % on_course)
	_torqa.abort_ride()
	DirAccess.remove_absolute(course_path)
	print("RIDE SMOKE TEST PASSED (%s)" % _saved_path)
	quit(0)


## A heart-rate workout on its own (R56, R58): the fake trainer's simulated heart, no route.
func _workout() -> void:
	var workout: Dictionary = {
		"kind": "zone", "zone": 2, "min_w": 120.0, "max_w": 200.0, "name": "Smoke zone 2"
	}
	_check(_torqa.start_workout(workout, false, false), "workout started")
	await create_timer(2.0).timeout
	var state: Dictionary = _torqa.ride_state()
	_check(not state.has("x"), "no place on a route: %s" % state)
	var info: Dictionary = state.get("workout", {})
	var target: float = info.get("target_power_w", 0.0)
	_check(target >= 120.0 and target < 125.0, "starts at the lowest power: %s" % info)
	_check(info.get("target_heart_rate") != null, "holds a heart rate: %s" % info)
	_check(state.get("heart_rate") != null, "the fake rider's heart beats: %s" % state)
	var chart: Dictionary = _torqa.ride_chart(100)
	var power: PackedVector2Array = chart["power"]
	_check(not power.is_empty(), "live chart: %s" % chart)
	_saved_path = ""
	_torqa.finish_ride()
	_check(_saved_path.ends_with(".fit"), "workout saved")
	var newest: Dictionary = _torqa.history()[0]
	var route: String = newest["route"]
	_check(route == "Smoke zone 2", "in the history by its name: %s" % newest)

	# A structured workout from the library (R21): its steps and their progress.
	var plans: Array = _torqa.workouts()
	_check(not plans.is_empty(), "built-in workouts")
	var plan: Dictionary = plans[0]
	var structured: Dictionary = {"kind": "plan", "id": plan["id"], "name": plan["name"]}
	_check(_torqa.start_workout(structured, false, false), "structured workout started")
	await create_timer(1.5).timeout
	var progress: Variant = _torqa.ride_state()["workout"]["progress"]
	_check(progress != null, "its progress: %s" % _torqa.ride_state()["workout"])
	var steps: int = progress["steps"]
	var plan_steps: Array = plan["steps"]
	_check(steps == plan_steps.size(), "as many steps as listed: %s" % progress)
	_check(
		not _torqa.start_workout({"kind": "plan", "id": "builtin:none"}, false, false), "unknown"
	)
	_torqa.abort_ride()

	# The FTP test (R22): a warm-up, then steps until the rider gives way.
	_check(_torqa.start_workout({"kind": "ftp_test", "name": "FTP test"}, false, false), "test")
	await create_timer(1.5).timeout
	var test: Dictionary = _torqa.ride_state()["workout"]
	var test_progress: Dictionary = test["progress"]
	var open_ended: int = test_progress["steps"]
	_check(open_ended == 0, "the FTP test goes on until the rider gives way: %s" % test)
	var preview: Array = _torqa.ftp_test_steps()
	_check(preview.size() > 10, "a preview of its steps: %d" % preview.size())
	_torqa.abort_ride()


## Waits for a signal and returns its arguments, failing after TIMEOUT_S.
func _wait_for(sig: Signal) -> Array:
	var result: Array = []
	var received: Array[bool] = [false]
	var on_signal: Callable = func(arg: Variant) -> void:
		result.append(arg)
		received[0] = true
	sig.connect(on_signal, CONNECT_ONE_SHOT)
	var waited: float = 0.0
	while not received[0] and waited < TIMEOUT_S:
		await process_frame
		waited += root.get_process_delta_time()
	_check(
		received[0], "signal %s within %ss (failure: %s)" % [sig.get_name(), TIMEOUT_S, _failure]
	)
	return result


func _write_route(path: String) -> void:
	var xml: String = "<gpx><trk><name>Smoke</name><trkseg>"
	for i: int in range(41):
		var lat: float = 46.0 + i * 10.0 / 111195.0
		xml += '<trkpt lat="%f" lon="7"><ele>500</ele></trkpt>' % lat
	xml += "</trkseg></trk></gpx>"
	var file: FileAccess = FileAccess.open(path, FileAccess.WRITE)
	file.store_string(xml)
	file.close()


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("RIDE SMOKE TEST FAILED: " + what)
		quit(1)
