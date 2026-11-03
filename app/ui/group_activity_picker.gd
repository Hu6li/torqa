class_name GroupActivityPicker
extends VBoxContainer
# gdlint: disable=max-line-length
## The group activity's presentation, using the same previews as the solo ride pages. Only the
## organizer chooses; friends see who is choosing, then what they look at or chose (R196).

signal chosen(course: String, workout: String)
## The organizer changed what the picker shows; friends see it as a preview (#418).
signal browsed(course: String, workout: String)

## Extra shared ride options, presented before confirming the activity.
var options: VBoxContainer = VBoxContainer.new()

var _orqwa: OrqwaApp
var _previews: BoxContainer = BoxContainer.new()
var _course_preview: VBoxContainer = VBoxContainer.new()
var _workout_preview: VBoxContainer = VBoxContainer.new()
var _course: OptionButton = OptionButton.new()
var _workout: OptionButton = OptionButton.new()
var _course_caption: Label
var _workout_caption: Label
var _course_name: Label = Label.new()
var _workout_name: Label = Label.new()
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
## Countdown locks the organizer's choices while keeping their preview columns in place.
var _show_choices: bool = true
var _active_course: String = ""
var _active_workout: String = ""
## What the organizer is choosing, for a friend (#418): shown instead of the shared activity.
var _shown: Dictionary = {}
var _organizer: String = ""
## The shared activity's name, shown to a friend until their copy is ready.
var _selection: String = ""
## Friends wait here while the organizer has neither chosen nor started looking.
var _waiting: VBoxContainer = VBoxContainer.new()
var _waiting_text: Label = Label.new()


func _ready() -> void:
	add_theme_constant_override("separation", 12)
	var title: Label = Label.new()
	title.text = tr("What are we riding?")
	title.add_theme_font_size_override("font_size", 22)
	add_child(title)
	_hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_hint.add_theme_color_override("font_color", UiTheme.MUTED)
	_hint.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	add_child(_hint)
	_waiting.alignment = BoxContainer.ALIGNMENT_CENTER
	_waiting.add_theme_constant_override("separation", 18)
	_waiting.custom_minimum_size.y = 420
	var spinner: Spinner = Spinner.new(34.0)
	spinner.size_flags_horizontal = Control.SIZE_SHRINK_CENTER
	_waiting.add_child(spinner)
	_waiting_text.add_theme_font_size_override("font_size", 19)
	_waiting_text.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_waiting_text.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_waiting_text.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_waiting.add_child(_waiting_text)
	_waiting.hide()
	add_child(_waiting)
	_previews.add_theme_constant_override("separation", 24)
	for column: VBoxContainer in [_course_preview, _workout_preview]:
		column.add_theme_constant_override("separation", 12)
		column.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		_previews.add_child(column)
	add_child(_previews)
	_course_caption = UiTheme.caption(tr("Course"))
	_course_preview.add_child(_course_caption)
	UiTheme.narrowable(_course)
	_course.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_course.item_selected.connect(func(_index: int) -> void: _browse())
	_course_preview.add_child(_course)
	_course_preview.add_child(_shown_name(_course_name))
	_map.custom_minimum_size.y = 170
	_map.resized.connect(_fit_map)
	_course_preview.add_child(_map)
	_elevation.custom_minimum_size.y = 55
	_course_preview.add_child(_elevation)
	_figures.add_theme_constant_override("h_separation", 24)
	_course_preview.add_child(_figures)
	_workout_caption = UiTheme.caption(tr("Workout"))
	_workout_preview.add_child(_workout_caption)
	UiTheme.narrowable(_workout)
	_workout.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_workout.item_selected.connect(func(_index: int) -> void: _browse())
	_workout_preview.add_child(_workout)
	_workout_preview.add_child(_shown_name(_workout_name))
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
	_apply.text = tr("Share with the group")
	_apply.custom_minimum_size.y = 44
	_apply.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_apply.pressed.connect(func() -> void: chosen.emit(course(), workout()))
	add_child(_apply)
	resized.connect(_reflow)
	_reflow()
	refresh()


func _reflow() -> void:
	_previews.vertical = size.x < 460


## A previewed course's or workout's name, where the organizer has their picker.
func _shown_name(label: Label) -> Label:
	label.add_theme_font_size_override("font_size", 17)
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	label.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	label.hide()
	return label


func _browse() -> void:
	_preview()
	if _editable:
		browsed.emit(course(), workout())


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


## Friends see names; the organizer keeps their pickers, disabled while `locked`.
## Unlocked organizer drafts remain untouched by polling.
func show_selection(
	organizer: bool, course_path: String, workout_id: String, locked: bool = false
) -> void:
	_active_course = course_path
	_active_workout = workout_id
	if organizer != _show_choices or not organizer or locked:
		_select(_course, course_path)
		_select(_workout, workout_id)
	_show_choices = organizer
	_editable = organizer and not locked
	_course.disabled = not _editable
	_workout.disabled = not _editable
	_apply.visible = organizer
	_preview()


## Names the organizer, who friends see choosing and having chosen, and the activity they
## shared (`selection`), shown until a friend's copy is ready.
func set_organizer(organizer: String, selection: String) -> void:
	if organizer == _organizer and selection == _selection:
		return
	_organizer = organizer
	_selection = selection
	_preview()


## Shows what the organizer is choosing (`OrqwaApp.group_preview_data()`) instead of the
## shared activity; an empty `preview` shows that again.
func show_preview(preview: Dictionary) -> void:
	_shown = preview
	_preview()


## Whether a friend sees what the organizer is choosing.
func previewing() -> bool:
	return not _shown.is_empty()


func _select(picker: OptionButton, value: String) -> void:
	picker.select(0)
	for index: int in range(picker.item_count):
		if picker.get_item_metadata(index) == value:
			picker.select(index)
			return


func _preview() -> void:
	if _orqwa == null:
		return
	var previewing: bool = not _shown.is_empty()
	var shown_course: Dictionary = _shown.get("course", {})
	var shown_workout: Dictionary = _shown.get("workout", {})
	_course_caption.text = tr("Course preview") if previewing else tr("Course")
	_workout_caption.text = tr("Workout preview") if previewing else tr("Workout")
	# Friends see names, never the organizer's pickers (R196).
	var chosen: bool = not _active_course.is_empty() or not _active_workout.is_empty()
	var waiting: bool = not _show_choices and not previewing and not chosen
	_waiting.visible = waiting
	_waiting_text.text = tr("%s is choosing the ride") % _organizer
	_previews.visible = not waiting
	options.visible = not waiting
	_hint.text = tr("Chosen by %s") % _organizer
	_hint.visible = not _show_choices and not previewing and chosen
	_course.visible = _show_choices and not previewing
	_workout.visible = _show_choices and not previewing
	_course_preview.visible = (
		(_show_choices and not previewing)
		or (previewing and not shown_course.is_empty())
		or (not previewing and not _active_course.is_empty())
	)
	_workout_preview.visible = (
		(_show_choices and not previewing)
		or (previewing and not shown_workout.is_empty())
		or (not previewing and not _active_workout.is_empty())
	)
	for child: Node in _figures.get_children():
		_figures.remove_child(child)
		child.queue_free()
	_map.hide()
	_elevation.hide()
	_figures.hide()
	var entry: Dictionary = shown_course if previewing else _library_course()
	_course_name.text = entry.get("name", "")
	_course_name.visible = not _show_choices or previewing
	if not entry.is_empty():
		var track: PackedVector2Array = entry.get("track", PackedVector2Array())
		var profile: PackedVector2Array = entry.get("profile", PackedVector2Array())
		var picture: PackedByteArray = entry.get("picture", PackedByteArray())
		if not previewing:
			picture = _orqwa.course_preview(course())
		_map.set_track(track)
		_map.set_map(picture)
		_map.show()
		_elevation.set_profile(profile)
		_elevation.visible = profile.size() >= 2
		_figures.show()
		var imperial: bool = _orqwa.profile().get("units", "metric") == "imperial"
		for figure: PackedStringArray in CourseCard.figures(entry, imperial):
			var label: Label = Label.new()
			label.text = tr(figure[0]) + "  " + figure[1]
			_figures.add_child(label)
	if (
		chosen
		and not _show_choices
		and not previewing
		and entry.is_empty()
		and _library_workout().is_empty()
	):
		# Still on its way: its name is all there is to show yet.
		_course_name.text = _selection
		_course_preview.visible = true
	_chart.hide()
	_description.hide()
	_duration.hide()
	var plan: Dictionary = shown_workout if previewing else _library_workout()
	_workout_name.text = plan.get("name", "")
	_workout_name.visible = not _show_choices or previewing
	if not plan.is_empty():
		var steps: Array = plan["steps"]
		var ftp: float = _orqwa.profile().get("ftp_w", 200.0)
		var seconds: float = plan["duration_s"]
		_chart.set_steps(steps, ftp)
		_chart.show()
		_duration.text = tr("%s · power based on your FTP") % UiTheme.duration(seconds)
		_duration.show()
		_description.text = plan["description"]
		_description.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		_description.visible = not _description.text.is_empty()
	_apply.disabled = (
		not _editable
		or (course().is_empty() and workout().is_empty())
		or (course() == _active_course and workout() == _active_workout)
	)
	_apply.text = (
		tr("Shared with the group")
		if _apply.disabled and (not course().is_empty() or not workout().is_empty())
		else tr("Share with the group")
	)


## The library's entry for the chosen course, empty for none.
func _library_course() -> Dictionary:
	for entry: Dictionary in _courses:
		if not course().is_empty() and entry["path"] == course():
			return entry
	return {}


## The library's entry for the chosen workout, empty for none.
func _library_workout() -> Dictionary:
	for entry: Dictionary in _workouts:
		if not workout().is_empty() and entry["id"] == workout():
			return entry
	return {}
