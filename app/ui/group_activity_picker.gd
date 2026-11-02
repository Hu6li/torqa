class_name GroupActivityPicker
extends VBoxContainer
## The group activity's presentation, using the same previews as the solo ride pages.

signal chosen(course: String, workout: String)

## Extra shared ride options, presented before confirming the activity.
var options: VBoxContainer = VBoxContainer.new()

var _orqwa: OrqwaApp
var _previews: BoxContainer = BoxContainer.new()
var _course_preview: VBoxContainer = VBoxContainer.new()
var _workout_preview: VBoxContainer = VBoxContainer.new()
var _course: OptionButton = OptionButton.new()
var _workout: OptionButton = OptionButton.new()
var _map: PathCard = PathCard.new()
var _elevation: ElevationProfile = ElevationProfile.new()
var _figures: HFlowContainer = HFlowContainer.new()
var _chart: WorkoutChart = WorkoutChart.new()
var _description: Label = Label.new()
var _duration: Label = Label.new()
var _hint: Label = Label.new()
var _apply: Button = Button.new()
var _courses: Array = []
var _workouts: Array = []
var _editable: bool = true
var _active_course: String = ""
var _active_workout: String = ""


func _ready() -> void:
	add_theme_constant_override("separation", 12)
	var title: Label = Label.new()
	title.text = tr("What are we riding?")
	title.add_theme_font_size_override("font_size", 22)
	add_child(title)
	_hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_hint.add_theme_color_override("font_color", UiTheme.MUTED)
	_hint.text = tr("Choose a course, a workout, or both. Everyone gets a copy automatically.")
	add_child(_hint)
	_previews.add_theme_constant_override("separation", 24)
	for column: VBoxContainer in [_course_preview, _workout_preview]:
		column.add_theme_constant_override("separation", 12)
		column.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		_previews.add_child(column)
	add_child(_previews)
	_course_preview.add_child(UiTheme.caption(tr("Course")))
	UiTheme.narrowable(_course)
	_course.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_course.item_selected.connect(func(_index: int) -> void: _preview())
	_course_preview.add_child(_course)
	_map.custom_minimum_size.y = 170
	_map.resized.connect(_fit_map)
	_course_preview.add_child(_map)
	_elevation.custom_minimum_size.y = 55
	_course_preview.add_child(_elevation)
	_figures.add_theme_constant_override("h_separation", 24)
	_course_preview.add_child(_figures)
	_workout_preview.add_child(UiTheme.caption(tr("Workout")))
	UiTheme.narrowable(_workout)
	_workout.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_workout.item_selected.connect(func(_index: int) -> void: _preview())
	_workout_preview.add_child(_workout)
	_chart.custom_minimum_size.y = 170
	_workout_preview.add_child(_chart)
	_duration.add_theme_color_override("font_color", UiTheme.MUTED)
	_duration.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_workout_preview.add_child(_duration)
	_description.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_description.add_theme_color_override("font_color", UiTheme.MUTED)
	_workout_preview.add_child(_description)
	options.add_theme_constant_override("separation", 8)
	add_child(options)
	_apply.text = tr("Use this activity")
	_apply.custom_minimum_size.y = 44
	_apply.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_apply.pressed.connect(func() -> void: chosen.emit(course(), workout()))
	add_child(_apply)
	resized.connect(_reflow)
	_reflow()
	refresh()


func _reflow() -> void:
	_previews.vertical = size.x < 460


func _fit_map() -> void:
	_map.custom_minimum_size.y = _map.size.x * CourseCard.MAP_ASPECT


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	if is_node_ready():
		refresh()


func course() -> String:
	return "" if _course.selected < 0 else _course.get_selected_metadata()


func workout() -> String:
	return "" if _workout.selected < 0 else _workout.get_selected_metadata()


func refresh() -> void:
	if _orqwa == null or not is_node_ready():
		return
	var previous_course: String = course()
	var previous_workout: String = workout()
	_courses = _orqwa.courses()
	_workouts = _orqwa.workouts()
	_course.clear()
	_course.add_item(tr("No course · workout only"))
	_course.set_item_metadata(0, "")
	for entry: Dictionary in _courses:
		var video: String = entry.get("video", "")
		if not video.is_empty():
			continue
		var course_name: String = entry["name"]
		_course.add_item(course_name)
		_course.set_item_metadata(_course.item_count - 1, entry["path"])
	_workout.clear()
	_workout.add_item(tr("Free ride · no workout"))
	_workout.set_item_metadata(0, "")
	for entry: Dictionary in _workouts:
		var workout_name: String = entry["name"]
		_workout.add_item(workout_name)
		_workout.set_item_metadata(_workout.item_count - 1, entry["id"])
	_select(_course, previous_course)
	_select(_workout, previous_workout)
	_preview()


## Locks a received selection; host drafts remain untouched by polling.
func show_selection(editable: bool, course_path: String, workout_id: String) -> void:
	_active_course = course_path
	_active_workout = workout_id
	if editable != _editable or not editable:
		_select(_course, course_path)
		_select(_workout, workout_id)
	_editable = editable
	_course.disabled = not editable
	_workout.disabled = not editable
	_apply.visible = editable
	_hint.text = (
		tr("Choose a course, a workout, or both. Everyone gets a copy automatically.")
		if editable
		else tr("Chosen by the organizer. Your copy downloads automatically.")
	)
	_preview()


func _select(picker: OptionButton, value: String) -> void:
	picker.select(0)
	for index: int in range(picker.item_count):
		if picker.get_item_metadata(index) == value:
			picker.select(index)
			return


func _preview() -> void:
	if _orqwa == null:
		return
	for child: Node in _figures.get_children():
		_figures.remove_child(child)
		child.queue_free()
	_map.hide()
	_elevation.hide()
	_figures.hide()
	for entry: Dictionary in _courses:
		if entry["path"] != course() or course().is_empty():
			continue
		var track: PackedVector2Array = entry.get("track", PackedVector2Array())
		var profile: PackedVector2Array = entry.get("profile", PackedVector2Array())
		_map.set_track(track)
		_map.set_map(_orqwa.course_preview(course()))
		_map.show()
		_elevation.set_profile(profile)
		_elevation.visible = profile.size() >= 2
		_figures.show()
		var imperial: bool = _orqwa.profile().get("units", "metric") == "imperial"
		for figure: PackedStringArray in CourseCard.figures(entry, imperial):
			var label: Label = Label.new()
			label.text = tr(figure[0]) + "  " + figure[1]
			_figures.add_child(label)
	_chart.hide()
	_description.hide()
	_duration.hide()
	for entry: Dictionary in _workouts:
		if entry["id"] != workout() or workout().is_empty():
			continue
		var steps: Array = entry["steps"]
		var ftp: float = _orqwa.profile().get("ftp_w", 200.0)
		var seconds: float = entry["duration_s"]
		_chart.set_steps(steps, ftp)
		_chart.show()
		_duration.text = tr("%s · power based on your FTP") % UiTheme.duration(seconds)
		_duration.show()
		_description.text = entry["description"]
		_description.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		_description.visible = not _description.text.is_empty()
	_apply.disabled = (
		(course().is_empty() and workout().is_empty())
		or (course() == _active_course and workout() == _active_workout)
	)
	_apply.text = (
		tr("Shared with everyone")
		if _apply.disabled and (not course().is_empty() or not workout().is_empty())
		else tr("Use this activity")
	)
