class_name ClimbPanel
extends PanelContainer
## Progress on the climb the rider is on during a ride: its category in its colour, which of the
## route's climbs it is, and the ascent, distance and gradient still to go. Hidden between climbs.

const KM_PER_MILE: float = 1.609344
const METERS_PER_FOOT: float = 0.3048

var imperial: bool = false
var _title: Label = UiTheme.caption("")
var _left: Label = UiTheme.value(14)


func _init() -> void:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 2)
	rows.add_child(_title)
	rows.add_child(_left)
	var style: StyleBoxFlat = UiTheme.panel()
	style.content_margin_top = 10
	style.content_margin_bottom = 10
	add_theme_stylebox_override("panel", style)
	add_child(rows)
	hide()


## Shows `climb` (`ride_state()["climb"]`); null hides the panel.
func show_climb(climb: Variant) -> void:
	if climb == null:
		hide()
		return
	var info: Dictionary = climb
	var index: int = info["index"]
	var count: int = info["count"]
	var category: String = info["category"]
	var left_m: float = info["length_m"] - info["ridden_m"]
	var grade: float = info["grade"]
	var ascent_m: float = info["remaining_ascent_m"]
	_title.text = (tr("%s  ·  climb %d of %d") % [tr(category).to_upper(), index + 1, count])
	var color: Color = UiTheme.CLIMB_COLORS.get(category, UiTheme.MUTED)
	_title.add_theme_color_override("font_color", color)
	var ascent: String = (
		tr("%sft ascent") % roundi(ascent_m / METERS_PER_FOOT)
		if imperial
		else tr("%sm ascent") % roundi(ascent_m)
	)
	_left.text = tr("%s · %s · %.1f%% to go") % [ascent, short_distance(left_m, imperial), grade]
	show()


## A distance left on the road as the panels show it: "1.2km", "350m" or "0.75mi".
static func short_distance(meters: float, in_miles: bool) -> String:
	if in_miles:
		return "%.2fmi" % (meters / 1000.0 / KM_PER_MILE)
	return "%.1fkm" % (meters / 1000.0) if meters >= 1000.0 else "%dm" % roundi(meters)
