class_name Minimap
extends Control
## Map of the surroundings (north up) with the route and the rider. Click to switch between a
## close view following the rider and the whole route.

const PADDING: float = 20.0
## Half the width of the close view.
const CLOSE_RADIUS_M: float = 600.0
const TRACK_COLOR: Color = Color(0.95, 0.4, 0.15)
const START_COLOR: Color = Color(0.2, 0.75, 0.35)
const FINISH_COLOR: Color = Color(0.85, 0.2, 0.2)

## Metres east/north of the route start.
var _track: PackedVector2Array = PackedVector2Array()
var _rider: Vector2 = Vector2.ZERO
var _map: ArrayMesh
var _background: Color = Color(0.56, 0.68, 0.46)
var _follow: bool = true


func set_track(track: PackedVector2Array) -> void:
	_track = track
	queue_redraw()


## Takes the flat map from `TorqaApp.minimap_mesh()`.
func set_map(map: Dictionary) -> void:
	_map = null
	if map.is_empty():
		return
	var vertices: PackedVector2Array = map["vertices"]
	var colors: PackedColorArray = map["colors"]
	_background = map["background"]
	if vertices.is_empty():
		return
	var arrays: Array = []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = vertices
	arrays[Mesh.ARRAY_COLOR] = colors
	_map = ArrayMesh.new()
	_map.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	queue_redraw()


func set_rider(position_m: Vector2) -> void:
	_rider = position_m
	queue_redraw()


func _ready() -> void:
	clip_contents = true
	mouse_filter = MOUSE_FILTER_STOP
	tooltip_text = "Click: close view / whole route"


func _gui_input(event: InputEvent) -> void:
	var click: InputEventMouseButton = event as InputEventMouseButton
	if click != null and click.pressed and click.button_index == MOUSE_BUTTON_LEFT:
		_follow = not _follow
		queue_redraw()
		accept_event()


func _draw() -> void:
	draw_rect(Rect2(Vector2.ZERO, size), _background)
	var view: Transform2D = _view()
	if _map != null:
		draw_mesh(_map, null, view)
	if _track.size() >= 2:
		draw_polyline(view * _track, TRACK_COLOR, 4.0, true)
		draw_circle(view * _track[0], 7.0, START_COLOR)
		draw_circle(view * _track[_track.size() - 1], 7.0, FINISH_COLOR)
	var rider: Vector2 = view * _rider
	draw_circle(rider, 10.0, Color.BLACK)
	draw_circle(rider, 7.0, Color.WHITE)
	var label: String = "Close" if _follow else "Route"
	draw_string(get_theme_default_font(), Vector2(10, size.y - 10), label)


## Map metres (north up) to control pixels.
func _view() -> Transform2D:
	var center: Vector2 = _rider
	var scale_px: float = (minf(size.x, size.y) / 2.0) / CLOSE_RADIUS_M
	if not _follow and _track.size() >= 2:
		var bounds: Rect2 = Rect2(_track[0], Vector2.ZERO)
		for point: Vector2 in _track:
			bounds = bounds.expand(point)
		var extent: Vector2 = bounds.size.max(Vector2(1, 1))
		var available: Vector2 = size - Vector2(PADDING, PADDING) * 2.0
		scale_px = minf(available.x / extent.x, available.y / extent.y)
		center = bounds.get_center()
	# Screen y grows downwards, north is up.
	return Transform2D(
		Vector2(scale_px, 0),
		Vector2(0, -scale_px),
		size / 2.0 - Vector2(center.x, -center.y) * scale_px
	)
