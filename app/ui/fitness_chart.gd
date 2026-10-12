class_name FitnessChart
extends Control
## Fitness, fatigue and form over the days of a period (R64): fitness and fatigue as lines over
## each day's training stress, form below them in its bands. Hovering reads a day off.

const FONT_SIZE: int = 11
const LEFT: float = 40.0
const RIGHT: float = 70.0
const TOP: float = 22.0
const BOTTOM: float = 20.0
## The share of the height for fitness and fatigue; form gets the rest.
const UPPER: float = 0.6
const GAP: float = 16.0
const GRID: Color = Color(1, 1, 1, 0.07)
const FITNESS_COLOR: Color = UiTheme.POWER_COLOR
const FATIGUE_COLOR: Color = Color(0.8, 0.45, 0.95)
const FORM_COLOR: Color = Color(0.98, 0.8, 0.2)
const STRESS_COLOR: Color = Color(1, 1, 1, 0.12)
## Form at least this far above and below zero, so its bands show.
const FORM_RANGE: float = 40.0
## The bands of form, as intervals.icu names them: from, to, name and colour.
# i18n-begin
const FORM_BANDS: Array[Array] = [
	[25.0, INF, "Transition", Color(0.98, 0.8, 0.2, 0.08)],
	[5.0, 25.0, "Fresh", Color(0.25, 0.6, 0.95, 0.12)],
	[-10.0, 5.0, "Grey zone", Color(1, 1, 1, 0.04)],
	[-30.0, -10.0, "Optimal", Color(0.3, 0.8, 0.45, 0.12)],
	[-INF, -30.0, "High risk", Color(0.95, 0.3, 0.3, 0.12)],
]
# i18n-end

var _days: PackedInt64Array = PackedInt64Array()
var _stress: PackedFloat64Array = PackedFloat64Array()
var _fitness: PackedFloat64Array = PackedFloat64Array()
var _fatigue: PackedFloat64Array = PackedFloat64Array()
var _form: PackedFloat64Array = PackedFloat64Array()
## The day under the pointer; -1 when the pointer is elsewhere.
var _hover: int = -1


## The name of the band `form` falls in.
static func band(form: float) -> String:
	for entry: Array in FORM_BANDS:
		var low: float = entry[0]
		var high: float = entry[1]
		if form >= low and form < high:
			var band_name: String = entry[2]
			return band_name
	return ""


## "12 Oct": a day's date without weekday and year.
static func short_date(unix_s: int) -> String:
	var parts: PackedStringArray = UiTheme.date(unix_s).get_slice(",", 0).split(" ")
	return "%s %s" % [parts[1], parts[2]] if parts.size() >= 3 else ""


func _init() -> void:
	mouse_filter = Control.MOUSE_FILTER_PASS


## The days as `TorqaApp.fitness()` gives them: each day's start, stress, fitness, fatigue and
## form.
func set_days(data: Dictionary) -> void:
	_days = data.get("day_unix_s", PackedInt64Array())
	_stress = data.get("stress", PackedFloat64Array())
	_fitness = data.get("fitness", PackedFloat64Array())
	_fatigue = data.get("fatigue", PackedFloat64Array())
	_form = data.get("form", PackedFloat64Array())
	_hover = -1
	queue_redraw()


func _gui_input(event: InputEvent) -> void:
	var motion: InputEventMouseMotion = event as InputEventMouseMotion
	if motion == null or _days.is_empty():
		return
	var width: float = maxf(_upper().size.x, 1.0)
	var share: float = clampf((motion.position.x - LEFT) / width, 0.0, 0.999)
	_hover = int(share * _days.size())
	queue_redraw()


func _notification(what: int) -> void:
	if what == NOTIFICATION_MOUSE_EXIT:
		_hover = -1
		queue_redraw()


func _draw() -> void:
	if _days.is_empty():
		return
	var font: Font = get_theme_default_font()
	var upper: Rect2 = _upper()
	var lower: Rect2 = _lower()
	var top_load: float = _top_load()
	var form_range: float = _form_range()
	# Form's bands, named at the right.
	for entry: Array in FORM_BANDS:
		var band_low: float = entry[0]
		var band_high: float = entry[1]
		var low: float = maxf(band_low, -form_range)
		var high: float = minf(band_high, form_range)
		if high <= low:
			continue
		var y_high: float = _form_y(high, lower, form_range)
		var y_low: float = _form_y(low, lower, form_range)
		var color: Color = entry[3]
		draw_rect(Rect2(lower.position.x, y_high, lower.size.x, y_low - y_high), color)
		var band_name: String = entry[2]
		draw_string(
			font,
			Vector2(lower.end.x + 6.0, (y_high + y_low) / 2.0 + FONT_SIZE * 0.35),
			tr(band_name),
			HORIZONTAL_ALIGNMENT_LEFT,
			RIGHT - 6.0,
			FONT_SIZE,
			UiTheme.MUTED
		)
	var zero: float = _form_y(0.0, lower, form_range)
	draw_line(Vector2(lower.position.x, zero), Vector2(lower.end.x, zero), GRID)
	for value: float in [-form_range, 0.0, form_range]:
		_value_label(
			font, "%+d" % roundi(value) if value != 0.0 else "0", _form_y(value, lower, form_range)
		)
	# Fitness and fatigue over the days' stress.
	for value: float in [0.0, top_load / 2.0, top_load]:
		var y: float = _load_y(value, upper, top_load)
		draw_line(Vector2(upper.position.x, y), Vector2(upper.end.x, y), GRID)
		_value_label(font, "%d" % roundi(value), y)
	var step: float = upper.size.x / _days.size()
	for i: int in range(_days.size()):
		if _stress[i] <= 0.0:
			continue
		var y: float = _load_y(_stress[i], upper, top_load)
		draw_rect(
			Rect2(upper.position.x + i * step, y, maxf(step - 1.0, 1.0), upper.end.y - y),
			STRESS_COLOR
		)
	_draw_line(
		_fitness,
		upper,
		func(v: float) -> float: return _load_y(v, upper, top_load),
		FITNESS_COLOR,
		2.5
	)
	_draw_line(
		_fatigue,
		upper,
		func(v: float) -> float: return _load_y(v, upper, top_load),
		FATIGUE_COLOR,
		1.5
	)
	_draw_line(
		_form, lower, func(v: float) -> float: return _form_y(v, lower, form_range), FORM_COLOR, 2.0
	)
	_draw_dates(font, lower)
	if _hover >= 0 and _hover < _days.size():
		_draw_readout(font, upper, lower)


func _draw_line(
	values: PackedFloat64Array, pane: Rect2, y_of: Callable, color: Color, width: float
) -> void:
	if values.size() < 2:
		return
	var step: float = pane.size.x / values.size()
	var points: PackedVector2Array = PackedVector2Array()
	for i: int in range(values.size()):
		var y: float = y_of.call(values[i])
		points.append(Vector2(pane.position.x + (i + 0.5) * step, y))
	draw_polyline(points, color, width, true)


## About five dates under the chart, evenly spread.
func _draw_dates(font: Font, pane: Rect2) -> void:
	var count: int = _days.size()
	var labels: int = mini(5, count)
	var step: float = pane.size.x / count
	for k: int in range(labels):
		var i: int = roundi(float(k) * (count - 1) / maxf(labels - 1, 1))
		var text: String = short_date(_days[i])
		var width: float = font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, FONT_SIZE).x
		var x: float = clampf(pane.position.x + (i + 0.5) * step - width / 2.0, 0.0, size.x - width)
		draw_string(
			font,
			Vector2(x, size.y - 4.0),
			text,
			HORIZONTAL_ALIGNMENT_LEFT,
			-1,
			FONT_SIZE,
			UiTheme.MUTED
		)


## A line at the hovered day and, above the chart, its date and figures.
func _draw_readout(font: Font, upper: Rect2, lower: Rect2) -> void:
	var step: float = upper.size.x / _days.size()
	var x: float = upper.position.x + (_hover + 0.5) * step
	draw_line(Vector2(x, upper.position.y), Vector2(x, lower.end.y), Color(1, 1, 1, 0.35))
	var parts: Array[Array] = [
		[UiTheme.date(_days[_hover]).get_slice(",", 0), UiTheme.TEXT],
		[tr("Fitness %d") % roundi(_fitness[_hover]), FITNESS_COLOR],
		[tr("Fatigue %d") % roundi(_fatigue[_hover]), FATIGUE_COLOR],
		[tr("Form %+d") % roundi(_form[_hover]), FORM_COLOR],
		[tr("TSS %d") % roundi(_stress[_hover]), UiTheme.MUTED],
	]
	var left: float = upper.position.x
	for part: Array in parts:
		var text: String = part[0]
		var color: Color = part[1]
		draw_string(
			font, Vector2(left, FONT_SIZE + 2.0), text, HORIZONTAL_ALIGNMENT_LEFT, -1, 13, color
		)
		left += font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, 13).x + 22.0


func _value_label(font: Font, text: String, y: float) -> void:
	draw_string(
		font,
		Vector2(0.0, y + FONT_SIZE * 0.35),
		text,
		HORIZONTAL_ALIGNMENT_RIGHT,
		LEFT - 6.0,
		FONT_SIZE,
		UiTheme.MUTED
	)


func _upper() -> Rect2:
	var height: float = size.y - TOP - BOTTOM - GAP
	return Rect2(LEFT, TOP, size.x - LEFT - RIGHT, height * UPPER)


func _lower() -> Rect2:
	var upper: Rect2 = _upper()
	var height: float = size.y - TOP - BOTTOM - GAP
	return Rect2(LEFT, upper.end.y + GAP, upper.size.x, height * (1.0 - UPPER))


## The top of the load axis: a round number above the highest fitness, fatigue or stress.
func _top_load() -> float:
	var highest: float = 10.0
	for series: PackedFloat64Array in [_stress, _fitness, _fatigue]:
		for value: float in series:
			highest = maxf(highest, value)
	return ceilf(highest * 1.05 / 10.0) * 10.0


func _form_range() -> float:
	var furthest: float = FORM_RANGE
	for value: float in _form:
		furthest = maxf(furthest, absf(value) * 1.1)
	return ceilf(furthest / 10.0) * 10.0


func _load_y(value: float, pane: Rect2, top: float) -> float:
	return pane.end.y - clampf(value / top, 0.0, 1.0) * pane.size.y


func _form_y(value: float, pane: Rect2, form_range: float) -> float:
	var share: float = (clampf(value, -form_range, form_range) + form_range) / (2.0 * form_range)
	return pane.end.y - share * pane.size.y
