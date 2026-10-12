class_name TrendChart
extends Control
## Values over the weeks or months of a period (R65): lines, such as the best 1, 5 and 20
## minutes and the FTP, and dots, such as FTP tests. Hovering reads a week or month off.

const FONT_SIZE: int = 11
const LEFT: float = 44.0
const RIGHT: float = 10.0
const TOP: float = 22.0
const BOTTOM: float = 20.0
const GRID: Color = Color(1, 1, 1, 0.07)

## The values' unit; "W/kg" shows them with one decimal.
var unit: String = "W"

## The start of each week or month (Unix seconds) and whether they are months.
var _starts: PackedInt64Array = PackedInt64Array()
var _monthly: bool = false
## Per series: `values` (negative where there is none), `color`, `name`, `dashed`, `dots`.
var _series: Array[Dictionary] = []
## The week or month under the pointer; -1 when the pointer is elsewhere.
var _hover: int = -1


func _init() -> void:
	mouse_filter = Control.MOUSE_FILTER_PASS


## Clears the chart for the weeks or months starting at `starts`.
func set_periods(starts: PackedInt64Array, monthly: bool) -> void:
	_starts = starts
	_monthly = monthly
	_series.clear()
	_hover = -1
	queue_redraw()


## A series of one value per week or month (negative where there is none), named `name` in
## the readout: a line, dashed if asked, or with `dots` single marks.
func add_series(
	values: PackedFloat64Array, color: Color, series_name: String, dashed: bool, dots: bool
) -> void:
	_series.append(
		{"values": values, "color": color, "name": series_name, "dashed": dashed, "dots": dots}
	)
	queue_redraw()


func _gui_input(event: InputEvent) -> void:
	var motion: InputEventMouseMotion = event as InputEventMouseMotion
	if motion == null or _starts.is_empty():
		return
	var share: float = clampf((motion.position.x - LEFT) / maxf(_plot().size.x, 1.0), 0.0, 0.999)
	_hover = int(share * _starts.size())
	queue_redraw()


func _notification(what: int) -> void:
	if what == NOTIFICATION_MOUSE_EXIT:
		_hover = -1
		queue_redraw()


func _draw() -> void:
	var span: Vector2 = _value_range()
	if _starts.is_empty() or span == Vector2.ZERO:
		return
	var font: Font = get_theme_default_font()
	var plot: Rect2 = _plot()
	for share: float in [0.0, 0.5, 1.0]:
		var value: float = lerpf(span.x, span.y, share)
		var y: float = _y(value, span)
		draw_line(Vector2(plot.position.x, y), Vector2(plot.end.x, y), GRID)
		draw_string(
			font,
			Vector2(0.0, y + FONT_SIZE * 0.35),
			_format(value, false),
			HORIZONTAL_ALIGNMENT_RIGHT,
			LEFT - 6.0,
			FONT_SIZE,
			UiTheme.MUTED
		)
	for series: Dictionary in _series:
		_draw_series(series, span)
	_draw_periods(font)
	if _hover >= 0 and _hover < _starts.size():
		_draw_readout(font, plot)


func _draw_series(series: Dictionary, span: Vector2) -> void:
	var values: PackedFloat64Array = series["values"]
	var color: Color = series["color"]
	var dashed: bool = series["dashed"]
	var dots: bool = series["dots"]
	var previous: Vector2 = Vector2(-1.0, -1.0)
	for i: int in range(mini(values.size(), _starts.size())):
		if values[i] < 0.0:
			previous = Vector2(-1.0, -1.0)
			continue
		var point: Vector2 = Vector2(_x(i), _y(values[i], span))
		if dots:
			draw_circle(point, 4.5, color)
			continue
		if previous.x >= 0.0:
			if dashed:
				draw_dashed_line(previous, point, color, 1.5, 6.0)
			else:
				draw_line(previous, point, color, 2.0, true)
		draw_circle(point, 2.5, color)
		previous = point


## About eight labels under the chart: months by name, weeks by their Monday.
func _draw_periods(font: Font) -> void:
	var count: int = _starts.size()
	var labels: int = mini(8, count)
	for k: int in range(labels):
		var i: int = roundi(float(k) * (count - 1) / maxf(labels - 1, 1))
		var text: String = _period_name(i, false)
		var width: float = font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, FONT_SIZE).x
		var x: float = clampf(_x(i) - width / 2.0, 0.0, size.x - width)
		draw_string(
			font,
			Vector2(x, size.y - 4.0),
			text,
			HORIZONTAL_ALIGNMENT_LEFT,
			-1,
			FONT_SIZE,
			UiTheme.MUTED
		)


## A line at the hovered week or month and, above the chart, each series' value there.
func _draw_readout(font: Font, plot: Rect2) -> void:
	var x: float = _x(_hover)
	draw_line(Vector2(x, plot.position.y), Vector2(x, plot.end.y), Color(1, 1, 1, 0.35))
	var parts: Array[Array] = [[_period_name(_hover, true), UiTheme.TEXT]]
	for series: Dictionary in _series:
		var values: PackedFloat64Array = series["values"]
		if _hover < values.size() and values[_hover] >= 0.0:
			var series_name: String = series["name"]
			parts.append(["%s %s" % [series_name, _format(values[_hover], true)], series["color"]])
	var left: float = plot.position.x
	for part: Array in parts:
		var text: String = part[0]
		var color: Color = part[1]
		draw_string(
			font, Vector2(left, FONT_SIZE + 2.0), text, HORIZONTAL_ALIGNMENT_LEFT, -1, 13, color
		)
		left += font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, 13).x + 18.0


## A month as "Oct" ("Oct 2026" in full), a week as "12 Oct" ("Week of 12 Oct").
func _period_name(index: int, full: bool) -> String:
	var parts: PackedStringArray = UiTheme.date(_starts[index]).get_slice(",", 0).split(" ")
	if parts.size() < 4:
		return ""
	if _monthly:
		return "%s %s" % [parts[2], parts[3]] if full else parts[2]
	var day: String = "%s %s" % [parts[1], parts[2]]
	return tr("Week of %s") % day if full else day


func _format(value: float, with_unit: bool) -> String:
	var number: String = "%.1f" % value if unit == "W/kg" else "%d" % roundi(value)
	return "%s %s" % [number, unit] if with_unit else number


## The values' range on the chart, with some room around them; zero without values.
func _value_range() -> Vector2:
	var low: float = INF
	var high: float = -INF
	for series: Dictionary in _series:
		var values: PackedFloat64Array = series["values"]
		for value: float in values:
			if value >= 0.0:
				low = minf(low, value)
				high = maxf(high, value)
	if high < low:
		return Vector2.ZERO
	var margin: float = maxf((high - low) * 0.15, high * 0.05)
	return Vector2(maxf(low - margin, 0.0), high + margin)


func _plot() -> Rect2:
	return Rect2(LEFT, TOP, size.x - LEFT - RIGHT, size.y - TOP - BOTTOM)


func _x(index: int) -> float:
	var plot: Rect2 = _plot()
	return plot.position.x + (index + 0.5) * plot.size.x / maxf(_starts.size(), 1.0)


func _y(value: float, span: Vector2) -> float:
	var plot: Rect2 = _plot()
	var share: float = (value - span.x) / maxf(span.y - span.x, 1e-6)
	return plot.end.y - clampf(share, 0.0, 1.0) * plot.size.y
