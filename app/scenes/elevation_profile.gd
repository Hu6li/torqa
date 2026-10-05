class_name ElevationProfile
extends Control
## The route's elevation profile as a clean area chart: ridden part in the accent colour, the
## rest light, a crisp outline and the rider's position.

## A simulated ride's rider should jump to `distance_m` along the route (#53).
signal jump_requested(distance_m: float)

const PADDING: float = 2.0
## The smallest elevation range drawn, so flat routes do not look mountainous.
const MIN_RANGE_M: float = 40.0
const AHEAD: Color = Color(1, 1, 1, 0.13)
const OUTLINE: Color = Color(1, 1, 1, 0.75)

## Clicks jump the rider there (simulated rides).
var jumpable: bool = false:
	set(value):
		jumpable = value
		mouse_filter = MOUSE_FILTER_STOP if value else MOUSE_FILTER_IGNORE
		tooltip_text = tr("Click: jump there") if value else ""
		mouse_default_cursor_shape = CURSOR_POINTING_HAND if value else CURSOR_ARROW

## `(distance m, elevation m)` points.
var _profile: PackedVector2Array = PackedVector2Array()
var _rider_distance: float = 0.0
var _ghost_distance: float = -1.0
## `[start m, end m, colour]` per climb.
var _climbs: Array[Array] = []
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


## Marks the route's climbs (`TorqaApp.climbs()["climbs"]`) with a band in their category colour.
func set_climbs(climbs: Array) -> void:
	_climbs.clear()
	for climb: Dictionary in climbs:
		var category: String = climb["category"]
		_climbs.append(
			[climb["start_m"], climb["end_m"], UiTheme.CLIMB_COLORS.get(category, Color.GRAY)]
		)
	queue_redraw()


## The ghost's distance along the route, or a negative value to hide it.
func set_ghost_distance(distance_m: float) -> void:
	_ghost_distance = distance_m
	queue_redraw()


func set_rider_distance(distance_m: float) -> void:
	_rider_distance = distance_m
	queue_redraw()


func _draw() -> void:
	if _profile.size() < 2:
		return
	var bottom: float = size.y
	var ridden: Color = Color(UiTheme.ACCENT, 0.55)
	var outline: PackedVector2Array = PackedVector2Array()
	for i: int in range(_profile.size()):
		outline.append(_to_screen(_profile[i]))
	for i: int in range(outline.size() - 1):
		var a: Vector2 = outline[i]
		var b: Vector2 = outline[i + 1]
		var color: Color = ridden if _profile[i].x < _rider_distance else AHEAD
		# Drawn as quads without triangulation, which fails on flat segments at the bottom.
		draw_primitive(
			PackedVector2Array([a, b, Vector2(b.x, bottom), Vector2(a.x, bottom)]),
			PackedColorArray([color, color, color, color]),
			PackedVector2Array()
		)
	draw_polyline(outline, OUTLINE, 1.5, true)
	for climb: Array in _climbs:
		var start_m: float = climb[0]
		var end_m: float = climb[1]
		var color: Color = climb[2]
		var from: float = _to_screen(Vector2(start_m, 0.0)).x
		var to: float = _to_screen(Vector2(end_m, 0.0)).x
		draw_rect(Rect2(from, bottom - 3.0, to - from, 3.0), color)

	if _ghost_distance >= 0.0:
		var ghost: Vector2 = _to_screen(Vector2(_ghost_distance, _elevation_at(_ghost_distance)))
		draw_circle(ghost, 4.0, UiTheme.GHOST_COLOR)
	var rider: Vector2 = _to_screen(Vector2(_rider_distance, _elevation_at(_rider_distance)))
	draw_line(Vector2(rider.x, 0), Vector2(rider.x, bottom), Color(UiTheme.ACCENT, 0.9), 1.5)
	draw_circle(rider, 5.0, Color.WHITE)
	draw_circle(rider, 3.0, UiTheme.ACCENT)


func _elevation_at(distance_m: float) -> float:
	for i: int in range(_profile.size() - 1):
		var a: Vector2 = _profile[i]
		var b: Vector2 = _profile[i + 1]
		if distance_m <= b.x:
			var t: float = (distance_m - a.x) / maxf(b.x - a.x, 0.001)
			return lerpf(a.y, b.y, clampf(t, 0.0, 1.0))
	return _profile[_profile.size() - 1].y


func _gui_input(event: InputEvent) -> void:
	var click: InputEventMouseButton = event as InputEventMouseButton
	if not jumpable or _profile.size() < 2 or click == null or not click.pressed:
		return
	if click.button_index == MOUSE_BUTTON_LEFT:
		var length: float = _profile[_profile.size() - 1].x
		jump_requested.emit(clampf(click.position.x / maxf(size.x, 1.0), 0.0, 1.0) * length)
		accept_event()


func _to_screen(point: Vector2) -> Vector2:
	var length: float = maxf(_profile[_profile.size() - 1].x, 1.0)
	var height: float = size.y - PADDING * 2.0
	var x: float = point.x / length * size.x
	var y: float = (
		PADDING + (1.0 - (point.y - _min_elevation) / (_max_elevation - _min_elevation)) * height
	)
	return Vector2(x, y)
