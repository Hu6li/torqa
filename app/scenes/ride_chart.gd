class_name RideChart
extends Control
## A recorded ride over time: elevation as a muted area, power and heart rate as lines, each
## scaled to its own range. Live during a workout, heart rate can instead keep a fixed range
## with the target to hold drawn across it (R56).

const PADDING: float = 6.0
const ELEVATION: Color = Color(1, 1, 1, 0.08)

## Heart rate drawn from `x` to `y` bpm instead of its own range; zero for its own.
var heart_rate_range: Vector2 = Vector2.ZERO:
	set(value):
		heart_rate_range = value
		queue_redraw()
## A heart rate to hold, drawn as a dashed line; 0 for none.
var heart_rate_target: float = 0.0:
	set(value):
		heart_rate_target = value
		queue_redraw()
## The time axis spans at least this many seconds, so a live chart does not stretch its start.
var min_duration: float = 1.0

var _elevation: PackedVector2Array = PackedVector2Array()
var _power: PackedVector2Array = PackedVector2Array()
var _heart_rate: PackedVector2Array = PackedVector2Array()
var _duration: float = 1.0


## Series of `(elapsed s, value)` points, as `TorqaApp.ride_detail()` returns them.
func set_series(
	elevation: PackedVector2Array, power: PackedVector2Array, heart_rate: PackedVector2Array
) -> void:
	_elevation = elevation
	_power = power
	_heart_rate = heart_rate
	_duration = maxf(min_duration, 1.0)
	for series: PackedVector2Array in [elevation, power, heart_rate]:
		if not series.is_empty():
			_duration = maxf(_duration, series[series.size() - 1].x)
	queue_redraw()


func _draw() -> void:
	if _elevation.size() >= 2:
		var area: PackedVector2Array = _scaled(_elevation, _range(_elevation, 30.0, false))
		var bottom: float = size.y
		for i: int in range(area.size() - 1):
			var a: Vector2 = area[i]
			var b: Vector2 = area[i + 1]
			draw_primitive(
				PackedVector2Array([a, b, Vector2(b.x, bottom), Vector2(a.x, bottom)]),
				PackedColorArray([ELEVATION, ELEVATION, ELEVATION, ELEVATION]),
				PackedVector2Array()
			)
	var heart_range: Vector2 = heart_rate_range
	if heart_range == Vector2.ZERO and _heart_rate.size() >= 2:
		heart_range = _range(_heart_rate, 20.0, false)
	if heart_rate_target > 0.0 and heart_range != Vector2.ZERO:
		var y: float = _y(heart_rate_target, heart_range)
		var color: Color = UiTheme.HEART_RATE_COLOR
		color.a = 0.55
		draw_dashed_line(Vector2(0.0, y), Vector2(size.x, y), color, 1.0, 6.0)
	if _heart_rate.size() >= 2:
		draw_polyline(_scaled(_heart_rate, heart_range), UiTheme.HEART_RATE_COLOR, 1.5, true)
	if _power.size() >= 2:
		draw_polyline(_scaled(_power, _range(_power, 50.0, true)), UiTheme.POWER_COLOR, 1.5, true)


## The value range of a series, at least `min_range` so flat data stays flat. Power reads best
## from zero, with headroom above the highest value.
func _range(series: PackedVector2Array, min_range: float, from_zero: bool) -> Vector2:
	var low: float = series[0].y
	var high: float = series[0].y
	for point: Vector2 in series:
		low = minf(low, point.y)
		high = maxf(high, point.y)
	if from_zero:
		low = 0.0
		high *= 1.2
	return Vector2(low, maxf(high, low + min_range))


## Screen points for a series drawn over `value_range`.
func _scaled(series: PackedVector2Array, value_range: Vector2) -> PackedVector2Array:
	var points: PackedVector2Array = PackedVector2Array()
	for point: Vector2 in series:
		points.append(Vector2(point.x / _duration * size.x, _y(point.y, value_range)))
	return points


func _y(value: float, value_range: Vector2) -> float:
	var height: float = size.y - 2.0 * PADDING
	var share: float = (value - value_range.x) / maxf(value_range.y - value_range.x, 1e-6)
	return PADDING + height * (1.0 - clampf(share, 0.0, 1.0))
