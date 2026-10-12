class_name PowerCurveChart
extends Control
## Best power over the duration of an effort (R61): curves on a logarithmic time axis from a
## second to the longest effort, the first added on top. Hovering reads every curve off at the
## pointer.

## Durations marked on the time axis, in seconds.
const TICKS: Array[int] = [1, 5, 15, 60, 300, 1200, 3600, 10800]
const FONT_SIZE: int = 11
## Room for the value labels on the left, the duration labels below and the readout above.
const LEFT: float = 40.0
const RIGHT: float = 10.0
const TOP: float = 22.0
const BOTTOM: float = 20.0
const GRID: Color = Color(1, 1, 1, 0.07)

## The values' unit; "W/kg" shows them with one decimal.
var unit: String = "W":
	set(value):
		unit = value
		queue_redraw()

## Per curve: `points` `(duration s, value)`, `color` and `notes` per point for the readout.
var _curves: Array[Dictionary] = []
var _longest: float = 60.0
var _highest: float = 1.0
## The duration under the pointer, in seconds; negative when the pointer is elsewhere.
var _hover_s: float = -1.0


## "5 s", "20 min", "1 h", or m:ss and h:mm:ss between whole minutes and hours.
static func duration_label(seconds: float) -> String:
	var whole: int = roundi(seconds)
	if whole < 60:
		return TranslationServer.translate("%d s") % whole
	if whole % 3600 == 0:
		return TranslationServer.translate("%d h") % (whole / 3600)
	if whole < 3600 and whole % 60 == 0:
		return TranslationServer.translate("%d min") % (whole / 60)
	return UiTheme.duration(whole)


func _init() -> void:
	mouse_filter = Control.MOUSE_FILTER_PASS


## Removes all curves.
func clear() -> void:
	_curves.clear()
	_longest = 60.0
	_highest = 1.0
	queue_redraw()


## Adds a curve of `(duration s, value)` points in `color`, beneath those added before; `notes`
## per point follow its value in the readout, e.g. the date of the ride.
func add_curve(
	points: PackedVector2Array, color: Color, notes: PackedStringArray = PackedStringArray()
) -> void:
	if points.is_empty():
		return
	_curves.append({"points": points, "color": color, "notes": notes})
	for point: Vector2 in points:
		_longest = maxf(_longest, point.x)
		_highest = maxf(_highest, point.y)
	queue_redraw()


## Adds the best efforts of `TorqaApp.power_curves()` as a curve, in W/kg with `per_kg`, each
## noted with the date of its ride.
func add_efforts(efforts: Array, color: Color, per_kg: bool = false) -> void:
	var points: PackedVector2Array = PackedVector2Array()
	var notes: PackedStringArray = PackedStringArray()
	for effort: Dictionary in efforts:
		var duration_s: float = effort["duration_s"]
		var value: float = effort["watts_per_kg"] if per_kg else effort["power"]
		var start: int = effort["start_unix_s"]
		points.append(Vector2(duration_s, value))
		notes.append(UiTheme.date(start).get_slice(",", 0))
	add_curve(points, color, notes)


func _gui_input(event: InputEvent) -> void:
	var motion: InputEventMouseMotion = event as InputEventMouseMotion
	if motion == null:
		return
	var width: float = _plot().size.x
	var share: float = clampf((motion.position.x - LEFT) / maxf(width, 1.0), 0.0, 1.0)
	_hover_s = exp(share * log(_longest))
	queue_redraw()


func _notification(what: int) -> void:
	if what == NOTIFICATION_MOUSE_EXIT:
		_hover_s = -1.0
		queue_redraw()


func _draw() -> void:
	var plot: Rect2 = _plot()
	var font: Font = get_theme_default_font()
	var step: float = _value_step()
	var value: float = 0.0
	while value <= _top_value():
		var y: float = _y(value)
		draw_line(Vector2(plot.position.x, y), Vector2(plot.end.x, y), GRID)
		var text: String = _format(value, false)
		draw_string(
			font,
			Vector2(0.0, y + FONT_SIZE * 0.35),
			text,
			HORIZONTAL_ALIGNMENT_RIGHT,
			LEFT - 6.0,
			FONT_SIZE,
			UiTheme.MUTED
		)
		value += step
	for tick: int in TICKS:
		if tick > _longest:
			break
		var x: float = _x(tick)
		draw_line(Vector2(x, plot.position.y), Vector2(x, plot.end.y), GRID)
		var label: String = duration_label(tick)
		var width: float = font.get_string_size(label, HORIZONTAL_ALIGNMENT_LEFT, -1, FONT_SIZE).x
		draw_string(
			font,
			Vector2(clampf(x - width / 2.0, 0.0, size.x - width), size.y - 4.0),
			label,
			HORIZONTAL_ALIGNMENT_LEFT,
			-1,
			FONT_SIZE,
			UiTheme.MUTED
		)
	for i: int in range(_curves.size() - 1, -1, -1):
		var curve: Dictionary = _curves[i]
		var points: PackedVector2Array = curve["points"]
		var line: PackedVector2Array = PackedVector2Array()
		for point: Vector2 in points:
			line.append(Vector2(_x(point.x), _y(point.y)))
		var color: Color = curve["color"]
		if line.size() == 1:
			draw_circle(line[0], 3.0, color)
		else:
			draw_polyline(line, color, 2.5 if i == 0 else 1.5, true)
	if _hover_s > 0.0 and not _curves.is_empty():
		_draw_readout(font)


## A line at the pointer's duration and, above the chart, each curve's value there.
func _draw_readout(font: Font) -> void:
	var x: float = _x(_hover_s)
	var plot: Rect2 = _plot()
	draw_line(Vector2(x, plot.position.y), Vector2(x, plot.end.y), Color(1, 1, 1, 0.35))
	var parts: Array[Array] = []
	var shown_s: float = -1.0
	for curve: Dictionary in _curves:
		var points: PackedVector2Array = curve["points"]
		var index: int = _nearest(points, _hover_s)
		if index < 0:
			continue
		var point: Vector2 = points[index]
		if shown_s < 0.0:
			shown_s = point.x
		var notes: PackedStringArray = curve["notes"]
		var text: String = _format(point.y, true)
		if index < notes.size() and not notes[index].is_empty():
			text += "  ·  " + notes[index]
		parts.append([text, curve["color"]])
	if parts.is_empty():
		return
	var position_x: float = plot.position.x
	parts.push_front([duration_label(shown_s), UiTheme.TEXT])
	for part: Array in parts:
		var text: String = part[0]
		var color: Color = part[1]
		draw_string(
			font,
			Vector2(position_x, FONT_SIZE + 2.0),
			text,
			HORIZONTAL_ALIGNMENT_LEFT,
			-1,
			13,
			color
		)
		position_x += font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, 13).x + 22.0


## The point of `points` nearest `seconds` on the logarithmic axis, if it is not far beyond the
## curve's end; -1 otherwise.
func _nearest(points: PackedVector2Array, seconds: float) -> int:
	var best: int = -1
	var distance: float = INF
	for i: int in range(points.size()):
		var gap: float = absf(log(points[i].x) - log(seconds))
		if gap < distance:
			distance = gap
			best = i
	if best >= 0 and seconds > points[best].x * 1.5:
		return -1
	return best


func _format(value: float, with_unit: bool) -> String:
	var number: String = "%.1f" % value if unit == "W/kg" else "%d" % roundi(value)
	return "%s %s" % [number, unit] if with_unit else number


func _plot() -> Rect2:
	return Rect2(LEFT, TOP, size.x - LEFT - RIGHT, size.y - TOP - BOTTOM)


func _x(seconds: float) -> float:
	var plot: Rect2 = _plot()
	return plot.position.x + log(maxf(seconds, 1.0)) / log(_longest) * plot.size.x


func _y(value: float) -> float:
	var plot: Rect2 = _plot()
	return plot.end.y - value / _top_value() * plot.size.y


## The top of the value axis: some room above the highest value, on a grid line.
func _top_value() -> float:
	var step: float = _value_step()
	return ceilf(_highest * 1.08 / step) * step


## A round step between grid lines giving about four of them.
func _value_step() -> float:
	var rough: float = _highest / 4.0
	var magnitude: float = pow(10.0, floorf(log(rough) / log(10.0)))
	for factor: float in [1.0, 2.0, 2.5, 5.0, 10.0]:
		if rough <= factor * magnitude:
			return factor * magnitude
	return 10.0 * magnitude
