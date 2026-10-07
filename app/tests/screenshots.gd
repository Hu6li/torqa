extends SceneTree
## Renders the ride screen in each camera mode to PNG files, for checking visuals without a
## GPU (software Vulkan). Run via scripts/screenshots.sh.

const DEFAULT_ROUTE: String = "res://../core/fixtures/gurtenstrasse.gpx"
const TIMEOUT_S: float = 180.0

var _main: Control


func _initialize() -> void:
	_main = (load("res://scenes/main.tscn") as PackedScene).instantiate()
	root.add_child(_main)
	_run.call_deferred()


func _run() -> void:
	var out_dir: String = OS.get_environment("SCREENSHOT_DIR")
	var torqa: TorqaApp = _main.get_node("Torqa")
	var world: RideWorld = _main.get_node("World")
	var start: StartPage = _main.get_node("StartPage")

	var route: String = OS.get_environment("SCREENSHOT_ROUTE")
	if route.is_empty():
		route = ProjectSettings.globalize_path(DEFAULT_ROUTE)
	var ride_s: float = OS.get_environment("SCREENSHOT_RIDE_S").to_float()
	# Another interface language, e.g. SCREENSHOT_LOCALE=de.
	var locale: String = OS.get_environment("SCREENSHOT_LOCALE")
	if not locale.is_empty():
		TranslationServer.set_locale(locale)
	await create_timer(0.5).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("start-courses.png"))
	# Import the route as a new course, as the Courses tab's import does.
	var courses: CoursesTab = start.find_children("*", "CoursesTab", true, false)[0]
	courses.call("_on_file_selected", route)
	await create_timer(0.5).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("import-name.png"))
	# The course is named as suggested (#40); confirming closes the dialog.
	var name_dialog: ConfirmationDialog = courses.get("_name_dialog")
	name_dialog.hide()
	courses.call("_on_name_confirmed")
	await create_timer(1.0).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("loading.png"))
	# The rider settings with the HUD editor.
	var dialog: ProfileDialog = start.find_children("*", "ProfileDialog", true, false)[0]
	dialog.edit(torqa.profile(), torqa.hud_layout())
	var dialog_tabs: TabContainer = dialog.find_children("*", "TabContainer", true, false)[0]
	dialog_tabs.current_tab = 1
	await create_timer(0.5).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("rider-settings.png"))
	dialog.hide()
	await _wait_for(torqa.course_added)
	# The prepared course opens on its detail page.
	await create_timer(1.0).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("course-detail.png"))
	print("saved course detail")
	# Let the world stream its chunks in.
	for i: int in range(240):
		await process_frame
	# A custom HUD, e.g. SCREENSHOT_HUD=power_3s,speed,normalized_power,upcoming_grade
	var hud: String = OS.get_environment("SCREENSHOT_HUD")
	if not hud.is_empty():
		torqa.set_hud_layout(PackedStringArray(hud.split(",")))
	# A graphics preset, e.g. SCREENSHOT_QUALITY=ultra (R43).
	var quality: String = OS.get_environment("SCREENSHOT_QUALITY")
	if not quality.is_empty():
		_check(torqa.set_graphics_quality(quality), "graphics quality %s" % quality)
	_check(torqa.connect_fake_trainer(250.0, 90.0), "fake trainer")
	# A pacer to race, e.g. SCREENSHOT_GHOST=300 (watts).
	var ghost: Dictionary = {"kind": "none"}
	var ghost_watts: String = OS.get_environment("SCREENSHOT_GHOST")
	if not ghost_watts.is_empty():
		ghost = {"kind": "power", "watts": ghost_watts.to_float()}
	_check(torqa.start_ride(50.0, false, ghost), "ride started")
	start.ride_started.emit(start.ride_options())
	var time: String = OS.get_environment("SCREENSHOT_TIME")
	var weather: String = OS.get_environment("SCREENSHOT_WEATHER")
	if not time.is_empty() or not weather.is_empty():
		world.apply_conditions(
			time if not time.is_empty() else "Midday",
			weather if not weather.is_empty() else "Clear"
		)
	await create_timer(maxf(ride_s, 4.0)).timeout

	for mode: int in range(3):
		await create_timer(1.5).timeout
		var image: Image = root.get_texture().get_image()
		var path: String = out_dir.path_join("ride-%d.png" % mode)
		image.save_png(path)
		print("saved ", path)
		world.cycle_camera()

	# A close side view of the rider, for checking the avatar.
	var rider: Node3D = world.get_node("Rider")
	var side: Camera3D = Camera3D.new()
	world.add_child(side)
	var target: Vector3 = rider.global_position + Vector3.UP * 0.8
	var right: Vector3 = rider.global_transform.basis.x
	side.global_position = target + right * 3.0 + Vector3.UP * 0.3
	side.look_at(target, Vector3.UP)
	side.current = true
	for frame: int in range(3):
		await create_timer(0.25).timeout
		var image: Image = root.get_texture().get_image()
		image.save_png(out_dir.path_join("side-%d.png" % frame))
		print("saved side-%d" % frame)

	# The HUD editor.
	side.current = false
	var ride_screen: RideScreen = _main.get_node("RideScreen")
	for child: Node in ride_screen.get_children():
		if child is RideSettingsDialog:
			var settings_dialog: RideSettingsDialog = child
			settings_dialog.edit(start.ride_options(), torqa.hud_layout(), false)
			await create_timer(0.5).timeout
			root.get_texture().get_image().save_png(out_dir.path_join("ride-settings.png"))
			print("saved ride settings")
			var tabs: TabContainer = (
				settings_dialog.find_children("*", "TabContainer", true, false)[0]
			)
			tabs.current_tab = tabs.get_tab_count() - 1
			await process_frame
			# Hover a figure over the HUD's grid, so the drop indicator shows.
			var preview: HudPanel = settings_dialog.find_children("*", "HudPanel", true, false)[0]
			var grid: Node = preview.get_child(preview.get_child_count() - 1)
			grid.get_child(2).call(
				"_can_drop_data", Vector2(80, 10), {HudPanel.DRAG_KEY: "power_3s"}
			)
			await create_timer(0.5).timeout
			root.get_texture().get_image().save_png(out_dir.path_join("hud-settings_dialog.png"))
			print("saved hud settings_dialog")
			settings_dialog.hide()

	# The ride's summary (R42), then the history.
	torqa.finish_ride()
	var newest: Dictionary = torqa.history()[0]
	var saved_path: String = newest["path"]
	ride_screen.summary_requested.emit(saved_path)
	await create_timer(1.0).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("summary.png"))
	print("saved summary")
	(_main.get_node("HistoryScreen") as HistoryScreen).closed.emit()
	var start_tabs: TabContainer = start.find_children("*", "TabContainer", true, false)[0]
	start_tabs.current_tab = StartPage.Tab.HISTORY
	await create_timer(1.0).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("history.png"))
	start_tabs.current_tab = StartPage.Tab.COURSES
	await create_timer(0.5).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("start-after-ride.png"))
	var detail: CourseDetail = start.find_children("*", "CourseDetail", true, false)[0]
	detail.back_requested.emit()
	await create_timer(0.5).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("start-gallery.png"))
	for tab: Array in [
		[StartPage.Tab.PROFILE, "start-profile"], [StartPage.Tab.DEVICES, "start-devices"]
	]:
		var index: int = tab[0]
		var file: String = tab[1]
		start_tabs.current_tab = index
		await create_timer(0.5).timeout
		root.get_texture().get_image().save_png(out_dir.path_join(file + ".png"))
	print("saved history")
	await _workout_screens(torqa, start)
	quit(0)


## The Workouts tab (R58) and a heart-rate workout on its own (R56), with its settings.
func _workout_screens(torqa: TorqaApp, start: StartPage) -> void:
	var out_dir: String = OS.get_environment("SCREENSHOT_DIR")
	var start_tabs: TabContainer = start.find_children("*", "TabContainer", true, false)[0]
	start_tabs.current_tab = StartPage.Tab.WORKOUTS
	var tab: WorkoutsTab = start.find_children("*", "WorkoutsTab", true, false)[0]
	var options: WorkoutOptions = tab.find_children("*", "WorkoutOptions", true, false)[0]
	options.set_workout({"kind": "zone", "zone": 3})
	await create_timer(0.5).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("start-workouts.png"))
	var workout: Dictionary = tab.workout()
	_check(torqa.start_workout(workout, false, false), "workout started")
	start.ride_started.emit({"workout": workout, "on_course": false, "difficulty": 50.0})
	await create_timer(maxf(OS.get_environment("SCREENSHOT_WORKOUT_S").to_float(), 20.0)).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("workout.png"))
	var ride_screen: RideScreen = _main.get_node("RideScreen")
	ride_screen.call("_open_settings")
	await create_timer(0.5).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("workout-settings.png"))
	print("saved workout")
	for dialog: Node in ride_screen.find_children("*", "RideSettingsDialog", true, false):
		(dialog as Window).hide()
	# The overlay (R55): the window shrinks to the HUD, on top and see-through.
	ride_screen.overlay_requested.emit(true)
	await create_timer(1.5).timeout
	var window: Window = root
	print(
		(
			"overlay window: %s, borderless %s, on top %s, transparent %s"
			% [window.size, window.borderless, window.always_on_top, window.transparent]
		)
	)
	root.get_texture().get_image().save_png(out_dir.path_join("overlay.png"))
	ride_screen.overlay_requested.emit(false)
	await create_timer(0.5).timeout
	torqa.finish_ride()

	# A structured workout (R21): the plan on the tab, then ridden on its own.
	(_main.get_node("RideScreen") as Control).hide()
	start.show()
	start_tabs.current_tab = StartPage.Tab.WORKOUTS
	options.set_workout({"kind": "plan", "id": "builtin:threshold-2x15"})
	options.changed.emit()
	await create_timer(0.5).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("start-structured.png"))
	var structured: Dictionary = tab.workout()
	_check(torqa.start_workout(structured, false, false), "structured workout started")
	start.ride_started.emit({"workout": structured, "on_course": false, "difficulty": 50.0})
	await create_timer(25.0).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("structured.png"))
	print("saved structured workout")
	torqa.finish_ride()
	# The FTP test (R22) on the tab.
	(_main.get_node("RideScreen") as Control).hide()
	start.show()
	options.set_workout({"kind": "ftp_test"})
	options.changed.emit()
	await create_timer(0.5).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("start-ftp-test.png"))
	# The 20-minute test (#125): its all-out part is a free step.
	options.set_workout({"kind": "ftp_test", "test": "twenty_minutes"})
	options.changed.emit()
	await create_timer(0.5).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("start-ftp-test-20.png"))
	# The workout editor (R21) on a copy of a built-in workout.
	options.set_workout({"kind": "plan", "id": "builtin:threshold-2x15"})
	options.changed.emit()
	var editor: WorkoutEditor = tab.find_children("*", "WorkoutEditor", true, false)[0]
	editor.edit(options.plan(), 250.0)
	await create_timer(0.5).timeout
	root.get_texture().get_image().save_png(out_dir.path_join("workout-editor.png"))
	editor.hide()


func _wait_for(sig: Signal) -> void:
	var received: Array[bool] = [false]
	sig.connect(func(_arg: Variant) -> void: received[0] = true, CONNECT_ONE_SHOT)
	var waited: float = 0.0
	while not received[0] and waited < TIMEOUT_S:
		await process_frame
		waited += root.get_process_delta_time()
	_check(received[0], "signal %s" % sig.get_name())


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("SCREENSHOTS FAILED: " + what)
		quit(1)
