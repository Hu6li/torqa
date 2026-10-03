class_name CourseCard
extends PanelContainer
## A course in the gallery (R39): its path card, name and key figures; click to open it.

signal pressed

const WIDTH: float = 300.0


func _init(course: Dictionary, imperial: bool) -> void:
	custom_minimum_size = Vector2(WIDTH, 0)
	mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 8)
	rows.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var path: PathCard = PathCard.new()
	path.custom_minimum_size = Vector2(WIDTH - 32.0, 170)
	path.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var track: PackedVector2Array = course.get("track", PackedVector2Array())
	var profile: PackedVector2Array = course.get("profile", PackedVector2Array())
	path.set_preview(track, profile)
	rows.add_child(path)
	var title: Label = Label.new()
	title.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	title.text = course["name"]
	title.add_theme_font_size_override("font_size", 18)
	title.text_overrun_behavior = TextServer.OVERRUN_TRIM_ELLIPSIS
	rows.add_child(title)
	var figures: Label = Label.new()
	figures.text = figures_text(course, imperial)
	figures.add_theme_color_override("font_color", UiTheme.MUTED)
	rows.add_child(figures)
	add_child(rows)


## "12.4 km  ·  284 m climbing  ·  max 8 %", in the rider's units.
static func figures_text(course: Dictionary, imperial: bool) -> String:
	var length_km: float = course["length_m"] / 1000.0
	var gain_m: float = course["elevation_gain_m"]
	var max_grade: float = course["max_grade"]
	var distance: String = (
		"%.1f mi" % (length_km / HudPanel.KM_PER_MILE) if imperial else "%.1f km" % length_km
	)
	var climbing: String = (
		"%d ft" % roundi(gain_m / HudPanel.METERS_PER_FOOT) if imperial else "%d m" % roundi(gain_m)
	)
	return (
		TranslationServer.translate("%s  ·  %s climbing  ·  max %d %%")
		% [distance, climbing, roundi(max_grade)]
	)


func _gui_input(event: InputEvent) -> void:
	var click: InputEventMouseButton = event as InputEventMouseButton
	if click != null and click.pressed and click.button_index == MOUSE_BUTTON_LEFT:
		pressed.emit()
