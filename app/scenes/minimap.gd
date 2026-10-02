class_name Minimap
extends Control
## Bird's-eye view of the route (north up) with the rider's position.

const PADDING: float = 20.0
const TRACK_COLOR: Color = Color(0.95, 0.55, 0.2)
const START_COLOR: Color = Color(0.3, 0.85, 0.4)
const FINISH_COLOR: Color = Color(0.9, 0.3, 0.3)

## Metres east/north of the route start.
var _track: PackedVector2Array = PackedVector2Array()
var _rider: Vector2 = Vector2.ZERO
var _scale: float = 1.0
var _offset: Vector2 = Vector2.ZERO


func set_track(track: PackedVector2Array) -> void:
	_track = track
	_fit()
	queue_redraw()


func set_rider(position_m: Vector2) -> void:
	_rider = position_m
	queue_redraw()


func _notification(what: int) -> void:
	if what == NOTIFICATION_RESIZED:
		_fit()


func _draw() -> void:
	draw_rect(Rect2(Vector2.ZERO, size), Color(0, 0, 0, 0.35))
	if _track.size() < 2:
		return
	var points: PackedVector2Array = PackedVector2Array()
	for point: Vector2 in _track:
		points.append(_to_screen(point))
	draw_polyline(points, TRACK_COLOR, 4.0, true)
	draw_circle(points[0], 7.0, START_COLOR)
	draw_circle(points[points.size() - 1], 7.0, FINISH_COLOR)
	var rider: Vector2 = _to_screen(_rider)
	draw_circle(rider, 10.0, Color.BLACK)
	draw_circle(rider, 7.0, Color.WHITE)


## Scales the track to fill the control while keeping its proportions.
func _fit() -> void:
	if _track.size() < 2:
		return
	var bounds: Rect2 = Rect2(_flip(_track[0]), Vector2.ZERO)
	for point: Vector2 in _track:
		bounds = bounds.expand(_flip(point))
	var available: Vector2 = size - Vector2(PADDING, PADDING) * 2.0
	var extent: Vector2 = bounds.size.max(Vector2(1, 1))
	_scale = minf(available.x / extent.x, available.y / extent.y)
	_offset = size / 2.0 - (bounds.position + bounds.size / 2.0) * _scale


func _to_screen(point: Vector2) -> Vector2:
	return _flip(point) * _scale + _offset


## North is up on screen, but screen y grows downwards.
static func _flip(point: Vector2) -> Vector2:
	return Vector2(point.x, -point.y)
