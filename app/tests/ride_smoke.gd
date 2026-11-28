extends SceneTree
## Headless end-to-end check of the ORQWA node API: load a route, save it as a course, ride it
## with the fake trainer, save a FIT file and reopen the course.
## Run: godot --headless --path app -s res://tests/ride_smoke.gd

const TIMEOUT_S: float = 20.0

var _orqwa: OrqwaApp
var _saved_path: String = ""
var _failure: String = ""


func _initialize() -> void:
	_orqwa = OrqwaApp.new()
	root.add_child(_orqwa)
	_orqwa.failed.connect(func(message: String) -> void: _failure = message)
	_orqwa.ride_saved.connect(func(path: String) -> void: _saved_path = path)
	_run.call_deferred()


func _run() -> void:
	var gpx_path: String = OS.get_user_data_dir().path_join("smoke.gpx")
	_write_route(gpx_path)

	_orqwa.load_route(gpx_path, true)
	var route: Array = await _wait_for(_orqwa.route_loaded)
	_check(not route.is_empty(), "route loaded")
	var profile: PackedVector2Array = _orqwa.elevation_profile(100)
	_check(profile.size() >= 2, "elevation profile available")
	_check_grade_colors()
	# How long the rider would take (R66): at a share of their FTP, no quicker with descents
	# ridden flat.
	var estimate: Dictionary = _orqwa.estimated_time(false)
	var flat_estimate: Dictionary = _orqwa.estimated_time(true)
	var share: float = estimate["ftp_share_pct"]
	_check(estimate["time_s"] != null and share == 75.0, "estimated time: %s" % estimate)
	var estimated_s: float = estimate["time_s"]
	var flat_s: float = flat_estimate["time_s"]
	_check(flat_s >= estimated_s, "flat descents: %s" % flat_estimate)
	await _wait_for(_orqwa.world_ready)
	var chunk: Dictionary = _orqwa.world_chunk(0)
	_check(chunk.has("urban"), "urban surface mesh exported by the core")
	var treeless: Dictionary = _orqwa.world_chunk_with_tree_density(0, 0.0)
	var remaining_plants: Dictionary = treeless["plants"]
	for model: String in remaining_plants:
		_check(
			model.get_slice("_", 0) not in ["conifer", "broadleaf", "palm", "banana"],
			"tree filtering happens before upload: " + model
		)
	var unchanged: Dictionary = _orqwa.world_chunk(0)
	var original_plants: Dictionary = chunk["plants"]
	var unchanged_plants: Dictionary = unchanged["plants"]
	_check(original_plants == unchanged_plants, "tree quality preserves the complete world")
	# A world kept from an older ORQWA may name models this one no longer has (R101).
	var unknown: Array = [
		BuildingModels.mesh("gone"), VegetationModels.mesh("gone"), PropModels.mesh("gone")
	]
	_check(unknown == [null, null, null], "unknown models are left out")

	_check(_orqwa.save_course(), "course saving started")
	var added: Array = await _wait_for(_orqwa.course_added)
	var course_path: String = added[0]
	_check(course_path.ends_with(".oqc"), "course saved: %s" % course_path)
	var listed: bool = false
	for course: Dictionary in _orqwa.courses():
		var path: String = course["path"]
		listed = listed or path == course_path
	_check(listed, "course listed in the library")
	var first: Dictionary = _orqwa.courses()[0]
	var first_countries: Variant = first.get("countries")
	_check(first_countries is Array, "the courses list their countries: %s" % first)

	_segments()

	_check(_orqwa.connect_fake_trainer(250.0, 90.0), "fake trainer connected")
	_check(_orqwa.start_ride(50.0, false, {"kind": "none"}), "ride started")
	await create_timer(2.0).timeout
	var state: Dictionary = _orqwa.ride_state()
	# On the segment from the start, timed on the HUD (R302).
	var on_segment: Variant = state.get("segment")
	var segment_state: Dictionary = on_segment if on_segment is Dictionary else {}
	var segment_name: String = segment_state.get("name", "")
	var reached: bool = segment_state.get("ahead_m", 0.0) == null
	_check(segment_name == "Smoke sprint" and reached, "on the segment: %s" % segment_state)
	var panel: SegmentPanel = SegmentPanel.new()
	root.add_child(panel)
	panel.show_segment(on_segment)
	_check(panel.visible, "the segment's panel shows")
	panel.show_segment(null)
	_check(not panel.visible, "and hides away from segments")
	panel.free()
	for sprint: Dictionary in _orqwa.segments():
		var sprint_id: String = sprint["id"]
		_check(_orqwa.remove_segment(sprint_id), "segment removed")
	_check(_orqwa.segments().is_empty(), "no segments left")
	var distance_m: float = state.get("distance_m", 0.0)
	var power: float = state.get("power", 0.0)
	_check(distance_m > 0.5, "rider moves: %s" % state)
	_check(is_equal_approx(power, 250.0), "power arrives: %s" % state)
	var centre_x: float = state["x"]
	var centre_y: float = state["y"]
	var rider_x: float = state["rider_x"]
	var rider_y: float = state["rider_y"]
	var centre: Vector2 = Vector2(centre_x, centre_y)
	var avatar: Vector2 = Vector2(rider_x, rider_y)
	var heading: float = state["heading"]
	var right: Vector2 = Vector2(cos(heading), -sin(heading))
	_check((avatar - centre).dot(right) > 0.0, "rider keeps right of the route (R120)")
	var metrics: Dictionary = state.get("metrics", {})
	_check(metrics.get("time_remaining") != null, "time to go on the HUD: %s" % metrics)
	# The ride settings show the devices, and the connected trainer needs no reconnect (R69).
	var devices: Array = _orqwa.devices()
	var trainer: Dictionary = devices[0] if devices.size() == 1 else {}
	var trainer_connected: bool = trainer.get("connected", false)
	var kind: String = trainer.get("kind", "")
	_check(kind == "trainer" and trainer_connected, "devices during the ride: %s" % [devices])
	# What the trainer reports is at hand for the devices screen too (R80).
	var reading: Dictionary = _orqwa.device_reading("trainer")
	var reading_power: float = reading.get("power", 0.0)
	_check(is_equal_approx(reading_power, 250.0), "the trainer's reading: %s" % reading)
	var ride_devices: RideDevices = RideDevices.new()
	ride_devices.bind(_orqwa)
	root.add_child(ride_devices)
	ride_devices.refresh()
	var reconnect: Button = ride_devices.find_children("*", "Button", true, false)[0]
	_check(reconnect.disabled, "nothing to reconnect with every device connected")
	ride_devices.free()
	await _ambient_sound()

	_orqwa.finish_ride()
	_check(not _orqwa.ambient_sound_active(), "finishing stops ambient sound")
	_check(_saved_path.ends_with(".fit"), "ride saved: %s" % _saved_path)
	_check(_failure.is_empty(), "no failure: %s" % _failure)
	var history: Array = _orqwa.history()
	_check(not history.is_empty(), "ride in the history")
	var newest: Dictionary = history[0]
	var newest_path: String = newest["path"]
	_check(newest_path == _saved_path, "newest ride first: %s" % newest)
	_check(_orqwa.rename_ride(newest_path, "Smoke spin"), "ride renamed")
	var renamed: Dictionary = _orqwa.history()[0]
	var renamed_name: String = renamed["name"]
	_check(renamed_name == "Smoke spin", "name kept: %s" % renamed)
	# Exported under its name to a place outside the data directory (R28, R50).
	var file_name: String = OrqwaApp.ride_export_file_name(renamed_name)
	_check(file_name == "Smoke spin.fit", "exported under its name: %s" % file_name)
	var exported: String = OS.get_user_data_dir().path_join(file_name)
	_check(_orqwa.export_ride(newest_path, exported), "ride exported")
	_check(
		FileAccess.get_file_as_bytes(exported) == FileAccess.get_file_as_bytes(newest_path),
		"the export is the ride's FIT file"
	)
	DirAccess.remove_absolute(exported)
	_check(not _orqwa.export_ride(newest_path, "/nonexistent/x.fit"), "a failed export is told")
	_check(not _failure.is_empty(), "with the reason")
	_failure = ""
	var detail: Dictionary = _orqwa.ride_detail(newest_path, 100)
	var power_chart: PackedVector2Array = detail.get("power", PackedVector2Array())
	_check(not power_chart.is_empty(), "power chart: %s" % detail)
	# The ride's best power over each duration, held at the fake trainer's 250 W (R62).
	var curve: PackedVector2Array = newest.get("power_curve", PackedVector2Array())
	_check(
		not curve.is_empty() and curve[0] == Vector2(1, 250), "the ride's power curve: %s" % curve
	)
	var curves: Dictionary = _orqwa.power_curves(0, false, false)
	var bests: Array = curves.get("period", [])
	_check(not bests.is_empty(), "the rider's power curve: %s" % curves)
	# The gallery (R67): the course found by its name, with the rider's estimated time and
	# the ride just saved; a search that matches nothing shows no group.
	var library: Dictionary = _orqwa.course_library(
		{"search": "SMO", "sort": "ridden", "descending": true, "group": "terrain"}
	)
	var found: Dictionary = {}
	var groups: Array = library["groups"]
	for group: Dictionary in groups:
		var group_courses: Array = group["courses"]
		for course: Dictionary in group_courses:
			if course["path"] == course_path:
				found = course
	_check(
		found.get("estimated_s") != null and found.get("last_ridden_unix_s") != null,
		"the course in the gallery: %s" % found
	)
	var nothing: Dictionary = _orqwa.course_library({"search": "no such course"})
	var no_groups: Array = nothing["groups"]
	var total: int = nothing["total"]
	_check(no_groups.is_empty() and total > 0, "a search matching nothing: %s" % nothing)

	await _workout()

	# The course's page (R40): loaded already, it is ready at once; opened while the course
	# loads, a spinner turns and Ride waits until its route is in.
	var page: CourseDetail = CourseDetail.new()
	root.add_child(page)
	page.bind(_orqwa)
	var spinner: Spinner = page.get("_spinner")
	var ride: Button = page.get("_ride_button")
	page.open(found)
	_check(not spinner.visible and not ride.disabled, "a loaded course is ready at once")
	_orqwa.open_course(course_path)
	page.open(found)
	_check(spinner.is_visible_in_tree() and ride.disabled, "a spinner while the course loads")
	var reopened: Array = await _wait_for(_orqwa.route_loaded)
	var figures: VBoxContainer = page.get("_figures")
	var estimated: bool = figures.find_children("*", "Label", true, false).any(
		func(label: Label) -> bool: return label.text.begins_with("Time at")
	)
	_check(
		not spinner.visible and not ride.disabled and estimated,
		"the spinner stops once the course is loaded, its estimate shown"
	)
	page.free()
	var reopened_route: Dictionary = reopened[0]
	var reopened_name: String = reopened_route["name"]
	_check(reopened_name == "Smoke", "course reopened: %s" % reopened_route)
	_check(not _orqwa.build_world(false, INF), "the world is built on request")
	# Leaving and re-entering the course page while its world is being built.
	_orqwa.open_course(course_path)
	_orqwa.build_world(false, INF)
	_orqwa.open_course(course_path)
	await _wait_for(_orqwa.route_loaded)
	_orqwa.build_world(false, INF)
	await _wait_for(_orqwa.world_ready)
	_check(_orqwa.world_chunk_count() > 0, "world of the course opened last")
	# A workout on the course (R58): the trainer holds its power along the route.
	_check(_orqwa.start_workout({"kind": "power", "power_w": 180.0}, true, false), "on a course")
	await create_timer(1.0).timeout
	var on_course: Dictionary = _orqwa.ride_state()
	_check(
		on_course.has("x") and on_course.get("workout") != null, "rides the course: %s" % on_course
	)
	var held: float = on_course.get("power", 0.0)
	_check(is_equal_approx(held, 180.0), "holds 180 W: %s" % on_course)
	_orqwa.abort_ride()

	# Virtual gears (R9): a rider on a single cog shifts from the keyboard.
	var rider: Dictionary = _orqwa.profile()
	var id: String = rider["id"]
	var on_cog: Dictionary = rider.duplicate()
	on_cog["drivetrain"] = "single_cog"
	_check(_orqwa.save_profile(id, on_cog) == id, "rider on a single cog")
	_check(_orqwa.start_ride(50.0, false, {"kind": "none"}), "ride with virtual gears")
	await process_frame
	var gear: Dictionary = _orqwa.ride_state().get("gear", {})
	var first_gear: int = gear.get("number", 0)
	_orqwa.shift(1)
	await process_frame
	var shifted: Dictionary = _orqwa.ride_state().get("gear", {})
	var second_gear: int = shifted.get("number", 0)
	_check(
		first_gear > 0 and second_gear == first_gear + 1, "shifted up: %s → %s" % [gear, shifted]
	)
	# The profile changes during the ride: the ride follows at once, the profile keeps it (R98).
	on_cog["drivetrain"] = "cassette"
	_orqwa.save_profile(id, on_cog)
	var no_gear: bool = _orqwa.ride_state().get("gear") == null
	_check(no_gear, "a cassette during the ride")
	var kept: String = _orqwa.profile()["drivetrain"]
	_check(kept == "cassette", "kept in the profile")
	on_cog["drivetrain"] = "single_cog"
	_orqwa.save_profile(id, on_cog)
	var again: Dictionary = _orqwa.ride_state().get("gear", {})
	var again_number: int = again.get("number", 0)
	_check(again_number == first_gear, "virtual gears again, from the real gear")
	_orqwa.abort_ride()
	_orqwa.save_profile(id, rider)

	# Riders can be deleted (R22); the app switches to one left.
	var guest: String = _orqwa.save_profile("", {"name": "Guest"})
	var guest_drivetrain: String = _orqwa.profile()["drivetrain"]
	_check(guest_drivetrain == "single_cog", "a new rider shifts virtual gears (R98)")
	# Elevation colours are the rider's choice, off until they opt in (R242).
	var plain: bool = _orqwa.profile()["gradient_colours"]
	_check(not plain, "a new rider sees the plain profile")
	var colourful: Dictionary = _orqwa.profile().duplicate()
	colourful["gradient_colours"] = true
	_orqwa.save_profile(guest, colourful)
	var opted_in: bool = _orqwa.profile()["gradient_colours"]
	_check(opted_in, "the rider opted in to gradient colours")
	_check(_orqwa.delete_profile(guest), "a rider is deleted")
	var active: String = _orqwa.profile()["id"]
	var gone: bool = _orqwa.profiles().all(func(p: Dictionary) -> bool: return p["id"] != guest)
	_check(gone and active != guest, "the deleted rider is gone")
	_orqwa.select_profile(id)

	# Di2 buttons (#139): any press of a channel can be given an action, or none.
	_check(_orqwa.assign_button("di2", 3, 1, "next_camera"), "holding channel 3 moves the camera")
	var buttons: Array = _orqwa.button_map("di2")
	var third: PackedStringArray = buttons[2]
	_check(third[1] == "next_camera" and third[0] == "", "assigned: %s" % [buttons])
	_check(not _orqwa.assign_button("di2", 3, 1, "fly"), "unknown actions are refused")
	_check(_orqwa.assign_button("di2", 1, 0, "pause"), "channel 1 pauses the ride")
	# Each shifter has its own (R84): the Di2's channel 1 is not a Click's +.
	var plus: PackedStringArray = _orqwa.button_map("zwift_click")[0]
	_check(plus[0] == "shift_up", "a Click's + still shifts: %s" % [plus])
	_check(not _orqwa.assign_button("zwift_play", 1, 0, "pause"), "unknown shifters are refused")
	_orqwa.assign_button("di2", 1, 0, "shift_up")
	_orqwa.assign_button("di2", 3, 1, "")
	DirAccess.remove_absolute(course_path)
	print("RIDE SMOKE TEST PASSED (%s)" % _saved_path)
	quit(0)


## A heart-rate workout on its own (R56, R58): the fake trainer's simulated heart, no route.
func _workout() -> void:
	var workout: Dictionary = {
		"kind": "zone", "zone": 2, "min_w": 120.0, "max_w": 200.0, "name": "Smoke zone 2"
	}
	_check(_orqwa.start_workout(workout, false, false), "workout started")
	await create_timer(2.0).timeout
	var state: Dictionary = _orqwa.ride_state()
	_check(not state.has("x"), "no place on a route: %s" % state)
	_check(not _orqwa.ambient_sound_active(), "workouts without a course stay quiet")
	var info: Dictionary = state.get("workout", {})
	var target: float = info.get("target_power_w", 0.0)
	_check(target >= 120.0 and target < 125.0, "starts at the lowest power: %s" % info)
	_check(info.get("target_heart_rate") != null, "holds a heart rate: %s" % info)
	_check(state.get("heart_rate") != null, "the fake rider's heart beats: %s" % state)
	var chart: Dictionary = _orqwa.ride_chart(100)
	var power: PackedVector2Array = chart["power"]
	_check(not power.is_empty(), "live chart: %s" % chart)
	_saved_path = ""
	_orqwa.finish_ride()
	_check(_saved_path.ends_with(".fit"), "workout saved")
	var newest: Dictionary = _orqwa.history()[0]
	var route: String = newest["route"]
	_check(route == "Smoke zone 2", "in the history by its name: %s" % newest)

	# A structured workout from the library (R21): its steps and their progress.
	var plans: Array = _orqwa.workouts()
	_check(not plans.is_empty(), "built-in workouts")
	var plan: Dictionary = plans[0]
	var structured: Dictionary = {"kind": "plan", "id": plan["id"], "name": plan["name"]}
	_check(_orqwa.start_workout(structured, false, false), "structured workout started")
	await create_timer(1.5).timeout
	var progress: Variant = _orqwa.ride_state()["workout"]["progress"]
	_check(progress != null, "its progress: %s" % _orqwa.ride_state()["workout"])
	var steps: int = progress["steps"]
	var plan_steps: Array = plan["steps"]
	_check(steps == plan_steps.size(), "as many steps as listed: %s" % progress)
	# A shifter's buttons (#139) skip a step and change the workout's powers and difficulty.
	_check(_orqwa.skip_step(), "a step skipped")
	var skipped: Dictionary = _orqwa.ride_state()["workout"]["progress"]
	var skipped_to: int = skipped["step"]
	_check(skipped_to == 1, "on with the next step: %s" % skipped)
	_check(is_equal_approx(_orqwa.step_intensity(1), 105.0), "5 % harder")
	var harder: float = _orqwa.ride_state()["workout"]["intensity_pct"]
	_check(is_equal_approx(harder, 105.0), "the intensity shows: %s" % harder)
	_check(_orqwa.step_difficulty(-10) == 0.0, "no easier than flat")
	_check(
		not _orqwa.start_workout({"kind": "plan", "id": "builtin:none"}, false, false), "unknown"
	)
	_orqwa.abort_ride()

	# The FTP test (R22): a warm-up, then steps until the rider gives way.
	_check(_orqwa.start_workout({"kind": "ftp_test", "name": "FTP test"}, false, false), "test")
	await create_timer(1.5).timeout
	var test: Dictionary = _orqwa.ride_state()["workout"]
	var test_progress: Dictionary = test["progress"]
	var open_ended: int = test_progress["steps"]
	_check(open_ended == 0, "the FTP test goes on until the rider gives way: %s" % test)
	_check(not _orqwa.skip_step(), "the test's steps cannot be skipped")
	_check(_orqwa.step_intensity(1) < 0.0, "nor its powers changed")
	var preview: Array = _orqwa.ftp_test_steps("ramp")
	_check(preview.size() > 10, "a preview of its steps: %d" % preview.size())
	_orqwa.abort_ride()

	# The 20-minute test (#125): counted steps, its 20 minutes left to the rider.
	var twenty: Dictionary = {"kind": "ftp_test", "test": "twenty_minutes", "name": "FTP test"}
	_check(_orqwa.start_workout(twenty, false, false), "20-minute test")
	await create_timer(1.5).timeout
	var counted: Dictionary = _orqwa.ride_state()["workout"]["progress"]
	var counted_steps: int = counted["steps"]
	_check(counted_steps > 10, "its steps are counted out: %s" % counted)
	var all_out: bool = false
	for step: Dictionary in _orqwa.ftp_test_steps("twenty_minutes"):
		var seconds: float = step["duration_s"]
		var marked: bool = step["all_out"]
		var free: bool = step["from_w"] == null
		all_out = all_out or (marked and free and is_equal_approx(seconds, 1200.0))
	_check(all_out, "20 minutes all out, without a set power")
	_orqwa.abort_ride()

	# Workouts from the editor (R21): saved into the library, replaced, deleted.
	var made: Dictionary = {
		"name": "Smoke & tempo",
		"description": "",
		"steps": [{"duration_s": 60.0, "from_pct": 80.0, "to_pct": 80.0, "free": false}],
	}
	var id: String = _orqwa.save_workout(made, "")
	_check(id.ends_with(".zwo"), "saved as ZWO: %s" % id)
	var again: String = _orqwa.save_workout(made, id)
	_check(again == id, "edited in place")
	var found: bool = false
	for entry: Dictionary in _orqwa.workouts():
		found = found or (entry["id"] == id and entry["name"] == "Smoke & tempo")
	_check(found, "in the library by its name")
	_check(_orqwa.delete_workout(id), "deleted")
	_check(not _orqwa.delete_workout("builtin:recovery-30"), "built-ins stay")
	_failure = ""


## R114/R153: rainy PCM reaches Godot; pause and hidden screens flush the playback queue.
func _ambient_sound() -> void:
	_check(_orqwa.ambient_sound_active(), "ambient sound during a 3D ride")
	_check(OrqwaApp.ambient_sound_rate() == 48000, "ambient sample rate")
	_orqwa.set_ambient_sound(50.0, 0.0, "Rain")
	var samples: PackedVector2Array = _orqwa.ambient_sound(2400)
	_check(samples.size() == 2400, "stereo PCM from the live ride")
	var audible: bool = false
	for sample: Vector2 in samples:
		audible = audible or sample.length_squared() > 0.000001
	_check(audible, "rainy ambience is audible with bicycle sound muted")
	var sound: RideSound = RideSound.new()
	root.add_child(sound)
	sound.update(_orqwa, true)
	_check(sound.playing, "generator starts during the ride")
	_orqwa.set_paused(true)
	sound.update(_orqwa, true)
	_check(not sound.playing, "pause stops and drops queued samples")
	_check(_orqwa.ambient_sound(2400).is_empty(), "paused core produces no PCM")
	_orqwa.set_paused(false)
	sound.update(_orqwa, true)
	_check(sound.playing, "resume starts a fresh playback queue")
	sound.update(_orqwa, false)
	_check(not sound.playing, "hidden ride screen stops sound")
	sound.free()
	var options: RideOptions = RideOptions.new()
	root.add_child(options)
	options.set_options({"wind_volume": 0.0, "bike_volume": 75.0})
	var wind_volume: float = options.options()["wind_volume"]
	var bike_volume: float = options.options()["bike_volume"]
	_check(wind_volume == 0.0, "wind can be muted independently")
	_check(bike_volume == 75.0, "bicycle volume survives settings edits")
	options.show_option_groups(false, true)
	var wind: HSlider = options.get("_wind_volume")
	_check(not wind.visible, "ambient controls are hidden on video rides")
	options.free()
	await process_frame


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


## The rider marks a segment of the loaded course on its page (R302): typed in or clicked on
## the profile, listed with its length, renamed, and only saved when long enough to time.
func _segments() -> void:
	var list: SegmentList = SegmentList.new()
	list.bind(_orqwa)
	root.add_child(list)
	# Left by an interrupted run: the rider's data outlives it.
	for left: Dictionary in _orqwa.segments():
		var left_id: String = left["id"]
		_orqwa.remove_segment(left_id)
	var marked: Array = []
	list.changed.connect(func(segments: Array) -> void: marked.assign(segments))
	list.show_segments(400.0)
	_check(list.visible and marked.is_empty(), "no segments yet: %s" % [marked])
	var form: Control = list.get("_form")
	var save: Button = list.get("_save")
	var field: LineEdit = list.get("_name")
	var actions: HBoxContainer = list.get("_actions")
	var add: Button = actions.get_child(0)
	add.pressed.emit()
	_check(form.visible and save.disabled, "a new segment waits for its name")
	field.text = "Smoke"
	field.text_changed.emit(field.text)
	list.pick(100.0)
	list.pick(120.0)
	_check(save.disabled, "20 m is too short to time")
	list.pick(0.0)
	list.pick(300.0)
	_check(not save.disabled, "the profile's clicks set its ends")
	_drag_on_profile(list)
	save.pressed.emit()
	var first: Dictionary = marked[0] if marked.size() == 1 else {}
	var length: float = first.get("length_m", 0.0)
	var id: String = first.get("id", "")
	_check(
		not form.visible and is_equal_approx(length, 300.0), "the segment is listed: %s" % [marked]
	)
	_check(_orqwa.save_segment(id, "Smoke sprint", 0.0, 300.0), "renamed")
	# Renamed in miles, which the fields show to a hundredth (0.19 mi), it stays where it was.
	list.set("_imperial", true)
	list.call("_open_form", id, "Smoke sprint", 0.0, 300.0)
	field.text = "Smoke sprint, miles"
	field.text_changed.emit(field.text)
	save.pressed.emit()
	var renamed: Dictionary = _orqwa.segments()[0]
	var renamed_name: String = renamed.get("name", "")
	var renamed_end: float = renamed.get("end_m", 0.0)
	_check(
		renamed_name == "Smoke sprint, miles" and is_equal_approx(renamed_end, 300.0),
		"not moved by the miles: %s" % [renamed]
	)
	list.set("_imperial", false)
	_check(_orqwa.save_segment(id, "Smoke sprint", 0.0, 300.0), "renamed back")
	# A course prepared differently since: the row asks to check the ends, the form says why.
	var listed_now: Dictionary = _orqwa.segments()[0]
	var to_check: Dictionary = listed_now.duplicate()
	var set_here: bool = to_check.get("confirmed", false)
	_check(set_here, "set on this course: %s" % [to_check])
	to_check["confirmed"] = false
	var row: HBoxContainer = list.call("_row", to_check)
	var asks: Array[Button] = []
	for child: Node in row.get_children():
		if child is Button and (child as Button).text == "Check ends":
			asks.append(child)
	_check(asks.size() == 1, "the row asks to check its ends")
	var note: Label = list.get("_check_note")
	if asks.size() == 1:
		asks[0].pressed.emit()
	_check(form.visible and note.visible, "the form says why")
	list.call("_close_form")
	row.free()
	add.pressed.emit()
	_check(not note.visible, "no note for a new segment")
	list.call("_close_form")
	# Listed, but not drawn on the profile or the maps until checked: it would not be timed.
	to_check["id"] = "tocheck"
	list.call("_list", [listed_now, to_check])
	var drawn_ids: Array = marked.map(func(s: Dictionary) -> String: return s["id"])
	_check(drawn_ids == [id], "only the checked one is drawn: %s" % [drawn_ids])
	_check(SegmentList.drawn([to_check]).is_empty(), "none drawn on the ride")
	list.call("_refresh")
	# The maps draw its stretch of road: due north from the start for 300 m.
	var listed: Dictionary = _orqwa.segments()[0]
	var stretch: PackedVector2Array = listed.get("track", PackedVector2Array())
	var ends_ok: bool = (
		stretch.size() >= 2
		and stretch[0].length() < 2.0
		and stretch[stretch.size() - 1].distance_to(Vector2(0.0, 300.0)) < 2.0
	)
	_check(ends_ok, "the stretch for the maps: %s" % stretch)
	_check(not _orqwa.save_segment("", "Elsewhere", 300.0, 900.0), "not beyond the finish")
	_check(not _failure.is_empty(), "the refusal is reported")
	_failure = ""
	_check(_orqwa.segments().size() == 1, "one segment: %s" % [_orqwa.segments()])
	list.free()


## A drag across the course page's profile marks the stretch, either way, following the
## pointer; it stays, and its ends or middle drag on until saved (R304). A press released where
## it went down stays a click (R302). Leaves 0–300 m marked. 1 px is 1 m here.
func _drag_on_profile(list: SegmentList) -> void:
	var profile: ElevationProfile = ElevationProfile.new()
	root.add_child(profile)
	profile.size = Vector2(400.0, 60.0)
	profile.set_profile(PackedVector2Array([Vector2(0.0, 500.0), Vector2(400.0, 520.0)]))
	profile.jumpable = true
	profile.stretchable = true
	profile.jump_requested.connect(list.pick)
	profile.stretch_picked.connect(list.pick_stretch)
	list.draft_changed.connect(profile.set_draft)
	list.pick_stretch(0.0, 300.0)
	_check(_draft(profile).is_equal_approx(Vector2(0.0, 300.0)), "the form's ends show")
	# Outside it, a press starts a new stretch.
	_press(profile, 350.0, true)
	_move(profile, 353.0)
	_check(_ends(list).is_equal_approx(Vector2(0.0, 300.0)), "a twitch is no drag")
	_move(profile, 380.0)
	_check(_ends(list).is_equal_approx(Vector2(350.0, 380.0)), "the ends follow the drag")
	_press(profile, 390.0, false)
	_check(_ends(list).is_equal_approx(Vector2(350.0, 390.0)), "released: %s" % _ends(list))
	_check(_draft(profile).is_equal_approx(Vector2(350.0, 390.0)), "it stays once released")
	# Its start, taken near it, drags on its own…
	_press(profile, 352.0, true)
	_move(profile, 200.0)
	_press(profile, 200.0, false)
	_check(_ends(list).is_equal_approx(Vector2(200.0, 390.0)), "start moved: %s" % _ends(list))
	# …its middle moves all of it, and no further than the course's start.
	_press(profile, 300.0, true)
	_move(profile, 250.0)
	_press(profile, 250.0, false)
	_check(_ends(list).is_equal_approx(Vector2(150.0, 340.0)), "shifted: %s" % _ends(list))
	_press(profile, 200.0, true)
	_move(profile, 0.0)
	_press(profile, 0.0, false)
	_check(_ends(list).is_equal_approx(Vector2(0.0, 190.0)), "at the start: %s" % _ends(list))
	# A new one dragged backwards from beyond it.
	_press(profile, 300.0, true)
	_move(profile, 0.0)
	_press(profile, 0.0, false)
	_check(_ends(list).is_equal_approx(Vector2(0.0, 300.0)), "dragged back: %s" % _ends(list))
	var clicked: Array[float] = []
	profile.jump_requested.connect(func(at: float) -> void: clicked.append(at))
	_press(profile, 200.0, true)
	_check(clicked.is_empty(), "a click goes out once released")
	_press(profile, 201.0, false)
	_check(
		clicked.size() == 1 and is_equal_approx(clicked[0], 200.0),
		"a click on release: %s" % [clicked]
	)
	# The click was the start; put it back for the rest.
	list.pick_stretch(0.0, 300.0)
	profile.stretchable = false
	_check(_draft(profile) == Vector2(-1.0, -1.0), "gone once the form closes")
	profile.free()


## The stretch the profile shows as marked; `(-1, -1)` when none.
func _draft(profile: ElevationProfile) -> Vector2:
	var draft: Array[float] = profile.get("_draft")
	return Vector2(draft[0], draft[1]) if draft.size() == 2 else Vector2(-1.0, -1.0)


## The marked segment's ends in metres, as its form shows them.
func _ends(list: SegmentList) -> Vector2:
	var start: SpinBox = list.get("_start")
	var end: SpinBox = list.get("_end")
	return Vector2(start.value, end.value) * 1000.0


func _press(profile: ElevationProfile, x: float, down: bool) -> void:
	var button: InputEventMouseButton = InputEventMouseButton.new()
	button.button_index = MOUSE_BUTTON_LEFT
	button.pressed = down
	button.position = Vector2(x, 30.0)
	profile.call("_gui_input", button)


func _move(profile: ElevationProfile, x: float) -> void:
	var motion: InputEventMouseMotion = InputEventMouseMotion.new()
	motion.position = Vector2(x, 30.0)
	profile.call("_gui_input", motion)


func _write_route(path: String) -> void:
	var xml: String = "<gpx><trk><name>Smoke</name><trkseg>"
	for i: int in range(41):
		var lat: float = 46.0 + i * 10.0 / 111195.0
		xml += '<trkpt lat="%f" lon="7"><ele>500</ele></trkpt>' % lat
	xml += "</trkseg></trk></gpx>"
	var file: FileAccess = FileAccess.open(path, FileAccess.WRITE)
	file.store_string(xml)
	file.close()


## The profile's area is yellow on the flat, green going down and red going up, darker the
## steeper, and no darker beyond the steepest shade (R241).
func _check_grade_colors() -> void:
	var flat: Color = ElevationProfile.grade_color(0.0)
	_check(flat.r > 0.9 and flat.g > 0.8 and flat.b < 0.4, "flat is yellow: %s" % flat)
	for grade: float in [3.0, 10.0, 25.0]:
		var up: Color = ElevationProfile.grade_color(grade)
		var down: Color = ElevationProfile.grade_color(-grade)
		_check(up.r > up.g and up.r > up.b, "%.0f%% up is red: %s" % [grade, up])
		_check(down.g > down.r and down.g > down.b, "%.0f%% down is green: %s" % [grade, down])
	for pair: Vector2 in [Vector2(3, 10), Vector2(10, 20), Vector2(-3, -10), Vector2(-10, -20)]:
		var gentle: Color = ElevationProfile.grade_color(pair.x)
		var steep: Color = ElevationProfile.grade_color(pair.y)
		_check(steep.get_luminance() < gentle.get_luminance(), "steeper is darker: %s" % pair)
	_check(
		ElevationProfile.grade_color(40.0) == ElevationProfile.grade_color(25.0),
		"25% and steeper share the darkest red"
	)
	# Each point's colour comes from the road around it, so a steady climb is one colour and
	# metres of elevation noise do not make the area flicker between colours.
	var steady: PackedVector2Array = PackedVector2Array()
	var noisy: PackedVector2Array = PackedVector2Array()
	for i: int in range(251):
		steady.append(Vector2(i * 4.0, i * 0.2))
		noisy.append(Vector2(i * 4.0, 0.5 if i % 2 == 0 else -0.5))
	for grade: float in ElevationProfile.point_grades(steady, 50.0):
		_check(absf(grade - 5.0) < 0.01, "a steady 5%% climb stays 5%%: %.2f" % grade)
	for grade: float in ElevationProfile.point_grades(noisy, 50.0):
		_check(absf(grade) < 5.0, "noise of ±25%% between points is smoothed: %.2f" % grade)


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("RIDE SMOKE TEST FAILED: " + what)
		quit(1)
