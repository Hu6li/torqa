class_name ElevationProfile
extends Control
## The route's elevation profile, coloured by gradient, with the rider's position.

const PADDING: float = 12.0
## The smallest elevation range drawn, so flat routes do not look mountainous.
const MIN_RANGE_M: float = 60.0
const RIDDEN_DARKEN: float = 0.5

## `(distance m, elevation m)` points.
var _profile: PackedVector2Array = PackedVector2Array()
var _rider_distance: float = 0.0
var _min_elevation: float = 0.0
var _max_elevation: float = 0.0


func set_profile(profile: PackedVector2Array) -> void:
	_profile = profile
	_rider_distance = 0.0
	if not _profile.is_empty():
		_min_elevation = _profile[0].y
		_max_elevation = _profile[0].y
		for point: Vector2 in _profile:
			_min_elevation = minf(_min_elevation, point.y)
			_max_elevation = maxf(_max_elevation, point.y)
		var missing: float = MIN_RANGE_M - (_max_elevation - _min_elevation)
		if missing > 0.0:
			_min_elevation -= missing / 2.0
			_max_elevation += missing / 2.0
	queue_redraw()


func set_rider_distance(distance_m: float) -> void:
	_rider_distance = distance_m
	queue_redraw()


func _draw() -> void:
	draw_rect(Rect2(Vector2.ZERO, size), Color(0, 0, 0, 0.35))
	if _profile.size() < 2:
		return
	var bottom: float = size.y - PADDING
	for i: int in range(_profile.size() - 1):
		var a: Vector2 = _profile[i]
		var b: Vector2 = _profile[i + 1]
		var screen_a: Vector2 = _to_screen(a)
		var screen_b: Vector2 = _to_screen(b)
		if screen_b.x - screen_a.x < 0.01:
			continue
		var grade: float = (b.y - a.y) / (b.x - a.x) * 100.0
		var color: Color = _grade_color(grade)
		if a.x < _rider_distance:
			color = color.darkened(RIDDEN_DARKEN)
		var quad: PackedVector2Array = PackedVector2Array(
			[screen_a, screen_b, Vector2(screen_b.x, bottom), Vector2(screen_a.x, bottom)]
		)
		draw_colored_polygon(quad, color)

	var font: Font = get_theme_default_font()
	var top_label: String = "%d m" % roundi(_max_elevation)
	var bottom_label: String = "%d m" % roundi(_min_elevation)
	draw_string(font, Vector2(PADDING, PADDING + 16), top_label)
	draw_string(font, Vector2(PADDING, bottom - 4), bottom_label)

	var rider_x: float = _to_screen(Vector2(_rider_distance, _min_elevation)).x
	draw_line(Vector2(rider_x, PADDING), Vector2(rider_x, bottom), Color.WHITE, 3.0)


func _to_screen(point: Vector2) -> Vector2:
	var length: float = maxf(_profile[_profile.size() - 1].x, 1.0)
	var width: float = size.x - PADDING * 2.0
	var height: float = size.y - PADDING * 2.0
	var x: float = PADDING + point.x / length * width
	var y: float = (
		PADDING + (1.0 - (point.y - _min_elevation) / (_max_elevation - _min_elevation)) * height
	)
	return Vector2(x, y)


static func _grade_color(grade: float) -> Color:
	if grade < 0.0:
		return Color(0.35, 0.6, 0.9)
	if grade < 3.0:
		return Color(0.35, 0.8, 0.4)
	if grade < 6.0:
		return Color(0.95, 0.8, 0.25)
	if grade < 9.0:
		return Color(0.95, 0.5, 0.2)
	return Color(0.9, 0.25, 0.25)
