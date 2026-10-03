class_name RideChart
extends Control
## A recorded ride over time: elevation as a muted area, power and heart rate as lines, each
## scaled to its own range.

const PADDING: float = 6.0
const ELEVATION: Color = Color(1, 1, 1, 0.08)

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
	_duration = 1.0
	for series: PackedVector2Array in [elevation, power, heart_rate]:
		if not series.is_empty():
			_duration = maxf(_duration, series[series.size() - 1].x)
	queue_redraw()


func _draw() -> void:
	if _elevation.size() >= 2:
		var area: PackedVector2Array = _scaled(_elevation, 30.0, false)
		var bottom: float = size.y
		for i: int in range(area.size() - 1):
			var a: Vector2 = area[i]
			var b: Vector2 = area[i + 1]
			draw_primitive(
				PackedVector2Array([a, b, Vector2(b.x, bottom), Vector2(a.x, bottom)]),
				PackedColorArray([ELEVATION, ELEVATION, ELEVATION, ELEVATION]),
				PackedVector2Array()
			)
	if _heart_rate.size() >= 2:
		draw_polyline(_scaled(_heart_rate, 20.0, false), UiTheme.HEART_RATE_COLOR, 1.5, true)
	if _power.size() >= 2:
		draw_polyline(_scaled(_power, 50.0, true), UiTheme.POWER_COLOR, 1.5, true)


## Screen points for a series, its value range at least `min_range` so flat data stays flat.
## Power reads best from zero, with headroom above the highest value.
func _scaled(series: PackedVector2Array, min_range: float, from_zero: bool) -> PackedVector2Array:
	var low: float = series[0].y
	var high: float = series[0].y
	for point: Vector2 in series:
		low = minf(low, point.y)
		high = maxf(high, point.y)
	if from_zero:
		low = 0.0
		high *= 1.2
	if high - low < min_range:
		high = low + min_range
	var points: PackedVector2Array = PackedVector2Array()
	var height: float = size.y - 2.0 * PADDING
	for point: Vector2 in series:
		points.append(
			Vector2(
				point.x / _duration * size.x,
				PADDING + height * (1.0 - (point.y - low) / (high - low))
			)
		)
	return points
