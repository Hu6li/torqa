class_name UiTheme
extends RefCounted
## The app's visual style: dark translucent panels, pill buttons and the brand accent.

const ACCENT: Color = Color(0.04, 0.61, 0.96)
const TEXT: Color = Color(0.94, 0.95, 0.97)
const MUTED: Color = Color(0.64, 0.68, 0.74)
const PANEL: Color = Color(0.06, 0.07, 0.09, 0.72)
const SURFACE: Color = Color(1, 1, 1, 0.07)
const RADIUS: int = 14
## Power zones 1–7 (Coggan): name and colour, as commonly used by training platforms.
const POWER_ZONES: Array[Array] = [
	["Recovery", Color(0.6, 0.62, 0.66)],
	["Endurance", Color(0.25, 0.6, 0.95)],
	["Tempo", Color(0.3, 0.8, 0.45)],
	["Threshold", Color(0.98, 0.8, 0.2)],
	["VO2max", Color(0.98, 0.55, 0.2)],
	["Anaerobic", Color(0.95, 0.3, 0.3)],
	["Neuromuscular", Color(0.7, 0.4, 0.95)],
]
## Heart-rate zones 1–5: name and colour.
const HEART_RATE_ZONES: Array[Array] = [
	["Very light", Color(0.6, 0.62, 0.66)],
	["Light", Color(0.25, 0.6, 0.95)],
	["Moderate", Color(0.3, 0.8, 0.45)],
	["Hard", Color(0.98, 0.55, 0.2)],
	["Maximum", Color(0.95, 0.3, 0.3)],
]
## Climb categories as labelled by Torqa, easiest first.
const CLIMB_COLORS: Dictionary[String, Color] = {
	"Climb": Color(0.6, 0.62, 0.66),
	"Cat 4": Color(0.3, 0.8, 0.45),
	"Cat 3": Color(0.98, 0.8, 0.2),
	"Cat 2": Color(0.98, 0.55, 0.2),
	"Cat 1": Color(0.95, 0.3, 0.3),
	"HC": Color(0.7, 0.4, 0.95),
}
const POWER_COLOR: Color = Color(0.04, 0.61, 0.96)
const HEART_RATE_COLOR: Color = Color(0.95, 0.33, 0.38)


static func build() -> Theme:
	var theme: Theme = Theme.new()
	theme.default_font_size = 15

	theme.set_stylebox("panel", "PanelContainer", panel())
	theme.set_stylebox("panel", "Panel", panel())

	theme.set_color("font_color", "Label", TEXT)

	for type: String in ["Button", "OptionButton", "CheckBox"]:
		theme.set_color("font_color", type, TEXT)
		theme.set_color("font_hover_color", type, TEXT)
		theme.set_color("font_pressed_color", type, Color.WHITE)
		theme.set_color("font_disabled_color", type, Color(TEXT, 0.35))
	for type: String in ["Button", "OptionButton"]:
		theme.set_stylebox("normal", type, _box(SURFACE, 10, 14, 9))
		theme.set_stylebox("hover", type, _box(Color(1, 1, 1, 0.13), 10, 14, 9))
		theme.set_stylebox("pressed", type, _box(Color(ACCENT, 0.85), 10, 14, 9))
		theme.set_stylebox("disabled", type, _box(Color(1, 1, 1, 0.04), 10, 14, 9))
		theme.set_stylebox("focus", type, StyleBoxEmpty.new())
	theme.set_stylebox("normal", "CheckBox", StyleBoxEmpty.new())
	theme.set_stylebox("hover", "CheckBox", StyleBoxEmpty.new())
	theme.set_stylebox("pressed", "CheckBox", StyleBoxEmpty.new())
	theme.set_stylebox("focus", "CheckBox", StyleBoxEmpty.new())

	var field: StyleBoxFlat = _box(SURFACE, 10, 12, 8)
	theme.set_stylebox("normal", "LineEdit", field)
	theme.set_stylebox("focus", "LineEdit", _box(Color(1, 1, 1, 0.12), 10, 12, 8))
	theme.set_color("font_color", "LineEdit", TEXT)

	theme.set_stylebox("background", "ProgressBar", _box(Color(1, 1, 1, 0.1), 6, 0, 4))
	theme.set_stylebox("fill", "ProgressBar", _box(ACCENT, 6, 0, 4))

	theme.set_stylebox("slider", "HSlider", _box(Color(1, 1, 1, 0.14), 4, 0, 3))
	theme.set_stylebox("grabber_area", "HSlider", _box(ACCENT, 4, 0, 3))
	theme.set_stylebox("grabber_area_highlight", "HSlider", _box(ACCENT, 4, 0, 3))

	var popup: StyleBoxFlat = _box(Color(0.09, 0.1, 0.12, 0.98), 10, 6, 6)
	theme.set_stylebox("panel", "PopupMenu", popup)
	theme.set_stylebox("hover", "PopupMenu", _box(Color(ACCENT, 0.35), 6, 8, 4))
	theme.set_color("font_color", "PopupMenu", TEXT)
	return theme


## The translucent card used for HUD and setup panels.
static func panel() -> StyleBoxFlat:
	var box: StyleBoxFlat = _box(PANEL, RADIUS, 16, 14)
	box.border_color = Color(1, 1, 1, 0.07)
	box.set_border_width_all(1)
	box.shadow_color = Color(0, 0, 0, 0.22)
	box.shadow_size = 12
	box.anti_aliasing = true
	return box


## A caption label: small, muted, upper case.
static func caption(text: String) -> Label:
	var label: Label = Label.new()
	label.text = text.to_upper()
	label.add_theme_font_size_override("font_size", 11)
	label.add_theme_color_override("font_color", MUTED)
	return label


## A value label in the given size.
static func value(size: int) -> Label:
	var label: Label = Label.new()
	label.text = "--"
	label.add_theme_font_size_override("font_size", size)
	return label


static func _box(color: Color, radius: int, horizontal: int, vertical: int) -> StyleBoxFlat:
	var box: StyleBoxFlat = StyleBoxFlat.new()
	box.bg_color = color
	box.set_corner_radius_all(radius)
	box.content_margin_left = horizontal
	box.content_margin_right = horizontal
	box.content_margin_top = vertical
	box.content_margin_bottom = vertical
	box.anti_aliasing = true
	return box


## A time as m:ss, or h:mm:ss from an hour.
static func duration(seconds: float) -> String:
	var total: int = roundi(seconds)
	if total >= 3600:
		return "%d:%02d:%02d" % [total / 3600, total / 60 % 60, total % 60]
	return "%d:%02d" % [total / 60, total % 60]
