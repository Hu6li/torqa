class_name Minimap
extends Control
## The map around the rider. The close view turns with the direction of travel (heading up);
## clicking switches to the whole route, north up.
##
## The map itself (land, rivers, roads and the route) is drawn once at the view's scale and then
## only moved and turned as the rider goes: drawing a city's thousands of lines anew every frame
## held whole courses to ten frames a second. The rider, ghost and north mark are drawn over it
## every frame.

## A simulated ride's rider should jump to `position_m` (metres east/north of the start, #53).
signal jump_requested(position_m: Vector2)

const PADDING: float = 24.0
## Half the width of the close view.
const CLOSE_RADIUS_M: float = 450.0
const TRACK_COLOR: Color = Color(1.0, 0.56, 0.2)
const START_COLOR: Color = Color(0.3, 0.8, 0.45)
const FINISH_COLOR: Color = Color(0.92, 0.3, 0.3)

## The corner caption that switches between close view and whole route.
const CAPTION_WIDTH: float = 90.0
const CAPTION_HEIGHT: float = 28.0

## Clicks jump the rider there (simulated rides).
var jumpable: bool = false:
	set(value):
		jumpable = value
		tooltip_text = (
			tr("Click: jump there · corner: close view / whole route")
			if value
			else tr("Click: close view / whole route")
		)

## Metres east/north of the route start.
var _ghost: Vector2 = Vector2.ZERO
var _ghost_shown: bool = false
var _peers: Array[Dictionary] = []
var _track: PackedVector2Array = PackedVector2Array()
## The track's extent, for the whole-route view.
var _bounds: Rect2 = Rect2()
var _rider: Vector2 = Vector2.ZERO
## Direction of travel, radians clockwise from north.
var _heading: float = 0.0
## The land around the whole route, its rivers and roads over it, then the corridor in detail.
var _land: ArrayMesh
var _lines: Array[Dictionary] = []
var _corridor: ArrayMesh
var _background: Color = Color(0.15, 0.17, 0.16)
var _follow: bool = true
## The map, drawn in metres scaled by `_map_scale`: scaled when drawn rather than by the layer,
## so lines keep their widths in pixels. Each frame only moves and turns the layer.
var _map: Node2D = Node2D.new()
var _map_scale: Transform2D = Transform2D(Vector2.ZERO, Vector2.ZERO, Vector2.ZERO)
## The rider, ghost, north mark and caption over the map.
var _marks: Node2D = Node2D.new()


func set_track(track: PackedVector2Array) -> void:
	_track = track
	_bounds = Rect2(_track[0], Vector2.ZERO) if not _track.is_empty() else Rect2()
	for point: Vector2 in _track:
		_bounds = _bounds.expand(point)
	_redraw_map()


## Takes the maps from `OrqwaApp.minimap_mesh()`.
func set_map(map: Dictionary) -> void:
	_land = null
	_lines.clear()
	_corridor = null
	if not map.is_empty():
		_background = map["background"]
		var land: Dictionary = map["land"]
		_land = _mesh(land)
		var lines: Array = map["lines"]
		_lines.assign(lines)
		var corridor: Dictionary = map["corridor"]
		_corridor = _mesh(corridor)
	queue_redraw()
	_redraw_map()


## The ghost's position in metres east/north of the start, or hides it with `visible` false.
func set_ghost(position_m: Vector2, visible_on_map: bool) -> void:
	_ghost = position_m
	_ghost_shown = visible_on_map
	_marks.queue_redraw()


func set_rider(position_m: Vector2, heading: float) -> void:
	_rider = position_m
	_heading = heading
	_place_map()


## Marks the current authenticated group riders without rebuilding the background map.
func set_group_riders(peers: Array) -> void:
	_peers.assign(peers)
	_marks.queue_redraw()


func _ready() -> void:
	mouse_filter = MOUSE_FILTER_STOP
	jumpable = jumpable
	add_child(_map)
	add_child(_marks)
	_map.draw.connect(_draw_map)
	_marks.draw.connect(_draw_marks)
	resized.connect(_redraw_map)


func _gui_input(event: InputEvent) -> void:
	var click: InputEventMouseButton = event as InputEventMouseButton
	if click == null or not click.pressed or click.button_index != MOUSE_BUTTON_LEFT:
		return
	# In a simulated ride a click jumps there; the corner caption still switches the view.
	var caption: Rect2 = Rect2(0, size.y - CAPTION_HEIGHT, CAPTION_WIDTH, CAPTION_HEIGHT)
	if jumpable and not caption.has_point(click.position):
		jump_requested.emit(_view().affine_inverse() * click.position)
	else:
		_follow = not _follow
		_redraw_map()
	accept_event()


func _draw() -> void:
	draw_rect(Rect2(Vector2.ZERO, size), _background)


## Draws the map again at the view's current scale, after the map, the track, the view or the
## size changed.
func _redraw_map() -> void:
	_map_scale = Transform2D().scaled(_view().get_scale())
	_map.queue_redraw()
	_place_map()


## Moves and turns the map drawn at `_map_scale` to where the view has it now: the view is that
## scale followed by a turn and a shift, which leave the lines as they were drawn.
func _place_map() -> void:
	_marks.queue_redraw()
	if is_zero_approx(_map_scale.x.x):
		return
	_map.transform = _view() * _map_scale.affine_inverse()


func _draw_map() -> void:
	if is_zero_approx(_map_scale.x.x):
		return
	if _land != null:
		_map.draw_mesh(_land, null, _map_scale)
	for line: Dictionary in _lines:
		var points: PackedVector2Array = line["points"]
		var width: float = line["width"]
		var color: Color = line["color"]
		_map.draw_polyline(_map_scale * points, color, width, true)
	if _corridor != null:
		_map.draw_mesh(_corridor, null, _map_scale)
	if _track.size() >= 2:
		var track: PackedVector2Array = _map_scale * _track
		_map.draw_polyline(track, Color(0, 0, 0, 0.35), 6.0, true)
		_map.draw_polyline(track, TRACK_COLOR, 3.5, true)
		_map.draw_circle(track[0], 5.0, START_COLOR)
		_map.draw_circle(track[track.size() - 1], 5.0, FINISH_COLOR)


func _draw_marks() -> void:
	var view: Transform2D = _view()
	for peer: Dictionary in _peers:
		if not peer.has("x"):
			continue
		var id: int = peer["id"]
		var east: float = peer["x"]
		var north: float = peer["y"]
		var position: Vector2 = view * Vector2(east, north)
		_marks.draw_circle(position, 6.0, Color(0, 0, 0, 0.35))
		_marks.draw_circle(position, 4.5, UiTheme.group_color(id))
	if _ghost_shown:
		_marks.draw_circle(view * _ghost, 6.0, Color(0, 0, 0, 0.35))
		_marks.draw_circle(view * _ghost, 4.5, UiTheme.GHOST_COLOR)
	_draw_rider(view * _rider, _heading if not _follow else 0.0)
	if _follow:
		_draw_north(view)
	var font: Font = get_theme_default_font()
	var label: String = (tr("Close") if _follow else tr("Route")).to_upper()
	_marks.draw_string(
		font, Vector2(14, size.y - 12), label, HORIZONTAL_ALIGNMENT_LEFT, -1, 11, UiTheme.MUTED
	)


## Coloured triangles `{vertices, colors}` as a mesh; null without any.
static func _mesh(triangles: Dictionary) -> ArrayMesh:
	var vertices: PackedVector2Array = triangles["vertices"]
	if vertices.is_empty():
		return null
	var arrays: Array = []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = vertices
	arrays[Mesh.ARRAY_COLOR] = triangles["colors"]
	var mesh: ArrayMesh = ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	return mesh


## An arrow pointing in the direction of travel (`angle` clockwise from screen up).
func _draw_rider(center: Vector2, angle: float) -> void:
	var tip: Vector2 = Vector2(0, -11).rotated(angle)
	var left: Vector2 = Vector2(-7, 8).rotated(angle)
	var right: Vector2 = Vector2(7, 8).rotated(angle)
	var notch: Vector2 = Vector2(0, 4).rotated(angle)
	var arrow: PackedVector2Array = PackedVector2Array(
		[center + tip, center + right, center + notch, center + left]
	)
	_marks.draw_circle(center, 14.0, Color(0, 0, 0, 0.25))
	_marks.draw_colored_polygon(arrow, Color.WHITE)
	_marks.draw_polyline(arrow + PackedVector2Array([center + tip]), UiTheme.ACCENT, 1.5, true)


## A small "N" at the edge showing where north is in the rotated close view.
func _draw_north(view: Transform2D) -> void:
	var north: Vector2 = (view.basis_xform(Vector2(0, 1))).normalized()
	var center: Vector2 = size / 2.0
	var position: Vector2 = center + north * (minf(size.x, size.y) / 2.0 - 18.0)
	_marks.draw_circle(position, 10.0, Color(0, 0, 0, 0.45))
	var font: Font = get_theme_default_font()
	_marks.draw_string(
		font, position + Vector2(-4.5, 4.5), "N", HORIZONTAL_ALIGNMENT_LEFT, -1, 12, Color.WHITE
	)


## Map metres to control pixels: heading up around the rider, or the whole route north up.
func _view() -> Transform2D:
	if not _follow and _track.size() >= 2:
		var extent: Vector2 = _bounds.size.max(Vector2(1, 1))
		var available: Vector2 = size - Vector2(PADDING, PADDING) * 2.0
		var fit: float = minf(available.x / extent.x, available.y / extent.y)
		return (
			Transform2D()
			. translated(-_bounds.get_center())
			. scaled(Vector2(fit, -fit))
			. translated(size / 2.0)
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
