class_name PathCard
extends Control
## A course's route image (R37): the track in the logo blue on black, start and finish marked.
## The elevation profile is shown separately, on the course page.

const BLUE: Color = Color("#2EB0FF")
const BACKGROUND: Color = Color.BLACK
const START: Color = Color(0.3, 0.85, 0.45)
const FINISH: Color = Color.WHITE
const PADDING: float = 14.0
const RADIUS: int = 10

## Metres east/north of the start.
var _track: PackedVector2Array = PackedVector2Array()


## Shows a course's track (`TorqaApp.courses()` `track`).
func set_track(track: PackedVector2Array) -> void:
	_track = track
	queue_redraw()


func _draw() -> void:
	var background: StyleBoxFlat = StyleBoxFlat.new()
	background.bg_color = BACKGROUND
	background.set_corner_radius_all(RADIUS)
	background.anti_aliasing = true
	draw_style_box(background, Rect2(Vector2.ZERO, size))
	if _track.size() >= 2:
		_draw_track(Rect2(Vector2(PADDING, PADDING), size - Vector2(PADDING * 2.0, PADDING * 2.0)))


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
