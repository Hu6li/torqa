class_name VideoAlignDialog
extends ConfirmationDialog
## Places a video without GPS on its route (R17) with sync points: each pairs a position on the
## route with the moment of the video showing it. The route's start and end are always there;
## points in between help where the footage stops or changes speed. Between neighbouring
## points the video follows the rider's distance evenly.

## The rider confirmed: `marks` (x metres along the route, y seconds into the video), from the
## route's start to its end.
signal aligned(marks: PackedVector2Array)

## The preview's largest and smallest size; in between it takes what the window leaves (#50).
const PREVIEW_SIZE: Vector2 = Vector2(560, 315)
const MIN_PREVIEW_WIDTH: float = 200.0
## Room taken beside and below the preview: the point list, margins, the controls under the
## video, the route profile and the buttons.
const BESIDE_PREVIEW: float = 330.0
const BELOW_PREVIEW: float = 400.0
## Fine steps, in seconds, for the buttons under the video.
const STEPS: Array[float] = [-1.0, -0.1, 0.1, 1.0]

var _torqa: TorqaApp
var _video: String = ""
var _length_m: float = 0.0
var _imperial: bool = false
var _marks: PackedVector2Array = PackedVector2Array()
var _selected: int = 0
var _list: VBoxContainer = VBoxContainer.new()
var _remove_button: Button = Button.new()
var _picture: TextureRect = TextureRect.new()
var _time_slider: HSlider = HSlider.new()
var _time_label: Label = Label.new()
var _profile: ElevationProfile = ElevationProfile.new()
var _distance_slider: HSlider = HSlider.new()
var _distance_label: Label = Label.new()
var _problem: Label = Label.new()
var _hint: Label = Label.new()
## Decoding a frame per slider step would lag behind the drag; the newest one wins.
var _refresh: Timer = Timer.new()


## Opens the dialog for the video at `video` (`duration_s` long) on a route `length_m` long
## with elevation `profile` (distance, elevation); `marks` as `aligned` gives them, or empty
## for the whole video over the whole route.
func edit(
	torqa: TorqaApp,
	video: String,
	duration_s: float,
	length_m: float,
	profile: PackedVector2Array,
	marks: PackedVector2Array,
	imperial: bool = false
) -> void:
	_torqa = torqa
	_video = video
	_length_m = length_m
	_imperial = imperial
	_marks = marks.duplicate()
	if _marks.size() < 2:
		_marks = PackedVector2Array([Vector2(0.0, 0.0), Vector2(length_m, duration_s)])
	_marks[0].x = 0.0
	_marks[_marks.size() - 1].x = length_m
	_time_slider.max_value = duration_s
	_distance_slider.max_value = length_m
	_profile.set_profile(profile)
	_select(0)
	_fit()
	popup_centered()


## The marks as set: x metres along the route, y seconds into the video.
func marks() -> PackedVector2Array:
	return _marks.duplicate()


## Selects mark `index` for editing.
func select(index: int) -> void:
	_select(index)


## Adds a point after the selected mark (before it for the end), halfway to its neighbour.
func add_point() -> void:
	var at: int = mini(_selected + 1, _marks.size() - 1)
	_marks.insert(at, (_marks[at - 1] + _marks[at]) / 2.0)
	_select(at)


## Removes the selected point; the start and the end stay.
func remove_point() -> void:
	if _selected > 0 and _selected < _marks.size() - 1:
		_marks.remove_at(_selected)
		_select(_selected - 1)


## Whether the marks follow each other along the route and in the video.
func valid() -> bool:
	for i: int in range(1, _marks.size()):
		if _marks[i].x <= _marks[i - 1].x or _marks[i].y <= _marks[i - 1].y:
			return false
	return true


func _init() -> void:
	title = tr("Align video with route")
	ok_button_text = tr("Align")
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 14)
	var hint: Label = _hint
	var sentences: PackedStringArray = [
		tr("Pair moments of the video with places on the route."),
		tr("Add points where the footage stops or changes speed."),
		tr("In between, the video follows your distance evenly."),
	]
	hint.text = " ".join(sentences)
	hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	hint.custom_minimum_size = Vector2(PREVIEW_SIZE.x + BESIDE_PREVIEW - 30.0, 0)
	rows.add_child(hint)
	var columns: HBoxContainer = HBoxContainer.new()
	columns.add_theme_constant_override("separation", 24)
	columns.add_child(_build_list())
	columns.add_child(_build_editor())
	rows.add_child(columns)
	_problem.text = tr("Points must follow each other on the route and in the video.")
	_problem.add_theme_color_override("font_color", Color(1, 0.55, 0.45))
	_problem.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	rows.add_child(_problem)
	add_child(rows)
	_refresh.one_shot = true
	_refresh.wait_time = 0.08
	_refresh.timeout.connect(_show_frame)
	add_child(_refresh)
	confirmed.connect(func() -> void: aligned.emit(marks()))
	visibility_changed.connect(_on_visibility_changed)


func _ready() -> void:
	get_tree().root.size_changed.connect(_on_window_resized)


## Sizes the preview to the app window so that the whole dialog, buttons included, fits.
func _fit() -> void:
	var room: Vector2 = Vector2(get_tree().root.size) * 0.92
	var width: float = clampf(room.x - BESIDE_PREVIEW, MIN_PREVIEW_WIDTH, PREVIEW_SIZE.x)
	var height: float = minf(width * 9.0 / 16.0, room.y - BELOW_PREVIEW)
	height = maxf(height, MIN_PREVIEW_WIDTH * 9.0 / 16.0)
	width = minf(width, height * 16.0 / 9.0)
	_picture.custom_minimum_size = Vector2(width, height)
	_profile.custom_minimum_size = Vector2(width, 80)
	_hint.custom_minimum_size = Vector2(width + BESIDE_PREVIEW - 30.0, 0)
	# Shrink back as well as grow.
	reset_size()


func _on_window_resized() -> void:
	if visible:
		_fit()
		popup_centered()


func _build_list() -> Control:
	var column: VBoxContainer = VBoxContainer.new()
	column.custom_minimum_size = Vector2(260, 0)
	column.add_theme_constant_override("separation", 8)
	column.add_child(UiTheme.caption(tr("Sync points")))
	_list.add_theme_constant_override("separation", 4)
	column.add_child(_list)
	var buttons: HBoxContainer = HBoxContainer.new()
	var add: Button = Button.new()
	add.text = tr("Add point")
	add.pressed.connect(add_point)
	buttons.add_child(add)
	_remove_button.text = tr("Remove point")
	_remove_button.pressed.connect(remove_point)
	buttons.add_child(_remove_button)
	column.add_child(buttons)
	return column


func _build_editor() -> Control:
	var column: VBoxContainer = VBoxContainer.new()
	column.add_theme_constant_override("separation", 8)
	_picture.custom_minimum_size = PREVIEW_SIZE
	_picture.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	_picture.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	column.add_child(_picture)
	_time_slider.step = 0.01
	_time_slider.value_changed.connect(_on_time_changed)
	column.add_child(_time_slider)
	var steps: HBoxContainer = HBoxContainer.new()
	_time_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_time_label.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	steps.add_child(_time_label)
	for step: float in STEPS:
		var button: Button = Button.new()
		button.text = "%+.1f s" % step if absf(step) < 1.0 else "%+d s" % roundi(step)
		button.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		button.pressed.connect(func() -> void: _time_slider.value += step)
		steps.add_child(button)
	column.add_child(steps)
	column.add_child(UiTheme.caption(tr("Position on the route")))
	_profile.custom_minimum_size = Vector2(PREVIEW_SIZE.x, 80)
	column.add_child(_profile)
	var distance_row: HBoxContainer = HBoxContainer.new()
	_distance_slider.step = 1.0
	_distance_slider.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_distance_slider.value_changed.connect(_on_distance_changed)
	distance_row.add_child(_distance_slider)
	_distance_label.custom_minimum_size = Vector2(90, 0)
	_distance_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	_distance_label.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	distance_row.add_child(_distance_label)
	column.add_child(distance_row)
	return column


func _select(index: int) -> void:
	_selected = clampi(index, 0, _marks.size() - 1)
	var mark: Vector2 = _marks[_selected]
	_time_slider.set_value_no_signal(mark.y)
	_distance_slider.set_value_no_signal(mark.x)
	# The start and the end are the route's ends; only points in between move along it.
	var middle: bool = _selected > 0 and _selected < _marks.size() - 1
	_distance_slider.editable = middle
	_remove_button.disabled = not middle
	_changed()


func _on_time_changed(value: float) -> void:
	_marks[_selected].y = value
	_changed()


func _on_distance_changed(value: float) -> void:
	_marks[_selected].x = value
	_changed()


func _changed() -> void:
	var mark: Vector2 = _marks[_selected]
	_time_label.text = _time_text(mark.y)
	_distance_label.text = _distance_text(mark.x)
	_profile.set_rider_distance(mark.x)
	_rebuild_list()
	var ok: bool = valid()
	get_ok_button().disabled = not ok
	_problem.visible = not ok
	if is_inside_tree():
		_refresh.start()


func _rebuild_list() -> void:
	for child: Node in _list.get_children():
		_list.remove_child(child)
		child.queue_free()
	for i: int in range(_marks.size()):
		var name: String = tr("Point")
		if i == 0:
			name = tr("Start")
		elif i == _marks.size() - 1:
			name = tr("End")
		var button: Button = Button.new()
		button.text = "%s · %s · %s" % [name, _distance_text(_marks[i].x), _time_text(_marks[i].y)]
		button.alignment = HORIZONTAL_ALIGNMENT_LEFT
		button.toggle_mode = true
		button.button_pressed = i == _selected
		button.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		button.pressed.connect(_select.bind(i))
		_list.add_child(button)


func _show_frame() -> void:
	if _torqa == null or _video.is_empty():
		return
	var image: Image = _torqa.video_preview(_video, _marks[_selected].y)
	_picture.texture = ImageTexture.create_from_image(image) if image != null else null


func _distance_text(meters: float) -> String:
	if _imperial:
		return "%.2f mi" % (meters / 1000.0 / HudPanel.KM_PER_MILE)
	return "%.2f km" % (meters / 1000.0)


static func _time_text(seconds: float) -> String:
	return "%d:%04.1f" % [floori(seconds / 60.0), fmod(seconds, 60.0)]


func _on_visibility_changed() -> void:
	if not visible and _torqa != null:
		_torqa.close_video_preview()
