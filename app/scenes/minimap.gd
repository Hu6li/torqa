class_name Minimap
extends Control
## The map around the rider. The close view turns with the direction of travel (heading up);
## clicking switches to the whole route, north up.

const PADDING: float = 24.0
## Half the width of the close view.
const CLOSE_RADIUS_M: float = 450.0
const TRACK_COLOR: Color = Color(1.0, 0.56, 0.2)
const START_COLOR: Color = Color(0.3, 0.8, 0.45)
const FINISH_COLOR: Color = Color(0.92, 0.3, 0.3)

## Metres east/north of the route start.
var _ghost: Vector2 = Vector2.ZERO
var _ghost_shown: bool = false
var _track: PackedVector2Array = PackedVector2Array()
var _rider: Vector2 = Vector2.ZERO
## Direction of travel, radians clockwise from north.
var _heading: float = 0.0
var _map: ArrayMesh
var _background: Color = Color(0.15, 0.17, 0.16)
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


## The ghost's position in metres east/north of the start, or hides it with `visible` false.
func set_ghost(position_m: Vector2, visible_on_map: bool) -> void:
	_ghost = position_m
	_ghost_shown = visible_on_map
	queue_redraw()


func set_rider(position_m: Vector2, heading: float) -> void:
	_rider = position_m
	_heading = heading
	queue_redraw()


func _ready() -> void:
	mouse_filter = MOUSE_FILTER_STOP
	tooltip_text = tr("Click: close view / whole route")


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
		draw_polyline(view * _track, Color(0, 0, 0, 0.35), 6.0, true)
		draw_polyline(view * _track, TRACK_COLOR, 3.5, true)
		draw_circle(view * _track[0], 5.0, START_COLOR)
		draw_circle(view * _track[_track.size() - 1], 5.0, FINISH_COLOR)
	if _ghost_shown:
		draw_circle(view * _ghost, 6.0, Color(0, 0, 0, 0.35))
		draw_circle(view * _ghost, 4.5, UiTheme.GHOST_COLOR)
	_draw_rider(view * _rider, _heading if not _follow else 0.0)
	if _follow:
		_draw_north(view)
	var font: Font = get_theme_default_font()
	var label: String = (tr("Close") if _follow else tr("Route")).to_upper()
	draw_string(
		font, Vector2(14, size.y - 12), label, HORIZONTAL_ALIGNMENT_LEFT, -1, 11, UiTheme.MUTED
	)


## An arrow pointing in the direction of travel (`angle` clockwise from screen up).
func _draw_rider(center: Vector2, angle: float) -> void:
	var tip: Vector2 = Vector2(0, -11).rotated(angle)
	var left: Vector2 = Vector2(-7, 8).rotated(angle)
	var right: Vector2 = Vector2(7, 8).rotated(angle)
	var notch: Vector2 = Vector2(0, 4).rotated(angle)
	var arrow: PackedVector2Array = PackedVector2Array(
		[center + tip, center + right, center + notch, center + left]
	)
	draw_circle(center, 14.0, Color(0, 0, 0, 0.25))
	draw_colored_polygon(arrow, Color.WHITE)
	draw_polyline(arrow + PackedVector2Array([center + tip]), UiTheme.ACCENT, 1.5, true)


## A small "N" at the edge showing where north is in the rotated close view.
func _draw_north(view: Transform2D) -> void:
	var north: Vector2 = (view.basis_xform(Vector2(0, 1))).normalized()
	var center: Vector2 = size / 2.0
	var position: Vector2 = center + north * (minf(size.x, size.y) / 2.0 - 18.0)
	draw_circle(position, 10.0, Color(0, 0, 0, 0.45))
	var font: Font = get_theme_default_font()
	draw_string(
		font, position + Vector2(-4.5, 4.5), "N", HORIZONTAL_ALIGNMENT_LEFT, -1, 12, Color.WHITE
	)


## Map metres to control pixels: heading up around the rider, or the whole route north up.
func _view() -> Transform2D:
	if not _follow and _track.size() >= 2:
		var bounds: Rect2 = Rect2(_track[0], Vector2.ZERO)
		for point: Vector2 in _track:
			bounds = bounds.expand(point)
		var extent: Vector2 = bounds.size.max(Vector2(1, 1))
		var available: Vector2 = size - Vector2(PADDING, PADDING) * 2.0
		var fit: float = minf(available.x / extent.x, available.y / extent.y)
		return Transform2D().translated(-bounds.get_center()).scaled(Vector2(fit, -fit)).translated(
			size / 2.0
		)
	var scale_px: float = (minf(size.x, size.y) / 2.0) / CLOSE_RADIUS_M
	# Turning the map by the heading (counter-clockwise in map space) puts travel at the top.
	return (
		Transform2D()
		. translated(-_rider)
		. rotated(_heading)
		. scaled(Vector2(scale_px, -scale_px))
		. translated(size / 2.0 + Vector2(0, size.y * 0.15))
	)
