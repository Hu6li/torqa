class_name CourseCard
extends PanelContainer
## A course in the gallery (R39): its path card, name and key figures in rows, with the time
## the rider is estimated to take (R67), marked if it is ridden along a video (R17); click to
## open it, or reach it with the keyboard or the remote and press OK (#228), when a ring shows
## it has the focus. The elevation profile is left to the course page. The gallery sets its
## width, so its cards share the row; the route grows with it.

signal pressed

## The narrowest a card gets.
const WIDTH: float = 300.0
## The route's height to its width, as the course map is drawn (`course_map::PICTURE`).
const MAP_ASPECT: float = 640.0 / 1072.0

var _path_card: PathCard = PathCard.new()
var _title: Label = Label.new()
var _press_attempt: bool = false


func _init(course: Dictionary, imperial: bool) -> void:
	custom_minimum_size = Vector2(WIDTH, 0)
	mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	focus_mode = Control.FOCUS_ALL
	focus_entered.connect(queue_redraw)
	focus_exited.connect(queue_redraw)
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 12)
	rows.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_path_card.custom_minimum_size = Vector2(WIDTH - 32.0, (WIDTH - 32.0) * MAP_ASPECT)
	_path_card.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_path_card.resized.connect(_fit_map)
	var track: PackedVector2Array = course.get("track", PackedVector2Array())
	_path_card.set_track(track)
	rows.add_child(_path_card)
	if course.get("in_progress", false):
		var margin: MarginContainer = MarginContainer.new()
		margin.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
		margin.mouse_filter = Control.MOUSE_FILTER_IGNORE
		for side: String in ["left", "top", "right", "bottom"]:
			margin.add_theme_constant_override("margin_" + side, 10)
		var badge: PanelContainer = PanelContainer.new()
		badge.mouse_filter = Control.MOUSE_FILTER_IGNORE
		badge.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
		badge.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
		badge.add_theme_stylebox_override("panel", UiTheme.chip(true))
		var label: Label = Label.new()
		label.text = tr("In progress")
		label.add_theme_font_size_override("font_size", 12)
		badge.add_child(label)
		margin.add_child(badge)
		_path_card.add_child(margin)
	_title.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_title.text = course["name"]
	_title.add_theme_font_size_override("font_size", 18)
	_title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_title.text_overrun_behavior = TextServer.OVERRUN_TRIM_ELLIPSIS
	# Sees the mouse for its tooltip; the click still reaches the card.
	_title.mouse_filter = Control.MOUSE_FILTER_PASS
	_title.resized.connect(_fit_title)
	var heading: HBoxContainer = HBoxContainer.new()
	heading.mouse_filter = Control.MOUSE_FILTER_IGNORE
	heading.add_child(_title)
	if not str(course.get("video", "")).is_empty():
		heading.add_child(video_badge())
	rows.add_child(heading)
	var grid: GridContainer = figure_rows(course, imperial)
	# From the profile the course file keeps (R67); the course page rides the whole route.
	if course.get("estimated_s") != null:
		var seconds: float = course["estimated_s"]
		add_figure(grid, TranslationServer.translate("Time estimate"), estimate(seconds))
	var countries: Array = course.get("countries", [])
	_add_countries(grid, countries)
	rows.add_child(grid)
	add_child(rows)


## Shows the course's map under its route, `png` as the app drew it (#192); empty for none.
func set_map(png: PackedByteArray) -> void:
	_path_card.set_map(png)


## The route as tall as the map is for the card's width, so it fills the card uncropped.
func _fit_map() -> void:
	_path_card.custom_minimum_size.y = roundf(_path_card.size.x * MAP_ASPECT)


## A name the card's width cuts off shows in full on hover, as file managers do; one that fits
## needs no tooltip.
func _fit_title() -> void:
	var font: Font = _title.get_theme_font("font")
	var size: int = _title.get_theme_font_size("font_size")
	var width: float = font.get_string_size(_title.text, HORIZONTAL_ALIGNMENT_LEFT, -1, size).x
	_title.tooltip_text = _title.text if width > _title.size.x else ""


## The name shown on hover; empty while the whole name fits on the card.
func name_tooltip() -> String:
	return _title.tooltip_text


## The mark of a video course.
static func video_badge() -> PanelContainer:
	var badge: PanelContainer = PanelContainer.new()
	badge.mouse_filter = Control.MOUSE_FILTER_IGNORE
	badge.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	badge.add_theme_stylebox_override("panel", UiTheme.chip(true))
	var label: Label = Label.new()
	label.text = TranslationServer.translate("Video")
	label.add_theme_font_size_override("font_size", 12)
	badge.add_child(label)
	return badge


## The key figures, one per row: caption on the left, value on the right, in the rider's units.
static func figure_rows(course: Dictionary, imperial: bool) -> GridContainer:
	var grid: GridContainer = GridContainer.new()
	grid.columns = 2
	grid.mouse_filter = Control.MOUSE_FILTER_IGNORE
	grid.add_theme_constant_override("h_separation", 16)
	grid.add_theme_constant_override("v_separation", 6)
	for figure: PackedStringArray in figures(course, imperial):
		add_figure(grid, TranslationServer.translate(figure[0]), figure[1])
	return grid


## Adds a row to a grid of `figure_rows`: `caption`, translated already, and `value`.
static func add_figure(grid: GridContainer, caption: String, value: String) -> void:
	var caption_label: Label = Label.new()
	caption_label.text = caption
	caption_label.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	caption_label.add_theme_color_override("font_color", UiTheme.MUTED)
	caption_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	grid.add_child(caption_label)
	var value_label: Label = Label.new()
	value_label.text = value
	value_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	grid.add_child(value_label)


## The countries the course passes (#230) by their codes, e.g. "CH · DE · FR", their names
## on hover; nothing where they are not known.
static func _add_countries(grid: GridContainer, countries: Array) -> void:
	if countries.is_empty():
		return
	var codes: PackedStringArray = PackedStringArray()
	var names: PackedStringArray = PackedStringArray()
	for country: Dictionary in countries:
		var code: String = country["code"]
		var country_name: String = country["name"]
		codes.append(code)
		names.append(country_name)
	var caption: String = (
		TranslationServer.translate("Country")
		if countries.size() == 1
		else TranslationServer.translate("Countries")
	)
	add_figure(grid, caption, " · ".join(codes))
	var value: Label = grid.get_child(grid.get_child_count() - 1) as Label
	value.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	value.tooltip_text = ", ".join(names)
	# Sees the mouse for its tooltip; the click still reaches the card.
	value.mouse_filter = Control.MOUSE_FILTER_PASS


## An estimated time to the nearest 5 minutes, which is all an estimate can tell (R66, R67):
## "1 h 05" or "45 min", and "5 min" at the least.
static func estimate(seconds: float) -> String:
	var minutes: int = maxi(5, roundi(seconds / 300.0) * 5)
	if minutes < 60:
		return TranslationServer.translate("%d min") % minutes
	return TranslationServer.translate("%d h %02d") % [minutes / 60, minutes % 60]


## `[caption, value]` pairs: length, climbing and steepest gradient, in the rider's units.
static func figures(course: Dictionary, imperial: bool) -> Array[PackedStringArray]:
	var length_km: float = course["length_m"] / 1000.0
	var gain_m: float = course["elevation_gain_m"]
	var max_grade: float = course["max_grade"]
	var distance: String = (
		"%.1f mi" % (length_km / HudPanel.KM_PER_MILE) if imperial else "%.1f km" % length_km
	)
	var climbing: String = (
		"%d ft" % roundi(gain_m / HudPanel.METERS_PER_FOOT) if imperial else "%d m" % roundi(gain_m)
	)
	# i18n-begin
	return [
		PackedStringArray(["Length", distance]),
		PackedStringArray(["Climbing", climbing]),
		PackedStringArray(["Steepest", "%d %%" % roundi(max_grade)]),
	]
	# i18n-end


func _gui_input(event: InputEvent) -> void:
	var click: InputEventMouseButton = event as InputEventMouseButton
	if click != null and click.button_index == MOUSE_BUTTON_LEFT:
		if click.pressed:
			_press_attempt = true
		else:
			var activate: bool = (
				_press_attempt and Rect2(Vector2.ZERO, size).has_point(click.position)
			)
			_press_attempt = false
			if activate and not click.canceled:
				pressed.emit()
	elif event.is_action_pressed(&"ui_accept"):
		accept_event()
		pressed.emit()


func _notification(what: int) -> void:
	if (
		what
		in [
			NOTIFICATION_SCROLL_BEGIN,
			NOTIFICATION_DRAG_BEGIN,
			NOTIFICATION_FOCUS_EXIT,
			NOTIFICATION_EXIT_TREE
		]
	):
		_press_attempt = false
	elif what == NOTIFICATION_VISIBILITY_CHANGED and not is_visible_in_tree():
		_press_attempt = false


## On the card's own edge: the gallery's scrolling would cut a ring outside it.
func _draw() -> void:
	if has_focus():
		draw_style_box(UiTheme.focus_ring(UiTheme.RADIUS, 0), Rect2(Vector2.ZERO, size))
