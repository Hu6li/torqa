class_name WeeklyBars
extends Control
## The weeks of a period as bars (R64): one value a week, or several stacked in their colours,
## such as the time in each zone. Hovering reads a week off.

const FONT_SIZE: int = 11
const LEFT: float = 52.0
const RIGHT: float = 10.0
const TOP: float = 22.0
const BOTTOM: float = 20.0
const GRID: Color = Color(1, 1, 1, 0.07)

## Per week: the start of its Monday (Unix seconds) and its stacked values.
var _starts: PackedInt64Array = PackedInt64Array()
var _stacks: Array[PackedFloat64Array] = []
var _colors: Array[Color] = []
## Names of the stacked parts for the readout, e.g. "Z2"; empty for one value a week.
var _names: PackedStringArray = PackedStringArray()
## Turns a value into text for the axis and the readout.
var _format: Callable = func(value: float) -> String: return "%d" % roundi(value)
## The week under the pointer; -1 when the pointer is elsewhere.
var _hover: int = -1


func _init() -> void:
	mouse_filter = Control.MOUSE_FILTER_PASS


## Bars for the weeks starting at `starts`, each the `stacks` of that week drawn bottom up in
## `colors`, the parts named `names` in the readout; `format` writes a value.
func set_weeks(
	starts: PackedInt64Array,
	stacks: Array[PackedFloat64Array],
	colors: Array[Color],
	names: PackedStringArray,
	format: Callable
) -> void:
	_starts = starts
	_stacks = stacks
	_colors = colors
	_names = names
	_format = format
	_hover = -1
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
	if _starts.is_empty():
		return
	var font: Font = get_theme_default_font()
	var plot: Rect2 = _plot()
	var top: float = 0.0
	for stack: PackedFloat64Array in _stacks:
		top = maxf(top, _total(stack))
	if top <= 0.0:
		top = 1.0
	for share: float in [0.0, 0.5, 1.0]:
		var y: float = plot.end.y - share * plot.size.y
		draw_line(Vector2(plot.position.x, y), Vector2(plot.end.x, y), GRID)
		var text: String = _format.call(top * share)
		draw_string(
			font,
			Vector2(0.0, y + FONT_SIZE * 0.35),
			text,
			HORIZONTAL_ALIGNMENT_RIGHT,
			LEFT - 6.0,
			FONT_SIZE,
			UiTheme.MUTED
		)
	var step: float = plot.size.x / _starts.size()
	var gap: float = clampf(step * 0.25, 1.0, 10.0)
	for i: int in range(_starts.size()):
		var bottom: float = plot.end.y
		var stack: PackedFloat64Array = _stacks[i]
		for part: int in range(stack.size()):
			var height: float = stack[part] / top * plot.size.y
			if height <= 0.0:
				continue
			var color: Color = _colors[part % _colors.size()]
			if _hover >= 0 and i != _hover:
				color.a *= 0.55
			draw_rect(
				Rect2(plot.position.x + i * step + gap / 2.0, bottom - height, step - gap, height),
				color
			)
			bottom -= height
	_draw_dates(font, plot, step)
	if _hover >= 0 and _hover < _starts.size():
		_draw_readout(font, plot)


func _draw_dates(font: Font, plot: Rect2, step: float) -> void:
	var count: int = _starts.size()
	var labels: int = mini(6, count)
	for k: int in range(labels):
		var i: int = roundi(float(k) * (count - 1) / maxf(labels - 1, 1))
		var text: String = FitnessChart.short_date(_starts[i])
		var width: float = font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, FONT_SIZE).x
		var x: float = clampf(plot.position.x + (i + 0.5) * step - width / 2.0, 0.0, size.x - width)
		draw_string(
			font,
			Vector2(x, size.y - 4.0),
			text,
			HORIZONTAL_ALIGNMENT_LEFT,
			-1,
			FONT_SIZE,
			UiTheme.MUTED
		)


## Above the chart: the week, its total and, for stacked parts, each part.
func _draw_readout(font: Font, plot: Rect2) -> void:
	var stack: PackedFloat64Array = _stacks[_hover]
	var total: String = _format.call(_total(stack))
	var parts: Array[Array] = [
		[tr("Week of %s") % FitnessChart.short_date(_starts[_hover]), UiTheme.TEXT],
		[total, UiTheme.TEXT],
	]
	if not _names.is_empty():
		for part: int in range(mini(stack.size(), _names.size())):
			if stack[part] > 0.0:
				var value: String = _format.call(stack[part])
				parts.append(["%s %s" % [_names[part], value], _colors[part % _colors.size()]])
	var left: float = plot.position.x
	for part: Array in parts:
		var text: String = part[0]
		var color: Color = part[1]
		draw_string(
			font, Vector2(left, FONT_SIZE + 2.0), text, HORIZONTAL_ALIGNMENT_LEFT, -1, 13, color
		)
		left += font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, 13).x + 18.0


func _plot() -> Rect2:
	return Rect2(LEFT, TOP, size.x - LEFT - RIGHT, size.y - TOP - BOTTOM)


static func _total(stack: PackedFloat64Array) -> float:
	var total: float = 0.0
	for value: float in stack:
		total += value
	return total
