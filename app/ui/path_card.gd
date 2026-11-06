class_name PathCard
extends Control
## A course's route image (R37): the track in the logo blue on the course's map where it has
## one (#192), else on black, start and finish marked. The elevation profile is shown
## separately, on the course page.

const BLUE: Color = Color("#2EB0FF")
const BACKGROUND: Color = Color.BLACK
const START: Color = Color(0.3, 0.85, 0.45)
const FINISH: Color = Color.WHITE
const PADDING: float = 14.0
const RADIUS: int = 10

## Metres east/north of the start.
var _track: PackedVector2Array = PackedVector2Array()
## The course's map with the track on it, drawn when the course was prepared (#192).
var _map: TextureRect = TextureRect.new()
## Draws the track over the map; clipped to the rounded card with it.
var _lines: Control = Control.new()
## A saved course position in route coordinates and its prepared map frame, if any.
var _marker: Dictionary = {}


func _init() -> void:
	clip_children = CanvasItem.CLIP_CHILDREN_ONLY
	_map.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_map.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	_map.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	_map.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_map.hide()
	add_child(_map)
	_lines.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_lines.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_lines.draw.connect(_draw_lines)
	_lines.resized.connect(_lines.queue_redraw)
	add_child(_lines)


## Shows the course's map under the track, `png` as the app drew it (with the track on it);
## empty for the plain black card.
func set_map(png: PackedByteArray) -> void:
	var image: Image = Image.new()
	# Only a PNG is tried: anything else would have Godot report an error for a bad picture.
	var signature: PackedByteArray = PackedByteArray([137, 80, 78, 71, 13, 10, 26, 10])
	if png.slice(0, 8) != signature or image.load_png_from_buffer(png) != OK:
		_map.texture = null
		_map.hide()
	else:
		_map.texture = ImageTexture.create_from_image(image)
		_map.show()
	_lines.queue_redraw()


## Whether a map shows under the track.
func has_map() -> bool:
	return _map.visible


## Shows a course's track (`OrqwaApp.courses()` `track`).
func set_track(track: PackedVector2Array) -> void:
	_track = track
	_marker = {}
	_lines.queue_redraw()


## Marks a saved position (`OrqwaApp.course_position()`); empty clears it.
func set_marker(marker: Dictionary) -> void:
	_marker = marker
	_lines.queue_redraw()


func _draw() -> void:
	var background: StyleBoxFlat = StyleBoxFlat.new()
	background.bg_color = BACKGROUND
	background.set_corner_radius_all(RADIUS)
	background.anti_aliasing = true
	draw_style_box(background, Rect2(Vector2.ZERO, size))


func _draw_lines() -> void:
	# The map picture carries the track already, placed to its own fit.
	if _track.size() >= 2 and not _map.visible:
		_draw_track(Rect2(Vector2(PADDING, PADDING), size - Vector2(PADDING * 2.0, PADDING * 2.0)))
	var marker: Vector2 = _marker_screen_position()
	if marker.is_finite():
		_lines.draw_circle(marker, 8.0, Color(BLUE, 0.3))
		_lines.draw_circle(marker, 5.0, Color.WHITE)
		_lines.draw_circle(marker, 3.0, BLUE)


func _draw_track(area: Rect2) -> void:
	var points: PackedVector2Array = _track_view(area) * _track
	_lines.draw_polyline(points, Color(BLUE, 0.25), 7.0, true)
	_lines.draw_polyline(points, BLUE, 2.5, true)
	_lines.draw_circle(points[points.size() - 1], 5.0, FINISH)
	_lines.draw_circle(points[0], 5.0, START)


func _track_view(area: Rect2) -> Transform2D:
	var low: Vector2 = _track[0]
	var high: Vector2 = _track[0]
	for point: Vector2 in _track:
		low = low.min(point)
		high = high.max(point)
	var extent: Vector2 = (high - low).max(Vector2(1.0, 1.0))
	# One scale for both axes, so the route keeps its shape; centred in the area.
	var scale_factor: float = minf(area.size.x / extent.x, area.size.y / extent.y)
	var offset: Vector2 = area.position + (area.size - extent * scale_factor) / 2.0
	return Transform2D().translated(-low).scaled(Vector2(scale_factor, -scale_factor)).translated(
		offset + Vector2(0.0, extent.y * scale_factor)
	)


func _marker_screen_position() -> Vector2:
	if _marker.is_empty():
		return Vector2.INF
	var position: Vector2 = _marker["position"]
	if _map.visible:
		var frame: Rect2 = _marker.get("frame", Rect2())
		if not frame.has_area():
			return Vector2.INF
		var picture: Vector2 = _map.texture.get_size()
		var fitted: Vector2 = picture * minf(size.x / picture.x, size.y / picture.y)
		var offset: Vector2 = (size - fitted) / 2.0
		var relative: Vector2 = (position - frame.position) / frame.size
		return offset + Vector2(relative.x, 1.0 - relative.y) * fitted
	if _track.size() < 2:
		return Vector2.INF
	var area: Rect2 = Rect2(Vector2(PADDING, PADDING), size - Vector2(PADDING * 2.0, PADDING * 2.0))
	return _track_view(area) * position
