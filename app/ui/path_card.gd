class_name PathCard
extends Control
## A course's route image (R37): the track in the logo blue on black, start and finish marked,
## and the elevation profile as a strip underneath in the same blue.

const BLUE: Color = Color("#2EB0FF")
const BACKGROUND: Color = Color.BLACK
const START: Color = Color(0.3, 0.85, 0.45)
const FINISH: Color = Color.WHITE
## Share of the height taken by the elevation strip.
const STRIP: float = 0.24
const PADDING: float = 14.0
const RADIUS: int = 10

## Metres east/north of the start, and (distance, elevation).
var _track: PackedVector2Array = PackedVector2Array()
var _profile: PackedVector2Array = PackedVector2Array()


## Shows a course from its preview (`TorqaApp.courses()` track and profile).
func set_preview(track: PackedVector2Array, profile: PackedVector2Array) -> void:
	_track = track
	_profile = profile
	queue_redraw()


func _draw() -> void:
	var background: StyleBoxFlat = StyleBoxFlat.new()
	background.bg_color = BACKGROUND
	background.set_corner_radius_all(RADIUS)
	background.anti_aliasing = true
	draw_style_box(background, Rect2(Vector2.ZERO, size))
	var strip_height: float = size.y * STRIP if _profile.size() >= 2 else 0.0
	var map_area: Rect2 = Rect2(
		Vector2(PADDING, PADDING), size - Vector2(PADDING * 2.0, PADDING * 2.0 + strip_height)
	)
	if _track.size() >= 2:
		_draw_track(map_area)
	if strip_height > 0.0:
		_draw_strip(Rect2(0.0, size.y - strip_height, size.x, strip_height))


func _draw_track(area: Rect2) -> void:
	var low: Vector2 = _track[0]
	var high: Vector2 = _track[0]
	for point: Vector2 in _track:
		low = low.min(point)
		high = high.max(point)
	var extent: Vector2 = (high - low).max(Vector2(1.0, 1.0))
	# One scale for both axes, so the route keeps its shape; centred in the area.
	var scale_factor: float = minf(area.size.x / extent.x, area.size.y / extent.y)
	var offset: Vector2 = area.position + (area.size - extent * scale_factor) / 2.0
	var points: PackedVector2Array = PackedVector2Array()
	for point: Vector2 in _track:
		# North is up: screen y grows downwards.
		var local: Vector2 = point - low
		points.append(offset + Vector2(local.x, extent.y - local.y) * scale_factor)
	draw_polyline(points, Color(BLUE, 0.25), 7.0, true)
	draw_polyline(points, BLUE, 2.5, true)
	draw_circle(points[points.size() - 1], 5.0, FINISH)
	draw_circle(points[0], 5.0, START)


func _draw_strip(area: Rect2) -> void:
	var length: float = maxf(_profile[_profile.size() - 1].x, 1.0)
	var low: float = _profile[0].y
	var high: float = _profile[0].y
	for point: Vector2 in _profile:
		low = minf(low, point.y)
		high = maxf(high, point.y)
	# Flat routes stay flat rather than being stretched into mountains.
	high = maxf(high, low + 50.0)
	var top: float = area.position.y + 4.0
	var bottom: float = area.end.y - 6.0
	var fill: Color = Color(BLUE, 0.35)
	var outline: PackedVector2Array = PackedVector2Array()
	for point: Vector2 in _profile:
		var x: float = area.position.x + PADDING + point.x / length * (area.size.x - PADDING * 2.0)
		var y: float = lerpf(bottom, top, (point.y - low) / (high - low))
		outline.append(Vector2(x, y))
	for i: int in range(outline.size() - 1):
		var a: Vector2 = outline[i]
		var b: Vector2 = outline[i + 1]
		draw_primitive(
			PackedVector2Array([a, b, Vector2(b.x, bottom), Vector2(a.x, bottom)]),
			PackedColorArray([fill, fill, fill, fill]),
			PackedVector2Array()
		)
	draw_polyline(outline, BLUE, 1.5, true)
