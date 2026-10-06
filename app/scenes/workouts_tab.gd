class_name WorkoutsTab
extends HBoxContainer
## The Workouts tab (R58): a constant power or a heart rate to hold (R56), or a structured
## workout from the library (R21) with its steps shown, ridden on its own (the HUD only) or on
## a course in its 3D world → Start. A course's world is built once Start is pressed, as on the
## course page. Workout files are imported into the library here.

## Start was pressed for `workout` (`WorkoutOptions.workout()` with its `name`), on the course
## at `course_path` or on its own if empty.
signal start_requested(workout: Dictionary, course_path: String)
## The course asked for with `prepare()` is ready: the workout can start.
signal ready_to_start

var _torqa: TorqaApp
var _options: WorkoutOptions = WorkoutOptions.new()
## The structured workout chosen: what it is about and its steps.
var _about: Label = Label.new()
var _plan_chart: WorkoutChart = WorkoutChart.new()
var _import_dialog: FileDialog = FileDialog.new()
var _where: OptionButton = OptionButton.new()
## The course of each entry of `_where` after the first ("on its own").
var _course_paths: PackedStringArray = PackedStringArray()
var _overlay: CheckBox = CheckBox.new()
var _start_button: Button = Button.new()
var _loading_bar: ProgressBar = ProgressBar.new()
var _status: Label = Label.new()
## Waiting for the chosen course's route, or for its world.
var _loading_route: bool = false
var _building: bool = false


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_torqa.route_loaded.connect(_on_route_loaded)
	_torqa.loading_progress.connect(_on_loading_progress)
	_torqa.world_ready.connect(_on_world_ready)
	_torqa.failed.connect(_on_failed)
	refresh()


## Brings the rider's zones and the course list up to date, e.g. after another rider was chosen.
func refresh() -> void:
	var ftp: float = _torqa.profile().get("ftp_w", 200.0)
	_options.configure(_torqa.heart_rate_zones(), ftp)
	_options.set_plans(_torqa.workouts())
	_show_plan()
	var chosen: String = course_path()
	_where.clear()
	_course_paths.clear()
	_where.add_item(tr("On its own (HUD only)"))
	for course: Dictionary in _torqa.courses():
		var located: bool = course.get("located", true)
		if not located:
			continue
		var course_name: String = course["name"]
		_where.add_item(course_name)
		var path: String = course["path"]
		_course_paths.append(path)
	var index: int = _course_paths.find(chosen)
	_where.select(index + 1 if index >= 0 else 0)


## The chosen course's file, or "" to ride on its own.
func course_path() -> String:
	return _course_paths[_where.selected - 1] if _where.selected > 0 else ""


## Whether the workout starts as the overlay (R57), only the HUD over other windows.
func start_as_overlay() -> bool:
	return _overlay.button_pressed


## The workout chosen here, as `start_requested` hands it out: `WorkoutOptions.workout()` with
## its `name` (for the history and on screen) and, for a structured workout, its `plan`
## (`TorqaApp.workouts()` entry).
func workout() -> Dictionary:
	var chosen: Dictionary = _options.workout()
	chosen["name"] = _options.title()
	chosen["plan"] = _options.plan()
	return chosen


## Gets the chosen course's route and world ready; `ready_to_start` follows, at once without
## a course or with its world built already.
func prepare() -> void:
	var path: String = course_path()
	if path.is_empty():
		ready_to_start.emit()
		return
	_start_button.disabled = true
	_status.text = tr("Loading the course") + " …"
	_loading_route = true
	if _torqa.loaded_course() != path:
		_torqa.open_course(path)
	elif _torqa.has_route():
		_build()


## Shows why the workout cannot start (e.g. no trainer); empty clears it.
func show_status(message: String) -> void:
	_status.text = message


func _init() -> void:
	add_theme_constant_override("separation", 24)
	var left: VBoxContainer = VBoxContainer.new()
	left.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	left.add_theme_constant_override("separation", 14)
	var heading: Label = Label.new()
	heading.text = tr("Workout")
	heading.add_theme_font_size_override("font_size", 26)
	left.add_child(heading)
	var intro: Label = Label.new()
	intro.text = tr("Hold a power or a heart rate, or ride a structured workout step by step.")
	intro.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	intro.add_theme_color_override("font_color", UiTheme.MUTED)
	left.add_child(intro)
	var options_panel: PanelContainer = PanelContainer.new()
	var options_rows: VBoxContainer = VBoxContainer.new()
	options_rows.add_theme_constant_override("separation", 12)
	options_rows.add_child(_options)
	_about.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_about.add_theme_color_override("font_color", UiTheme.MUTED)
	# Descriptions are the authors' own.
	_about.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	options_rows.add_child(_about)
	_plan_chart.custom_minimum_size = Vector2(0, 140)
	options_rows.add_child(_plan_chart)
	options_panel.add_child(options_rows)
	left.add_child(options_panel)
	add_child(left)
	_options.changed.connect(_show_plan)
	_options.import_requested.connect(func() -> void: _import_dialog.popup_centered_ratio(0.7))
	_import_dialog.title = tr("Choose a workout file")
	_import_dialog.file_mode = FileDialog.FILE_MODE_OPEN_FILE
	_import_dialog.access = FileDialog.ACCESS_FILESYSTEM
	var patterns: PackedStringArray = PackedStringArray()
	for extension: String in TorqaApp.workout_extensions():
		patterns.append("*." + extension)
	_import_dialog.filters = PackedStringArray([", ".join(patterns) + " ; " + tr("Workouts")])
	_import_dialog.use_native_dialog = true
	_import_dialog.file_selected.connect(_import)
	add_child(_import_dialog)

	var panel: PanelContainer = PanelContainer.new()
	panel.custom_minimum_size = Vector2(520, 0)
	var right: VBoxContainer = VBoxContainer.new()
	right.add_theme_constant_override("separation", 14)
	right.add_child(UiTheme.caption(tr("Where")))
	# Course names are never translated.
	_where.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_where.tooltip_text = tr(
		"On a course, the trainer holds the workout's power while you ride through its 3D world"
	)
	right.add_child(_where)
	_overlay.text = tr("Start as overlay")
	_overlay.tooltip_text = tr("Only the HUD, on top of other windows, e.g. over a video")
	right.add_child(_overlay)
	var fill: Control = Control.new()
	fill.size_flags_vertical = Control.SIZE_EXPAND_FILL
	right.add_child(fill)
	_loading_bar.max_value = 1.0
	_loading_bar.show_percentage = false
	_loading_bar.custom_minimum_size = Vector2(0, 8)
	_loading_bar.hide()
	right.add_child(_loading_bar)
	_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_status.add_theme_color_override("font_color", UiTheme.MUTED)
	right.add_child(_status)
	_start_button.text = tr("Start")
	_start_button.custom_minimum_size = Vector2(0, 56)
	_start_button.add_theme_font_size_override("font_size", 22)
	_start_button.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_start_button.pressed.connect(_on_start_pressed)
	right.add_child(_start_button)
	panel.add_child(right)
	add_child(panel)


## The chosen structured workout's description and steps; nothing for the other kinds.
func _show_plan() -> void:
	var plan: Dictionary = _options.plan()
	_about.visible = not plan.is_empty()
	_plan_chart.visible = not plan.is_empty()
	if plan.is_empty():
		return
	var description: String = plan["description"]
	_about.text = description
	var ftp: float = _torqa.profile().get("ftp_w", 200.0)
	var steps: Array = plan["steps"]
	_plan_chart.set_steps(steps, ftp)


func _import(path: String) -> void:
	var id: String = _torqa.import_workout(path)
	if id.is_empty():
		return
	_options.set_plans(_torqa.workouts(), id)
	_show_plan()
	_status.text = tr("Added to your workouts.")


func _on_start_pressed() -> void:
	_status.text = ""
	start_requested.emit(workout(), course_path())


func _on_route_loaded(_route: Dictionary) -> void:
	if _loading_route and _torqa.loaded_course() == course_path():
		_build()


## The course's route is there: its world next, ridden in 3D even if it has a video (R58).
func _build() -> void:
	_loading_route = false
	_torqa.ride_along_video(false)
	if _torqa.build_world():
		_done()
		return
	_building = true
	_loading_bar.value = 0.0
	_loading_bar.show()
	_status.text = tr("Building 3D world") + " …"


func _on_loading_progress(step: String, _unit: String, done: int, total: int) -> void:
	if not _building:
		return
	_loading_bar.value = float(done) / float(maxi(total, 1))
	_status.text = tr(step) + " …"


func _on_world_ready(_info: Dictionary) -> void:
	if _building:
		_done()


func _done() -> void:
	_building = false
	_loading_bar.hide()
	_status.text = ""
	_start_button.disabled = false
	ready_to_start.emit()


func _on_failed(message: String) -> void:
	if _loading_route or _building:
		_loading_route = false
		_building = false
		_loading_bar.hide()
		_start_button.disabled = false
		_status.text = message
