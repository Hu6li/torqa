extends SceneTree
## Headless checks of the app's window: full screen (#218), and the interface keeping its size
## whatever the window's while the layouts reflow (R94).
## Run: godot --headless --path app -s res://tests/window_smoke.gd

const MAIN_SCENE: String = "res://scenes/main.tscn"

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	await _fullscreen()
	if not "--fullscreen-only" in OS.get_cmdline_user_args():
		await _fixed_scale()
		await _reflow_grid()
		await _gallery_settles()
		var ride: String = await _record_ride()
		for locale: String in ["en", "de"]:
			await _small_window(locale, ride)
	TranslationServer.set_locale("en")
	if not _failed:
		print("WINDOW SMOKE TEST PASSED")
	quit(1 if _failed else 0)


## Full screen (#218, #376): the keys, settings and ride bar switch the app's window and keep
## the choice for the next start; the overlay stays a window of its own.
func _fullscreen() -> void:
	var main: Control = (load(MAIN_SCENE) as PackedScene).instantiate()
	root.add_child(main)
	await process_frame
	var orqwa: OrqwaApp = main.get_node("Orqwa")
	var window: Window = root
	# Headless, the window has no mode of its own: it is checked on a real display.
	var headless: bool = DisplayServer.get_name() == "headless"
	orqwa.set_fullscreen(false)

	_press(KEY_F11)
	_check(orqwa.fullscreen(), "F11 fills the screen, also next time")
	_check(headless or FullScreen.is_on(window), "the window fills the screen")
	if not headless:
		_press(KEY_F11)
		_check(not orqwa.fullscreen() and not FullScreen.is_on(window), "F11 again: a window")
	orqwa.set_fullscreen(false)
	_press(KEY_ENTER, true)
	_check(orqwa.fullscreen(), "Alt+Enter as well")

	await _escape(main, headless)

	var box: CheckBox = _button_with_text(main, "Full screen (F11)")
	_check(box != null, "a check box in the settings")
	if box != null:
		box.button_pressed = true
		_check(orqwa.fullscreen(), "the check box fills the screen")
		box.button_pressed = false
		_check(not orqwa.fullscreen(), "and puts it back in a window")

	var ride: RideScreen = main.get_node("RideScreen")
	# The start page goes as a ride starts: the gallery is not made for the overlay's size.
	(main.get_node("StartPage") as Control).hide()
	ride.begin({"workout": {"kind": "zone", "zone": 3}, "on_course": false})
	ride.show()
	await _ride_fullscreen(main, ride, box, headless)
	var before_overlay: bool = not headless
	FullScreen.set_on(window, orqwa, before_overlay)
	await _settle()
	ride.overlay_requested.emit(true)
	for i: int in range(3):
		await process_frame
	var bar: RideBar = ride.get("_bar")
	_check(not bar.is_visible_in_tree(), "the ride controls stay out of the overlay")
	_check(headless or not FullScreen.is_on(window), "the overlay is a small window")
	_press(KEY_F11)
	_check(orqwa.fullscreen() == before_overlay, "the overlay keeps the full-screen preference")
	ride.overlay_requested.emit(false)
	_check(headless or FullScreen.is_on(window), "leaving the overlay restores full screen")
	await _settle()
	FullScreen.set_on(window, orqwa, false)
	main.free()


## Escape leaves full screen before it goes back (#439); the remote's Back only goes back, so a
## TV by the trainer stays full screen.
func _escape(main: Control, headless: bool) -> void:
	var orqwa: OrqwaApp = main.get_node("Orqwa")
	var start: StartPage = main.get_node("StartPage")
	var detail: Control = start.get("_detail")
	var courses: Control = start.get("_courses")
	FullScreen.set_on(root, orqwa, false)
	courses.hide()
	detail.show()
	_press(KEY_ESCAPE)
	_check(courses.visible and not orqwa.fullscreen(), "in a window, Escape goes back")
	if headless:
		return
	courses.hide()
	detail.show()
	FullScreen.set_on(root, orqwa, true)
	await _settle()
	_press(KEY_ESCAPE)
	await _settle()
	_check(not FullScreen.is_on(root) and not orqwa.fullscreen(), "Escape leaves full screen")
	_check(detail.visible, "leaving full screen stays on the course")
	_press(KEY_ESCAPE)
	_check(courses.visible, "the next Escape goes back")
	courses.hide()
	detail.show()
	FullScreen.set_on(root, orqwa, true)
	await _settle()
	RemoteNavigation.step(root, "back")
	await _settle()
	_check(courses.visible and FullScreen.is_on(root), "the remote's Back stays in full screen")
	FullScreen.set_on(root, orqwa, false)
	await _settle()


## The ride bar switches the app's window in both directions, follows the other controls and
## still works after being folded. Run on a display as well: headless has no window mode.
func _ride_fullscreen(main: Control, ride: RideScreen, box: CheckBox, headless: bool) -> void:
	var orqwa: OrqwaApp = main.get_node("Orqwa")
	var bar: RideBar = ride.get("_bar")
	var button: Button = bar.get("_fullscreen")
	FullScreen.set_on(root, orqwa, false)
	await _settle()
	_check(button.is_visible_in_tree(), "full screen is available during a workout")
	_check(button.tooltip_text == "Full screen (F11)", "the bar offers full screen")
	var expand: Texture2D = button.icon
	button.pressed.emit()
	await _settle()
	_check(orqwa.fullscreen(), "the ride button keeps the computer's full-screen choice")
	if not headless:
		_check(FullScreen.is_on(root), "the ride button fills the app's window")
		_check(
			button.icon != expand and button.tooltip_text == "Exit full screen (Esc or F11)",
			"full screen offers a different icon and a way back to a window"
		)
		var start: StartPage = main.get_node("StartPage")
		var settings: SettingsDialog = start.get("_settings")
		settings.open()
		await _settle()
		_check(box.button_pressed, "opening graphics follows the ride button")
		settings.hide()
		button.pressed.emit()
		await _settle()
		_check(not FullScreen.is_on(root) and not orqwa.fullscreen(), "the ride button exits")
		bar.folded = true
		_press(KEY_F11)
		await _settle()
		bar.folded = false
		_check(button.icon != expand, "unfolded controls follow F11")
		_press(KEY_ENTER, true)
		await _settle()
		_check(button.icon == expand, "the ride icon follows Alt+Enter")
		settings.open()
		await _settle()
		box.button_pressed = true
		await _settle()
		_check(button.icon != expand, "the ride icon follows the graphics setting")
		settings.hide()
		root.mode = Window.MODE_WINDOWED
		await _settle()
		_check(button.icon == expand, "the ride icon follows the system's window controls")
	FullScreen.set_on(root, orqwa, false)
	await _settle()


## The interface keeps the size the system gives it on the screen, whatever the window's (R94):
## a larger window shows more rather than larger text, a smaller one less rather than smaller.
func _fixed_scale() -> void:
	var main: Control = (load(MAIN_SCENE) as PackedScene).instantiate()
	root.add_child(main)
	await process_frame
	var scale: float = root.content_scale_factor
	_check(
		is_equal_approx(scale, UiScale.of_screen(root.current_screen)),
		"as large as the system draws on the screen: %s" % scale
	)
	_check(root.content_scale_mode == Window.CONTENT_SCALE_MODE_DISABLED, "not fit to the window")
	for units: Vector2 in [UiScale.MIN_WINDOW_SIZE, Vector2(1600, 900), Vector2(2560, 1440)]:
		root.size = Vector2i((units * scale).round())
		await process_frame
		var room: Vector2 = root.get_visible_rect().size
		_check(
			is_equal_approx(root.content_scale_factor, scale) and room.is_equal_approx(units),
			"a window of %s units lays out %s, at the same scale" % [units, room]
		)
	_check(
		root.min_size == Vector2i((UiScale.MIN_WINDOW_SIZE * scale).ceil()),
		"no smaller than the layouts are made for: %s" % root.min_size
	)
	main.free()


## As many of a reflowing grid's children side by side as fit its parent (R94), the rest below
## them, and back beside each other once there is room again; an exact fit is too tight.
func _reflow_grid() -> void:
	var parent: VBoxContainer = VBoxContainer.new()
	root.add_child(parent)
	var grid: ReflowGrid = ReflowGrid.new()
	grid.max_columns = 3
	grid.add_theme_constant_override("h_separation", 10)
	parent.add_child(grid)
	for i: int in range(3):
		var cell: Control = Control.new()
		cell.custom_minimum_size = Vector2(100, 20)
		grid.add_child(cell)
	# Three cells side by side need 320, two 210.
	for case: Array in [[400.0, 3], [320.0, 2], [250.0, 2], [150.0, 1], [321.0, 3]]:
		var width: float = case[0]
		var expected: int = case[1]
		# As a container would give it: the parent first grows to the grid's least size.
		for i: int in range(3):
			parent.size = Vector2(width, 200)
			await process_frame
		_check(
			grid.columns == expected and grid.size.x <= width,
			"%d side by side in %s: %d, %s wide" % [expected, width, grid.columns, grid.size.x]
		)
	# A child that expands, e.g. a map, needs room beside the others.
	(grid.get_child(0) as Control).size_flags_horizontal = Control.SIZE_EXPAND_FILL
	grid.expand_room = 100.0
	for i: int in range(3):
		parent.size = Vector2(400, 200)
		await process_frame
	_check(grid.columns == 2, "the map's room counts: %d side by side" % grid.columns)
	parent.free()


## The gallery settles at every width the window passes through, as when macOS animates a
## zoom (R94, R53). Its scroll bar used to take its room from the cards as it came: narrower,
## they fit without it, and so on without end, until the message queue overflowed and the app
## crashed (at 1666 × 1041 with ten courses).
func _gallery_settles() -> void:
	var main: Control = (load(MAIN_SCENE) as PackedScene).instantiate()
	root.add_child(main)
	await process_frame
	var start: StartPage = main.get_node("StartPage")
	var courses: CoursesTab = start.get("_courses")
	var gallery: VBoxContainer = courses.get("_gallery")
	var scroll: ScrollContainer = gallery.get_parent()
	root.size = Vector2i((Vector2(1600, 900) * root.content_scale_factor).round())
	_show_courses(courses, 1)
	await _settle()
	var unscrolled: float = gallery.size.x
	_show_courses(courses, 10)
	await _settle()
	_check(
		scroll.get_v_scroll_bar().visible and is_equal_approx(gallery.size.x, unscrolled),
		(
			"the cards keep their room when the gallery scrolls: %s, not %s"
			% [gallery.size.x, unscrolled]
		)
	)
	var resizes: Array[int] = [0]
	var endless: Array[Vector2i] = []
	# Breaks off a layout that does not settle by keeping the bar, so the check can report
	# it rather than the queue overflowing.
	gallery.resized.connect(
		func() -> void:
			resizes[0] += 1
			if resizes[0] == 100:
				endless.append(root.size)
				scroll.vertical_scroll_mode = ScrollContainer.SCROLL_MODE_SHOW_ALWAYS
	)
	var height: int = roundi(1041 * root.content_scale_factor)
	var widest: int = roundi(1800 * root.content_scale_factor)
	for width: int in range(roundi(UiScale.MIN_WINDOW_SIZE.x * root.content_scale_factor), widest):
		resizes[0] = 0
		root.size = Vector2i(width, height)
		await process_frame
		if not endless.is_empty():
			break
	_check(endless.is_empty(), "the gallery settles at every width: not at %s" % [endless])
	main.free()


## The gallery showing `count` courses: their cards are as tall as those of the library the
## endless layout was seen with.
func _show_courses(courses: CoursesTab, count: int) -> void:
	var gallery: VBoxContainer = courses.get("_gallery")
	for child: Node in gallery.get_children():
		child.free()
	var library: Array[Dictionary] = []
	for i: int in range(count):
		var course: Dictionary = {
			"path": "",
			"name": "Col %d" % i,
			"length_m": 12000.0 + i * 1000.0,
			"elevation_gain_m": 600.0,
			"max_grade": 9.5,
			"estimated_s": 2400.0,
		}
		library.append(course)
	courses.call("_add_group", {"group": "", "courses": library})


## A short ride, so the history, its summary and the analysis have something to lay out.
func _record_ride() -> String:
	var orqwa: OrqwaApp = OrqwaApp.new()
	root.add_child(orqwa)
	var saved: Array[String] = []
	orqwa.ride_saved.connect(func(path: String) -> void: saved.append(path))
	_check(orqwa.connect_fake_trainer(250.0, 90.0), "fake trainer")
	var workout: Dictionary = {"kind": "power", "power": 200.0, "name": "Window smoke test"}
	_check(orqwa.start_workout(workout, false, false), "a ride")
	await create_timer(1.5).timeout
	orqwa.finish_ride()
	orqwa.free()
	_check(saved.size() == 1, "the ride saved: %s" % [saved])
	return saved[0] if saved.size() == 1 else ""


## Every page and dialog fits the smallest window the layouts are made for (R94, R53), in
## English and in German, whose words run longer: nothing reaches past the window's edges, and
## what is taller than the window scrolls.
func _small_window(locale: String, ride: String) -> void:
	TranslationServer.set_locale(locale)
	var settings: OrqwaApp = OrqwaApp.new()
	root.add_child(settings)
	var rider: Dictionary = settings.profile()
	var rider_id: String = rider["id"]
	var language: String = rider.get("language", "")
	rider["language"] = locale
	settings.save_profile(rider_id, rider)
	settings.free()
	var main: Control = (load(MAIN_SCENE) as PackedScene).instantiate()
	root.add_child(main)
	await process_frame
	root.size = Vector2i((UiScale.MIN_WINDOW_SIZE * root.content_scale_factor).ceil())
	var room: Vector2 = root.get_visible_rect().size
	var orqwa: OrqwaApp = main.get_node("Orqwa")
	var start: StartPage = main.get_node("StartPage")
	# The gallery's toolbar as with courses in it, and the rider's whole setup.
	var courses: CoursesTab = start.get("_courses")
	var toolbar: Control = courses.get("_toolbar")
	toolbar.show()
	var profile: ProfileTab = start.get("_profile")
	var unfolded: Dictionary[String, bool] = profile.get("_unfolded")
	unfolded[rider_id] = true
	profile.refresh()
	var tabs: TabContainer = start.get("_tabs")
	for tab: int in range(tabs.get_tab_count()):
		tabs.current_tab = tab
		await _settle()
		_fits(start, room, "%s, the %s tab" % [locale, tabs.get_tab_control(tab).name])
	tabs.current_tab = StartPage.Tab.COURSES
	(
		start
		. call(
			"_open_course",
			{
				"path": "",
				"name": "Gurtenstrasse",
				"length_m": 2400.0,
				"elevation_gain_m": 149.4,
				"max_grade": 17.6,
			}
		)
	)
	await _settle()
	_fits(start, room, "%s, a course's page" % locale)
	await _course_building(start, room, locale)
	var settings_dialog: SettingsDialog = start.get("_settings")
	settings_dialog.open()
	await _dialog_fits(settings_dialog, room, locale)
	var rider_dialog: ProfileDialog = start.find_children("*", "ProfileDialog", true, false)[0]
	rider_dialog.edit(orqwa.profile(), orqwa.hud_layout())
	await _dialog_fits(rider_dialog, room, locale)
	var editor: WorkoutEditor = start.find_children("*", "WorkoutEditor", true, false)[0]
	editor.edit({}, 200.0)
	await _dialog_fits(editor, room, locale)
	var buttons: ButtonDialog = start.find_children("*", "ButtonDialog", true, false)[0]
	buttons.open(ButtonDialog.PHONE_REMOTE, OrqwaApp.phone_remote_name())
	await _dialog_fits(buttons, room, locale)
	start.hide()
	var summary: HistoryScreen = main.get_node("HistoryScreen")
	if not ride.is_empty():
		main.call("_show_quick_summary", ride)
		var quick_summary: RideSummaryDialog = main.get("_quick_summary")
		await _dialog_fits(quick_summary, room, locale)
		# All five record durations must fit, including the longer German captions.
		var record_ride: Dictionary = {}
		for entry: Dictionary in orqwa.history():
			if entry["path"] == ride:
				record_ride = entry.duplicate(true)
		record_ride["power_records"] = PackedFloat64Array([5.0, 60.0, 300.0, 1200.0, 3600.0])
		record_ride["power_curve"] = PackedVector2Array(
			[
				Vector2(5, 900),
				Vector2(60, 500),
				Vector2(300, 350),
				Vector2(1200, 280),
				Vector2(3600, 250)
			]
		)
		quick_summary.call("_show_ride", record_ride)
		await _dialog_fits(quick_summary, room, locale)
		main.call("_show_summary", ride)
		await _settle()
		_fits(summary, room, "%s, a ride's summary" % locale)
	summary.hide()
	var ride_screen: RideScreen = main.get_node("RideScreen")
	ride_screen.begin({"workout": {"kind": "zone", "zone": 3}, "on_course": false})
	ride_screen.show()
	await _settle()
	_fits(ride_screen, room, "%s, the ride's screen" % locale)
	ride_screen.call("_open_settings")
	var ride_settings: RideSettingsDialog = (
		ride_screen.find_children("*", "RideSettingsDialog", true, false)[0]
	)
	await _dialog_fits(ride_settings, room, locale)
	main.free()
	rider["language"] = language
	settings = OrqwaApp.new()
	root.add_child(settings)
	settings.save_profile(rider_id, rider)
	settings.free()


## A growing build log must not push Ride or the climbs past a tablet's screen (R53, R94).
func _course_building(start: StartPage, room: Vector2, locale: String) -> void:
	var course: CourseDetail = start.get("_detail")
	var records: Label = course.get("_records")
	var climbs: PackedStringArray = PackedStringArray()
	for i: int in range(20):
		climbs.append("Cat 1 %.1f km at 8.0 %%" % (i + 1.0))
	records.text = "\n".join(climbs)
	course.call("_show_building")
	var log: PreparationLog = course.get("_log")
	for i: int in range(12):
		log.step("Build step %d" % i, "tiles", 143, 412)
	await _settle()
	_fits(start, room, "%s, a course while building its world" % locale)
	var ride: Button = course.get("_ride_button")
	_check(
		Rect2(Vector2.ZERO, room).encloses(ride.get_global_rect()),
		"%s, Ride stays inside the screen while building" % locale
	)
	await _scroll_to_bottom(records, "%s, the last climb is reachable" % locale)
	await _scroll_to_bottom(log, "%s, the latest build step is reachable" % locale)
	log.finish()
	await _settle()
	_fits(start, room, "%s, a course after building its world" % locale)


## The bottom of tall content can be brought into its scroll area's visible height.
func _scroll_to_bottom(content: Control, what: String) -> void:
	var ancestor: Node = content.get_parent()
	while ancestor != null and not ancestor is ScrollContainer:
		ancestor = ancestor.get_parent()
	var scroll: ScrollContainer = ancestor as ScrollContainer
	_check(scroll != null, what + ": can scroll")
	if scroll == null:
		return
	scroll.scroll_vertical = ceili(scroll.get_v_scroll_bar().max_value)
	await _settle()
	var bottom: float = content.get_global_rect().end.y
	var viewport: Rect2 = scroll.get_global_rect()
	var screen_bottom: float = root.get_visible_rect().end.y
	_check(bottom > viewport.position.y and bottom <= minf(viewport.end.y, screen_bottom), what)


## Whether `page` and what is in it need no more room than `room`.
func _fits(page: Control, room: Vector2, what: String) -> void:
	var need: Vector2 = _need(page)
	_check(need.x <= room.x and need.y <= room.y, "%s fits %s: needs %s" % [what, room, need])


## The room a control needs: a container's own least size, or what its children need.
func _need(control: Control) -> Vector2:
	if control is Container:
		return control.get_combined_minimum_size()
	var need: Vector2 = control.get_combined_minimum_size()
	for child: Node in control.get_children():
		if child is Control and (child as Control).visible:
			need = need.max(_need(child as Control))
	return need


## Whether `dialog`, just opened, fits `room` with its title and lies inside it; closes it.
func _dialog_fits(dialog: Window, room: Vector2, locale: String) -> void:
	await _settle()
	var what: String = "%s, %s" % [locale, dialog.title]
	var title: float = dialog.get_theme_constant("title_height")
	var need: Vector2 = Vector2(dialog.min_size).max(dialog.get_contents_minimum_size())
	_check(
		dialog.visible and need.x <= room.x and need.y + title <= room.y,
		"%s fits %s: needs %s" % [what, room, need]
	)
	var frame: Rect2 = Rect2(
		Vector2(dialog.position) - Vector2(0, title), Vector2(dialog.size) + Vector2(0, title)
	)
	_check(Rect2(Vector2.ZERO, room).encloses(frame), "%s inside the window: %s" % [what, frame])
	dialog.hide()


func _settle() -> void:
	for i: int in range(4):
		await process_frame


func _press(key: Key, alt: bool = false) -> void:
	for pressed: bool in [true, false]:
		var event: InputEventKey = InputEventKey.new()
		event.keycode = key
		event.alt_pressed = alt
		event.pressed = pressed
		root.push_input(event)


## The first button under `node` showing `text`.
func _button_with_text(node: Node, text: String) -> Button:
	for button: Node in node.find_children("*", "Button", true, false):
		if (button as Button).text == text:
			return button
	return null


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("WINDOW SMOKE TEST FAILED: " + what)
		_failed = true
